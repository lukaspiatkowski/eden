/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Tests for the Bookmarks store.

#![deny(warnings)]

use anyhow::{Error, Result};
use bookmarks::{
    Bookmark, BookmarkKind, BookmarkName, BookmarkPagination, BookmarkPrefix, BookmarkUpdateLog,
    BookmarkUpdateLogEntry, BookmarkUpdateReason, Bookmarks, Freshness, RawBundleReplayData,
};
use context::CoreContext;
use dbbookmarks::SqlBookmarksBuilder;
use fbinit::FacebookInit;
use futures::stream::TryStreamExt;
use maplit::hashmap;
use mononoke_types::Timestamp;
use mononoke_types_mocks::changesetid::{
    FIVES_CSID, FOURS_CSID, ONES_CSID, SIXES_CSID, THREES_CSID, TWOS_CSID,
};
use mononoke_types_mocks::repo::{REPO_ONE, REPO_TWO, REPO_ZERO};
use sql::mysql_async::{prelude::ConvIr, Value};
use sql_construct::SqlConstruct;
use std::collections::HashMap;

fn create_bookmark_name(book: &str) -> BookmarkName {
    BookmarkName::new(book.to_string()).unwrap()
}

fn create_prefix(book: &str) -> BookmarkPrefix {
    BookmarkPrefix::new(book.to_string()).unwrap()
}

fn compare_log_entries(
    expected_entries: Vec<BookmarkUpdateLogEntry>,
    actual_entries: Vec<BookmarkUpdateLogEntry>,
) {
    assert_eq!(expected_entries.len(), actual_entries.len());
    for i in 0..expected_entries.len() {
        let expected = expected_entries.get(i).unwrap();
        let actual = actual_entries.get(i).unwrap();
        assert_eq!(expected.id, actual.id);
        assert_eq!(expected.repo_id, actual.repo_id);
        assert_eq!(expected.bookmark_name, actual.bookmark_name);
        assert_eq!(expected.to_changeset_id, actual.to_changeset_id);
        assert_eq!(expected.from_changeset_id, actual.from_changeset_id);
        assert_eq!(expected.reason, actual.reason);
    }
}

#[fbinit::test]
fn test_simple_unconditional_set_get(fb: FacebookInit) {
    async_unit::tokio_unit_test(async move {
        let ctx = CoreContext::test_mock(fb);
        let bookmarks = SqlBookmarksBuilder::with_sqlite_in_memory()
            .unwrap()
            .with_repo_id(REPO_ZERO);
        let name_correct = create_bookmark_name("book");
        let name_incorrect = create_bookmark_name("book2");

        let mut txn = bookmarks.create_transaction(ctx.clone());
        txn.force_set(
            &name_correct,
            ONES_CSID,
            BookmarkUpdateReason::TestMove,
            None,
        )
        .unwrap();
        assert!(txn.commit().await.unwrap());

        assert_eq!(
            bookmarks.get(ctx.clone(), &name_correct).await.unwrap(),
            Some(ONES_CSID)
        );
        assert_eq!(
            bookmarks.get(ctx.clone(), &name_incorrect).await.unwrap(),
            None
        );

        compare_log_entries(
            bookmarks
                .read_next_bookmark_log_entries(ctx.clone(), 0, 1, Freshness::MostRecent)
                .try_collect::<Vec<_>>()
                .await
                .unwrap(),
            vec![BookmarkUpdateLogEntry {
                id: 1,
                repo_id: REPO_ZERO,
                bookmark_name: name_correct,
                to_changeset_id: Some(ONES_CSID),
                from_changeset_id: None,
                reason: BookmarkUpdateReason::TestMove,
                timestamp: Timestamp::now(),
                bundle_replay_data: None,
            }],
        );
    })
}

#[fbinit::test]
fn test_multi_unconditional_set_get(fb: FacebookInit) {
    async_unit::tokio_unit_test(async move {
        let ctx = CoreContext::test_mock(fb);
        let bookmarks = SqlBookmarksBuilder::with_sqlite_in_memory()
            .unwrap()
            .with_repo_id(REPO_ZERO);
        let name_1 = create_bookmark_name("book");
        let name_2 = create_bookmark_name("book2");

        let mut txn = bookmarks.create_transaction(ctx.clone());
        txn.force_set(&name_1, ONES_CSID, BookmarkUpdateReason::TestMove, None)
            .unwrap();
        txn.force_set(&name_2, TWOS_CSID, BookmarkUpdateReason::TestMove, None)
            .unwrap();
        assert!(txn.commit().await.unwrap());

        assert_eq!(
            bookmarks.get(ctx.clone(), &name_1).await.unwrap(),
            Some(ONES_CSID)
        );

        assert_eq!(
            bookmarks.get(ctx.clone(), &name_2).await.unwrap(),
            Some(TWOS_CSID)
        );
    })
}

