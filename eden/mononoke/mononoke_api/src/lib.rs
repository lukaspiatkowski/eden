/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(backtrace)]
#![feature(bool_to_option)]
#![deny(warnings)]

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Error;
use blobrepo_factory::{BlobstoreOptions, Caching, ReadOnlyStorage};
use cloned::cloned;
use fbinit::FacebookInit;
use futures::future;
use futures::future::try_join_all;
use slog::{debug, info, o, Logger};
use sql_ext::facebook::MysqlOptions;

use metaconfig_parser::RepoConfigs;

use crate::repo::Repo;

pub mod changeset;
pub mod changeset_path;
pub mod changeset_path_diff;
pub mod errors;
pub mod file;
pub mod hg;
pub mod path;
pub mod repo;
pub mod repo_write;
pub mod specifiers;
pub mod tree;

#[cfg(test)]
mod test;

pub use crate::changeset::{ChangesetContext, Generation};
pub use crate::changeset_path::{
    unified_diff, ChangesetPathContext, CopyInfo, PathEntry, UnifiedDiff, UnifiedDiffMode,
};
pub use crate::changeset_path_diff::ChangesetPathDiffContext;
pub use crate::errors::MononokeError;
pub use crate::file::{FileContext, FileId, FileMetadata, FileType};
pub use crate::path::MononokePath;
pub use crate::repo::RepoContext;
pub use crate::repo_write::create_changeset::{CreateChange, CreateCopyInfo};
pub use crate::repo_write::RepoWriteContext;
pub use crate::specifiers::{
    ChangesetId, ChangesetIdPrefix, ChangesetPrefixSpecifier, ChangesetSpecifier,
    ChangesetSpecifierPrefixResolution, Globalrev, HgChangesetId, HgChangesetIdPrefix,
};
pub use crate::tree::{TreeContext, TreeEntry, TreeId, TreeSummary};

// Re-export types that are useful for clients.
pub use context::{CoreContext, LoggingContainer, SessionContainer};

/// An instance of Mononoke, which may manage multiple repositories.
pub struct Mononoke {
    repos: HashMap<String, Arc<Repo>>,
}

impl Mononoke {
    /// Create a Mononoke instance.
    pub async fn new(
        fb: FacebookInit,
        logger: Logger,
        configs: RepoConfigs,
        mysql_options: MysqlOptions,
        with_cachelib: Caching,
        readonly_storage: ReadOnlyStorage,
        blobstore_options: BlobstoreOptions,
    ) -> Result<Self, Error> {
        let common_config = configs.common;
        let repos = future::join_all(
            configs
                .repos
                .into_iter()
                .filter(move |&(_, ref config)| config.enabled)
                .map(move |(name, config)| {
                    cloned!(logger, common_config, blobstore_options);
                    async move {
                        info!(logger, "Initializing repo: {}", &name);
                        let repo = Repo::new(
                            fb,
                            logger.new(o!("repo" => name.clone())),
                            name.clone(),
                            config,
                            common_config,
                            mysql_options,
                            with_cachelib,
                            readonly_storage,
                            blobstore_options,
                        )
                        .await
                        .expect("failed to initialize repo");
                        debug!(logger, "Initialized {}", &name);
                        (name, Arc::new(repo))
                    }
                }),
        )
        .await
        .into_iter()
        .collect();
        Ok(Self { repos })
    }

    /// Start a request on a repository.
    pub async fn repo(
        &self,
        ctx: CoreContext,
        name: impl AsRef<str>,
    ) -> Result<Option<RepoContext>, MononokeError> {
        match self.repos.get(name.as_ref()) {
            None => Ok(None),
            Some(repo) => Ok(Some(RepoContext::new(ctx, repo.clone()).await?)),
        }
    }

    /// Returns an `Iterator` over all repo names.
    pub fn repo_names(&self) -> impl Iterator<Item = &str> {
        self.repos.keys().map(AsRef::as_ref)
    }

    /// Report configured monitoring stats
    pub async fn report_monitoring_stats(&self, ctx: &CoreContext) -> Result<(), MononokeError> {
        let reporting_futs: Vec<_> = self
            .repos
            .iter()
            .map(|(_, repo)| async move { repo.report_monitoring_stats(ctx).await })
            .collect();
        try_join_all(reporting_futs).await.map(|_| ())
    }
}

#[cfg(test)]
mod test_impl {
    use super::*;
    use blobrepo::BlobRepo;
    use metaconfig_types::CommitSyncConfig;
    use synced_commit_mapping::SyncedCommitMapping;

    impl Mononoke {
        /// Create a Mononoke instance for testing.
        pub(crate) async fn new_test(
            ctx: CoreContext,
            repos: impl IntoIterator<Item = (String, BlobRepo)>,
        ) -> Result<Self, Error> {
            use futures::stream::{FuturesOrdered, TryStreamExt};
            let repos = repos
                .into_iter()
                .map(move |(name, repo)| {
                    cloned!(ctx);
                    async move {
                        Repo::new_test(ctx.clone(), repo)
                            .await
                            .map(move |repo| (name, Arc::new(repo)))
                    }
                })
                .collect::<FuturesOrdered<_>>()
                .try_collect()
                .await?;

            Ok(Self { repos })
        }

        pub(crate) async fn new_test_xrepo(
            ctx: CoreContext,
            repos: impl IntoIterator<
                Item = (
                    String,
                    BlobRepo,
                    CommitSyncConfig,
                    Arc<dyn SyncedCommitMapping>,
                ),
            >,
        ) -> Result<Self, Error> {
            use futures::stream::{FuturesOrdered, TryStreamExt};
            let repos = repos
                .into_iter()
                .map(
                    move |(name, repo, commit_sync_config, synced_commit_maping)| {
                        cloned!(ctx);
                        async move {
                            Repo::new_test_xrepo(
                                ctx.clone(),
                                repo,
                                commit_sync_config,
                                synced_commit_maping,
                            )
                            .await
                            .map(move |repo| (name, Arc::new(repo)))
                        }
                    },
                )
                .collect::<FuturesOrdered<_>>()
                .try_collect()
                .await?;

            Ok(Self { repos })
        }
    }
}