#[fbinit::test]
fn test_unconditional_set_same_bookmark(fb: FacebookInit) {
    async_unit::tokio_unit_test(async move {
        let ctx = CoreContext::test_mock(fb);
        let bookmarks = SqlBookmarksBuilder::with_sqlite_in_memory()
            .unwrap()
            .with_repo_id(REPO_ZERO);
        let name_1 = create_bookmark_name("book");

        let mut txn = bookmarks.create_transaction(ctx.clone());
        txn.force_set(&name_1, ONES_CSID, BookmarkUpdateReason::TestMove, None)
            .unwrap();
        assert!(txn.commit().await.unwrap());

        let mut txn = bookmarks.create_transaction(ctx.clone());
        txn.force_set(&name_1, ONES_CSID, BookmarkUpdateReason::TestMove, None)
            .unwrap();
        assert!(txn.commit().await.unwrap());

        assert_eq!(
            bookmarks.get(ctx.clone(), &name_1).await.unwrap(),
            Some(ONES_CSID)
        );
    })
}

#[fbinit::test]
fn test_simple_create(fb: FacebookInit) {
    async_unit::tokio_unit_test(async move {
        let ctx = CoreContext::test_mock(fb);
        let bookmarks = SqlBookmarksBuilder::with_sqlite_in_memory()
            .unwrap()
            .with_repo_id(REPO_ZERO);
        let name_1 = create_bookmark_name("book");

        let mut txn = bookmarks.create_transaction(ctx.clone());
        txn.create(&name_1, ONES_CSID, BookmarkUpdateReason::TestMove, None)
            .unwrap();
        assert!(txn.commit().await.unwrap());

        assert_eq!(
            bookmarks.get(ctx.clone(), &name_1).await.unwrap(),
            Some(ONES_CSID)
        );

        compare_log_entries(
            bookmarks
                .read_next_bookmark_log_entries(ctx.clone(), 0, 1, Freshness::MostRecent)
                .try_collect::<Vec<_>>()
                .await
                .unwrap(),
            vec![BookmarkUpdateLogEntry {
                id: 1,
                repo_id: REPO_ZERO,
                bookmark_name: name_1,
                to_changeset_id: Some(ONES_CSID),
                from_changeset_id: None,
                reason: BookmarkUpdateReason::TestMove,
                timestamp: Timestamp::now(),
                bundle_replay_data: None,
            }],
        );
    })
}

#[fbinit::test]
fn test_create_already_existing(fb: FacebookInit) {
    async_unit::tokio_unit_test(async move {
        let ctx = CoreContext::test_mock(fb);
        let bookmarks = SqlBookmarksBuilder::with_sqlite_in_memory()
            .unwrap()
            .with_repo_id(REPO_ZERO);
        let name_1 = create_bookmark_name("book");

        let mut txn = bookmarks.create_transaction(ctx.clone());
        txn.create(&name_1, ONES_CSID, BookmarkUpdateReason::TestMove, None)
            .unwrap();
        assert!(txn.commit().await.unwrap());

        let mut txn = bookmarks.create_transaction(ctx.clone());
        txn.create(&name_1, ONES_CSID, BookmarkUpdateReason::TestMove, None)
            .unwrap();
        assert!(!txn.commit().await.unwrap());
    })
}

#[fbinit::test]
fn test_create_change_same_bookmark(fb: FacebookInit) {
    async_unit::tokio_unit_test(async move {
        let ctx = CoreContext::test_mock(fb);
        let bookmarks = SqlBookmarksBuilder::with_sqlite_in_memory()
            .unwrap()
            .with_repo_id(REPO_ZERO);
        let name_1 = create_bookmark_name("book");

        let mut txn = bookmarks.create_transaction(ctx.clone());
        txn.create(&name_1, ONES_CSID, BookmarkUpdateReason::TestMove, None)
            .unwrap();
        assert!(txn
            .force_set(&name_1, ONES_CSID, BookmarkUpdateReason::TestMove, None)
            .is_err());

        let mut txn = bookmarks.create_transaction(ctx.clone());
        txn.force_set(&name_1, ONES_CSID, BookmarkUpdateReason::TestMove, None)
            .unwrap();
        assert!(txn
            .create(&name_1, ONES_CSID, BookmarkUpdateReason::TestMove, None)
            .is_err());

        let mut txn = bookmarks.create_transaction(ctx.clone());
        txn.force_set(&name_1, ONES_CSID, BookmarkUpdateReason::TestMove, None)
            .unwrap();
        assert!(txn
            .update(
                &name_1,
                TWOS_CSID,
                ONES_CSID,
                BookmarkUpdateReason::TestMove,
                None,
            )
            .is_err());

        let mut txn = bookmarks.create_transaction(ctx.clone());
        txn.update(
            &name_1,
            TWOS_CSID,
            ONES_CSID,
            BookmarkUpdateReason::TestMove,
            None,
        )
        .unwrap();
        assert!(txn
            .force_set(&name_1, ONES_CSID, BookmarkUpdateReason::TestMove, None)
            .is_err());

        let mut txn = bookmarks.create_transaction(ctx.clone());
        txn.update(
            &name_1,
            TWOS_CSID,
            ONES_CSID,
            BookmarkUpdateReason::TestMove,
            None,
        )
        .unwrap();
        assert!(txn
            .force_delete(&name_1, BookmarkUpdateReason::TestMove, None)
            .is_err());

        let mut txn = bookmarks.create_transaction(ctx.clone());
        txn.force_delete(&name_1, BookmarkUpdateReason::TestMove, None)
            .unwrap();
        assert!(txn
            .update(
                &name_1,
                TWOS_CSID,
                ONES_CSID,
                BookmarkUpdateReason::TestMove,
                None
            )
            .is_err());

        let mut txn = bookmarks.create_transaction(ctx.clone());
        txn.delete(&name_1, ONES_CSID, BookmarkUpdateReason::TestMove, None)
            .unwrap();
        assert!(txn
            .update(
                &name_1,
                TWOS_CSID,
                ONES_CSID,
                BookmarkUpdateReason::TestMove,
                None,
            )
            .is_err());

        let mut txn = bookmarks.create_transaction(ctx.clone());
        txn.update(
            &name_1,
            TWOS_CSID,
            ONES_CSID,
            BookmarkUpdateReason::TestMove,
            None,
        )
        .unwrap();
        assert!(txn
            .delete(&name_1, ONES_CSID, BookmarkUpdateReason::TestMove, None)
            .is_err());
    })
}

#[fbinit::test]
fn test_simple_update_bookmark(fb: FacebookInit) {
    async_unit::tokio_unit_test(async move {
        let ctx = CoreContext::test_mock(fb);
        let bookmarks = SqlBookmarksBuilder::with_sqlite_in_memory()
            .unwrap()
            .with_repo_id(REPO_ZERO);
        let name_1 = create_bookmark_name("book");

        let mut txn = bookmarks.create_transaction(ctx.clone());
        txn.create(&name_1, ONES_CSID, BookmarkUpdateReason::TestMove, None)
            .unwrap();
        assert!(txn.commit().await.unwrap());

        let mut txn = bookmarks.create_transaction(ctx.clone());
        txn.update(
            &name_1,
            TWOS_CSID,
            ONES_CSID,
            BookmarkUpdateReason::TestMove,
            None,
        )
        .unwrap();
        assert!(txn.commit().await.unwrap());

        assert_eq!(
            bookmarks.get(ctx.clone(), &name_1).await.unwrap(),
            Some(TWOS_CSID)
        );

        compare_log_entries(
            bookmarks
                .read_next_bookmark_log_entries(ctx.clone(), 1, 1, Freshness::MostRecent)
                .try_collect::<Vec<_>>()
                .await
                .unwrap(),
            vec![BookmarkUpdateLogEntry {
                id: 2,
                repo_id: REPO_ZERO,
                bookmark_name: name_1,
                to_changeset_id: Some(TWOS_CSID),
                from_changeset_id: Some(ONES_CSID),
                reason: BookmarkUpdateReason::TestMove,
                timestamp: Timestamp::now(),
                bundle_replay_data: None,
            }],
        );
    })
}

#[fbinit::test]
fn test_noop_update(fb: FacebookInit) {
    async_unit::tokio_unit_test(async move {
        let ctx = CoreContext::test_mock(fb);
        let bookmarks = SqlBookmarksBuilder::with_sqlite_in_memory()
            .unwrap()
            .with_repo_id(REPO_ZERO);
        let name_1 = create_bookmark_name("book");

        let mut txn = bookmarks.create_transaction(ctx.clone());
        txn.create(&name_1, ONES_CSID, BookmarkUpdateReason::TestMove, None)
            .unwrap();
        assert!(txn.commit().await.unwrap());

        let mut txn = bookmarks.create_transaction(ctx.clone());
        txn.update(
            &name_1,
            ONES_CSID,
            ONES_CSID,
            BookmarkUpdateReason::TestMove,
            None,
        )
        .unwrap();
        assert!(txn.commit().await.unwrap());

        assert_eq!(
            bookmarks.get(ctx.clone(), &name_1).await.unwrap(),
            Some(ONES_CSID)
        );
    })
}

#[fbinit::test]
fn test_scratch_update_bookmark(fb: FacebookInit) {
    async_unit::tokio_unit_test(async move {
        let ctx = CoreContext::test_mock(fb);
        let bookmarks = SqlBookmarksBuilder::with_sqlite_in_memory()
            .unwrap()
            .with_repo_id(REPO_ZERO);
        let name_1 = create_bookmark_name("book");

        let mut txn = bookmarks.create_transaction(ctx.clone());
        txn.create_scratch(&name_1, ONES_CSID).unwrap();
        assert!(txn.commit().await.unwrap());

        let mut txn = bookmarks.create_transaction(ctx.clone());
        txn.update_scratch(&name_1, TWOS_CSID, ONES_CSID).unwrap();
        assert!(txn.commit().await.unwrap());

        assert_eq!(
            bookmarks.get(ctx.clone(), &name_1).await.unwrap(),
            Some(TWOS_CSID)
        );

        compare_log_entries(
            bookmarks
                .read_next_bookmark_log_entries(ctx.clone(), 1, 1, Freshness::MostRecent)
                .try_collect::<Vec<_>>()
                .await
                .unwrap(),
            vec![],
        );
    })
}

#[fbinit::test]
fn test_update_non_existent_bookmark(fb: FacebookInit) {
    async_unit::tokio_unit_test(async move {
        let ctx = CoreContext::test_mock(fb);
        let bookmarks = SqlBookmarksBuilder::with_sqlite_in_memory()
            .unwrap()
            .with_repo_id(REPO_ZERO);
        let name_1 = create_bookmark_name("book");

        let mut txn = bookmarks.create_transaction(ctx.clone());
        txn.update(
            &name_1,
            TWOS_CSID,
            ONES_CSID,
            BookmarkUpdateReason::TestMove,
            None,
        )
        .unwrap();
        assert_eq!(txn.commit().await.unwrap(), false);
    })
}

#[fbinit::test]
fn test_update_existing_bookmark_with_incorrect_commit(fb: FacebookInit) {
    async_unit::tokio_unit_test(async move {
        let ctx = CoreContext::test_mock(fb);
        let bookmarks = SqlBookmarksBuilder::with_sqlite_in_memory()
            .unwrap()
            .with_repo_id(REPO_ZERO);
        let name_1 = create_bookmark_name("book");

        let mut txn = bookmarks.create_transaction(ctx.clone());
        txn.create(&name_1, ONES_CSID, BookmarkUpdateReason::TestMove, None)
            .unwrap();
        assert!(txn.commit().await.unwrap());

        let mut txn = bookmarks.create_transaction(ctx.clone());
        txn.update(
            &name_1,
            ONES_CSID,
            TWOS_CSID,
            BookmarkUpdateReason::TestMove,
            None,
        )
        .unwrap();
        assert_eq!(txn.commit().await.unwrap(), false);
    })
}

#[fbinit::test]
fn test_force_delete(fb: FacebookInit) {
    async_unit::tokio_unit_test(async move {
        let ctx = CoreContext::test_mock(fb);
        let bookmarks = SqlBookmarksBuilder::with_sqlite_in_memory()
            .unwrap()
            .with_repo_id(REPO_ZERO);
        let name_1 = create_bookmark_name("book");

        let mut txn = bookmarks.create_transaction(ctx.clone());
        txn.force_delete(&name_1, BookmarkUpdateReason::TestMove, None)
            .unwrap();
        assert!(txn.commit().await.unwrap());

        assert_eq!(bookmarks.get(ctx.clone(), &name_1).await.unwrap(), None);

        let mut txn = bookmarks.create_transaction(ctx.clone());
        txn.create(&name_1, ONES_CSID, BookmarkUpdateReason::TestMove, None)
            .unwrap();
        assert!(txn.commit().await.unwrap());
        assert!(bookmarks.get(ctx.clone(), &name_1).await.unwrap().is_some());

        let mut txn = bookmarks.create_transaction(ctx.clone());
        txn.force_delete(&name_1, BookmarkUpdateReason::TestMove, None)
            .unwrap();
        assert!(txn.commit().await.unwrap());

        assert_eq!(bookmarks.get(ctx.clone(), &name_1).await.unwrap(), None);

        compare_log_entries(
            bookmarks
                .read_next_bookmark_log_entries(ctx.clone(), 2, 1, Freshness::MostRecent)
                .try_collect::<Vec<_>>()
                .await
                .unwrap(),
            vec![BookmarkUpdateLogEntry {
                id: 3,
                repo_id: REPO_ZERO,
                bookmark_name: name_1,
                to_changeset_id: None,
                from_changeset_id: None,
                reason: BookmarkUpdateReason::TestMove,
                timestamp: Timestamp::now(),
                bundle_replay_data: None,
            }],
        );
    })
}

#[fbinit::test]
fn test_delete(fb: FacebookInit) {
    async_unit::tokio_unit_test(async move {
        let ctx = CoreContext::test_mock(fb);
        let bookmarks = SqlBookmarksBuilder::with_sqlite_in_memory()
            .unwrap()
            .with_repo_id(REPO_ZERO);
        let name_1 = create_bookmark_name("book");

        let mut txn = bookmarks.create_transaction(ctx.clone());
        txn.delete(&name_1, ONES_CSID, BookmarkUpdateReason::TestMove, None)
            .unwrap();
        assert_eq!(txn.commit().await.unwrap(), false);

        let mut txn = bookmarks.create_transaction(ctx.clone());
        txn.create(&name_1, ONES_CSID, BookmarkUpdateReason::TestMove, None)
            .unwrap();
        assert!(txn.commit().await.unwrap());
        assert!(bookmarks.get(ctx.clone(), &name_1).await.unwrap().is_some());

        let mut txn = bookmarks.create_transaction(ctx.clone());
        txn.delete(&name_1, ONES_CSID, BookmarkUpdateReason::TestMove, None)
            .unwrap();
        assert!(txn.commit().await.unwrap());

        compare_log_entries(
            bookmarks
                .read_next_bookmark_log_entries(ctx.clone(), 1, 1, Freshness::MostRecent)
                .try_collect::<Vec<_>>()
                .await
                .unwrap(),
            vec![BookmarkUpdateLogEntry {
                id: 2,
                repo_id: REPO_ZERO,
                bookmark_name: name_1,
                to_changeset_id: None,
                from_changeset_id: Some(ONES_CSID),
                reason: BookmarkUpdateReason::TestMove,
                timestamp: Timestamp::now(),
                bundle_replay_data: None,
            }],
        );
    })
}

#[fbinit::test]
fn test_delete_incorrect_hash(fb: FacebookInit) {
    async_unit::tokio_unit_test(async move {
        let ctx = CoreContext::test_mock(fb);
        let bookmarks = SqlBookmarksBuilder::with_sqlite_in_memory()
            .unwrap()
            .with_repo_id(REPO_ZERO);
        let name_1 = create_bookmark_name("book");

        let mut txn = bookmarks.create_transaction(ctx.clone());
        txn.create(&name_1, ONES_CSID, BookmarkUpdateReason::TestMove, None)
            .unwrap();
        assert!(txn.commit().await.unwrap());
        assert!(bookmarks.get(ctx.clone(), &name_1).await.unwrap().is_some());

        let mut txn = bookmarks.create_transaction(ctx.clone());
        txn.delete(&name_1, TWOS_CSID, BookmarkUpdateReason::TestMove, None)
            .unwrap();
        assert_eq!(txn.commit().await.unwrap(), false);
    })
}

#[fbinit::test]
fn test_list_by_prefix(fb: FacebookInit) {
    async_unit::tokio_unit_test(async move {
        let ctx = CoreContext::test_mock(fb);
        let bookmarks = SqlBookmarksBuilder::with_sqlite_in_memory()
            .unwrap()
            .with_repo_id(REPO_ZERO);
        let name_1 = create_bookmark_name("book1");
        let name_2 = create_bookmark_name("book2");

        let mut txn = bookmarks.create_transaction(ctx.clone());
        txn.create(&name_1, ONES_CSID, BookmarkUpdateReason::TestMove, None)
            .unwrap();
        txn.create(&name_2, TWOS_CSID, BookmarkUpdateReason::TestMove, None)
            .unwrap();
        assert!(txn.commit().await.unwrap());

        let prefix = create_prefix("book");
        let name_1_prefix = create_prefix("book1");
        let name_2_prefix = create_prefix("book2");

        assert_eq!(
            bookmarks
                .list(
                    ctx.clone(),
                    Freshness::MostRecent,
                    &prefix,
                    BookmarkKind::ALL,
                    &BookmarkPagination::FromStart,
                    std::u64::MAX
                )
                .try_collect::<HashMap<_, _>>()
                .await
                .unwrap(),
            hashmap! {
                Bookmark::new(name_1.clone(), BookmarkKind::PullDefaultPublishing) => ONES_CSID,
                Bookmark::new(name_2.clone(), BookmarkKind::PullDefaultPublishing) => TWOS_CSID
            }
        );

        assert_eq!(
            bookmarks
                .list(
                    ctx.clone(),
                    Freshness::MostRecent,
                    &name_1_prefix,
                    BookmarkKind::ALL,
                    &BookmarkPagination::FromStart,
                    std::u64::MAX
                )
                .try_collect::<Vec<_>>()
                .await
                .unwrap(),
            vec![(
                Bookmark::new(name_1.clone(), BookmarkKind::PullDefaultPublishing),
                ONES_CSID
            )]
        );

        assert_eq!(
            bookmarks
                .list(
                    ctx.clone(),
                    Freshness::MostRecent,
                    &name_2_prefix,
                    BookmarkKind::ALL,
                    &BookmarkPagination::FromStart,
                    std::u64::MAX
                )
                .try_collect::<Vec<_>>()
                .await
                .unwrap(),
            vec![(
                Bookmark::new(name_2.clone(), BookmarkKind::PullDefaultPublishing),
                TWOS_CSID
            )]
        );
    })
}

#[fbinit::test]
fn test_create_different_repos(fb: FacebookInit) {
    async_unit::tokio_unit_test(async move {
        let ctx = CoreContext::test_mock(fb);
        let builder = SqlBookmarksBuilder::with_sqlite_in_memory().unwrap();
        let bookmarks_0 = builder.clone().with_repo_id(REPO_ZERO);
        let bookmarks_1 = builder.with_repo_id(REPO_ONE);
        let name_1 = create_bookmark_name("book");

        let mut txn = bookmarks_0.create_transaction(ctx.clone());
        txn.force_set(&name_1, ONES_CSID, BookmarkUpdateReason::TestMove, None)
            .unwrap();
        assert!(txn.commit().await.is_ok());

        // Updating value from another repo, should fail
        let mut txn = bookmarks_1.create_transaction(ctx.clone());
        txn.update(
            &name_1,
            TWOS_CSID,
            ONES_CSID,
            BookmarkUpdateReason::TestMove,
            None,
        )
        .unwrap();
        assert_eq!(txn.commit().await.unwrap(), false);

        // Creating value should succeed
        let mut txn = bookmarks_1.create_transaction(ctx.clone());
        txn.create(&name_1, TWOS_CSID, BookmarkUpdateReason::TestMove, None)
            .unwrap();
        assert!(txn.commit().await.is_ok());

        assert_eq!(
            bookmarks_0.get(ctx.clone(), &name_1).await.unwrap(),
            Some(ONES_CSID)
        );
        assert_eq!(
            bookmarks_1.get(ctx.clone(), &name_1).await.unwrap(),
            Some(TWOS_CSID)
        );

        // Force deleting should delete only from one repo
        let mut txn = bookmarks_1.create_transaction(ctx.clone());
        txn.force_delete(&name_1, BookmarkUpdateReason::TestMove, None)
            .unwrap();
        assert!(txn.commit().await.is_ok());
        assert_eq!(
            bookmarks_0.get(ctx.clone(), &name_1).await.unwrap(),
            Some(ONES_CSID)
        );

        // delete should fail for another repo
        let mut txn = bookmarks_1.create_transaction(ctx.clone());
        txn.delete(&name_1, ONES_CSID, BookmarkUpdateReason::TestMove, None)
            .unwrap();
        assert_eq!(txn.commit().await.unwrap(), false);
    })
}

async fn fetch_single(
    fb: FacebookInit,
    bookmarks: &dyn BookmarkUpdateLog,
    id: u64,
) -> BookmarkUpdateLogEntry {
    let ctx = CoreContext::test_mock(fb);
    bookmarks
        .read_next_bookmark_log_entries(ctx, id, 1, Freshness::MostRecent)
        .try_collect::<Vec<_>>()
        .await
        .unwrap()
        .get(0)
        .unwrap()
        .clone()
}

#[fbinit::test]
fn test_log_correct_order(fb: FacebookInit) {
    async_unit::tokio_unit_test(async move {
        let ctx = CoreContext::test_mock(fb);
        let bookmarks = SqlBookmarksBuilder::with_sqlite_in_memory()
            .unwrap()
            .with_repo_id(REPO_ZERO);
        let name_1 = create_bookmark_name("book");
        let name_2 = create_bookmark_name("book2");

        let mut txn = bookmarks.create_transaction(ctx.clone());
        txn.force_set(&name_1, ONES_CSID, BookmarkUpdateReason::TestMove, None)
            .unwrap();
        assert!(txn.commit().await.is_ok());

        let mut txn = bookmarks.create_transaction(ctx.clone());
        txn.update(
            &name_1,
            TWOS_CSID,
            ONES_CSID,
            BookmarkUpdateReason::TestMove,
            None,
        )
        .unwrap();
        txn.commit().await.unwrap();

        let mut txn = bookmarks.create_transaction(ctx.clone());
        txn.update(
            &name_1,
            THREES_CSID,
            TWOS_CSID,
            BookmarkUpdateReason::TestMove,
            None,
        )
        .unwrap();
        txn.commit().await.unwrap();

        let mut txn = bookmarks.create_transaction(ctx.clone());
        txn.update(
            &name_1,
            FOURS_CSID,
            THREES_CSID,
            BookmarkUpdateReason::TestMove,
            None,
        )
        .unwrap();
        txn.commit().await.unwrap();

        let mut txn = bookmarks.create_transaction(ctx.clone());
        txn.force_set(&name_2, ONES_CSID, BookmarkUpdateReason::TestMove, None)
            .unwrap();
        assert!(txn.commit().await.is_ok());

        let mut txn = bookmarks.create_transaction(ctx.clone());
        txn.update(
            &name_1,
            FIVES_CSID,
            FOURS_CSID,
            BookmarkUpdateReason::TestMove,
            None,
        )
        .unwrap();
        txn.commit().await.unwrap();

        let mut txn = bookmarks.create_transaction(ctx.clone());
        txn.update(
            &name_1,
            SIXES_CSID,
            FIVES_CSID,
            BookmarkUpdateReason::Pushrebase,
            None,
        )
        .unwrap();
        txn.commit().await.unwrap();

        let log_entry = fetch_single(fb, &bookmarks, 0).await;
        assert_eq!(log_entry.to_changeset_id.unwrap(), ONES_CSID);

        let log_entry = fetch_single(fb, &bookmarks, 1).await;
        assert_eq!(log_entry.to_changeset_id.unwrap(), TWOS_CSID);

        let log_entry = fetch_single(fb, &bookmarks, 2).await;
        assert_eq!(log_entry.to_changeset_id.unwrap(), THREES_CSID);

        let log_entry = fetch_single(fb, &bookmarks, 3).await;
        assert_eq!(log_entry.to_changeset_id.unwrap(), FOURS_CSID);

        let log_entry = fetch_single(fb, &bookmarks, 5).await;
        assert_eq!(log_entry.to_changeset_id.unwrap(), FIVES_CSID);

        assert_eq!(
            bookmarks
                .read_next_bookmark_log_entries(ctx.clone(), 0, 4, Freshness::MostRecent)
                .try_collect::<Vec<_>>()
                .await
                .unwrap()
                .len(),
            4
        );

        assert_eq!(
            bookmarks
                .read_next_bookmark_log_entries(ctx.clone(), 0, 8, Freshness::MostRecent)
                .try_collect::<Vec<_>>()
                .await
                .unwrap()
                .len(),
            7
        );

        let entries = bookmarks
            .read_next_bookmark_log_entries(ctx.clone(), 0, 6, Freshness::MostRecent)
            .try_collect::<Vec<_>>()
            .await
            .unwrap();

        let cs_ids: Vec<_> = entries
            .into_iter()
            .map(|entry| entry.to_changeset_id.unwrap())
            .collect();
        assert_eq!(
            cs_ids,
            vec![
                ONES_CSID,
                TWOS_CSID,
                THREES_CSID,
                FOURS_CSID,
                ONES_CSID,
                FIVES_CSID
            ]
        );

        let entries = bookmarks
            .read_next_bookmark_log_entries_same_bookmark_and_reason(ctx.clone(), 0, 6)
            .try_collect::<Vec<_>>()
            .await
            .unwrap();

        // FOURS_CSID -> FIVES_CSID update is missing, because it has a different bookmark
        let cs_ids: Vec<_> = entries
            .into_iter()
            .map(|entry| entry.to_changeset_id.unwrap())
            .collect();
        assert_eq!(cs_ids, vec![ONES_CSID, TWOS_CSID, THREES_CSID, FOURS_CSID]);

        let entries = bookmarks
            .read_next_bookmark_log_entries_same_bookmark_and_reason(ctx.clone(), 5, 6)
            .try_collect::<Vec<_>>()
            .await
            .unwrap();

        // FIVES_CSID -> SIXES_CSID update is missing, because it has a different reason
        let cs_ids: Vec<_> = entries
            .into_iter()
            .map(|entry| entry.to_changeset_id.unwrap())
            .collect();
        assert_eq!(cs_ids, vec![FIVES_CSID]);
    })
}

#[fbinit::test]
fn test_log_bundle_replay_data(fb: FacebookInit) {
    async_unit::tokio_unit_test(async move {
        let ctx = CoreContext::test_mock(fb);
        let bookmarks = SqlBookmarksBuilder::with_sqlite_in_memory()
            .unwrap()
            .with_repo_id(REPO_ZERO);
        let name_1 = create_bookmark_name("book");
        let expected = RawBundleReplayData {
            bundle_handle: "handle".to_string(),
            commit_timestamps_json: "json_data".to_string(),
        };

        let mut txn = bookmarks.create_transaction(ctx.clone());
        txn.force_set(
            &name_1,
            ONES_CSID,
            BookmarkUpdateReason::TestMove,
            Some(&expected),
        )
        .unwrap();
        assert!(txn.commit().await.is_ok());

        let log_entry = fetch_single(fb, &bookmarks, 0).await;
        let actual = log_entry.bundle_replay_data.unwrap();
        assert_eq!(actual, expected);
    })
}

#[fbinit::test]
fn test_read_log_entry_many_repos(fb: FacebookInit) {
    async_unit::tokio_unit_test(async move {
        let ctx = CoreContext::test_mock(fb);
        let builder = SqlBookmarksBuilder::with_sqlite_in_memory().unwrap();
        let bookmarks_0 = builder.clone().with_repo_id(REPO_ZERO);
        let bookmarks_1 = builder.clone().with_repo_id(REPO_ONE);
        let bookmarks_2 = builder.with_repo_id(REPO_TWO);
        let name_1 = create_bookmark_name("book");

        let mut txn = bookmarks_0.create_transaction(ctx.clone());
        txn.force_set(&name_1, ONES_CSID, BookmarkUpdateReason::TestMove, None)
            .unwrap();
        assert!(txn.commit().await.is_ok());

        let mut txn = bookmarks_1.create_transaction(ctx.clone());
        txn.force_set(&name_1, ONES_CSID, BookmarkUpdateReason::TestMove, None)
            .unwrap();
        assert!(txn.commit().await.is_ok());

        assert_eq!(
            bookmarks_0
                .read_next_bookmark_log_entries(ctx.clone(), 0, 1, Freshness::MostRecent)
                .try_collect::<Vec<_>>()
                .await
                .unwrap()
                .len(),
            1
        );

        assert_eq!(
            bookmarks_1
                .read_next_bookmark_log_entries(ctx.clone(), 0, 1, Freshness::MostRecent)
                .try_collect::<Vec<_>>()
                .await
                .unwrap()
                .len(),
            1
        );

        assert_eq!(
            bookmarks_0
                .read_next_bookmark_log_entries(ctx.clone(), 1, 1, Freshness::MostRecent)
                .try_collect::<Vec<_>>()
                .await
                .unwrap()
                .len(),
            0
        );

        assert_eq!(
            bookmarks_2
                .read_next_bookmark_log_entries(ctx.clone(), 0, 1, Freshness::MostRecent)
                .try_collect::<Vec<_>>()
                .await
                .unwrap()
                .len(),
            0
        );
    })
}

#[test]
fn test_update_reason_conversion() -> Result<(), Error> {
    use BookmarkUpdateReason::*;
    let unusedreason = TestMove;

    // If you are adding a new variant here, please also add a test
    // for the new bookmark reason.
    match unusedreason {
        Backsyncer => {}
        Blobimport => {}
        ManualMove => {}
        Push => {}
        Pushrebase => {}
        TestMove => {}
        XRepoSync => {}
    };

    let reasons = vec![
        Backsyncer, Blobimport, ManualMove, Push, Pushrebase, TestMove, XRepoSync,
    ];

    for reason in reasons {
        let value = Value::from(reason);
        BookmarkUpdateReason::new(value)?;
    }

    Ok(())
}

#[fbinit::test]
fn test_list_bookmark_log_entries(fb: FacebookInit) {
    async_unit::tokio_unit_test(async move {
        let ctx = CoreContext::test_mock(fb);
        let bookmarks = SqlBookmarksBuilder::with_sqlite_in_memory()
            .unwrap()
            .with_repo_id(REPO_ZERO);
        let name_1 = create_bookmark_name("book");

        let mut txn = bookmarks.create_transaction(ctx.clone());
        txn.force_set(&name_1, ONES_CSID, BookmarkUpdateReason::TestMove, None)
            .unwrap();
        assert!(txn.commit().await.is_ok());

        let mut txn = bookmarks.create_transaction(ctx.clone());
        txn.update(
            &name_1,
            TWOS_CSID,
            ONES_CSID,
            BookmarkUpdateReason::TestMove,
            None,
        )
        .unwrap();
        txn.commit().await.unwrap();

        let mut txn = bookmarks.create_transaction(ctx.clone());
        txn.update(
            &name_1,
            THREES_CSID,
            TWOS_CSID,
            BookmarkUpdateReason::TestMove,
            None,
        )
        .unwrap();
        txn.commit().await.unwrap();

        let mut txn = bookmarks.create_transaction(ctx.clone());
        txn.update(
            &name_1,
            FOURS_CSID,
            THREES_CSID,
            BookmarkUpdateReason::TestMove,
            None,
        )
        .unwrap();
        txn.commit().await.unwrap();

        let mut txn = bookmarks.create_transaction(ctx.clone());
        txn.update(
            &name_1,
            FIVES_CSID,
            FOURS_CSID,
            BookmarkUpdateReason::TestMove,
            None,
        )
        .unwrap();
        txn.commit().await.unwrap();

        assert_eq!(
            bookmarks
                .list_bookmark_log_entries(
                    ctx.clone(),
                    name_1.clone(),
                    3,
                    None,
                    Freshness::MostRecent
                )
                .map_ok(|(cs, rs, _ts)| (cs, rs)) // dropping timestamps
                .try_collect::<Vec<_>>()
                .await
                .unwrap(),
            vec![
                (Some(FIVES_CSID), BookmarkUpdateReason::TestMove),
                (Some(FOURS_CSID), BookmarkUpdateReason::TestMove),
                (Some(THREES_CSID), BookmarkUpdateReason::TestMove),
            ]
        );

        assert_eq!(
            bookmarks
                .list_bookmark_log_entries(ctx.clone(), name_1, 3, Some(1), Freshness::MostRecent)
                .map_ok(|(cs, rs, _ts)| (cs, rs)) // dropping timestamps
                .try_collect::<Vec<_>>()
                .await
                .unwrap(),
            vec![
                (Some(FOURS_CSID), BookmarkUpdateReason::TestMove),
                (Some(THREES_CSID), BookmarkUpdateReason::TestMove),
                (Some(TWOS_CSID), BookmarkUpdateReason::TestMove),
            ]
        );
    })
}
