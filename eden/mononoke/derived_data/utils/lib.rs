/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![type_length_limit = "4522397"]

use anyhow::{anyhow, format_err, Error};
use async_trait::async_trait;
use blame::{BlameRoot, BlameRootMapping};
use blobrepo::BlobRepo;
use blobrepo_override::DangerousOverride;
use blobstore::{Blobstore, Loadable};
use cacheblob::{dummy::DummyLease, LeaseOps, MemWritesBlobstore};
use changeset_info::{ChangesetInfo, ChangesetInfoMapping};
use cloned::cloned;
use context::CoreContext;
use deleted_files_manifest::{RootDeletedManifestId, RootDeletedManifestMapping};
use derived_data::{
    BonsaiDerived, BonsaiDerivedMapping, DeriveError, Mode as DeriveMode, RegenerateMapping,
};
use derived_data_filenodes::{FilenodesOnlyPublic, FilenodesOnlyPublicMapping};
use fastlog::{RootFastlog, RootFastlogMapping};
use fsnodes::{RootFsnodeId, RootFsnodeMapping};
use futures::{
    compat::Future01CompatExt,
    future::ready,
    stream::{self, futures_unordered::FuturesUnordered},
    Future, FutureExt, StreamExt, TryFutureExt, TryStreamExt,
};
use futures_ext::{BoxFuture, FutureExt as OldFutureExt};
use futures_old::{future, stream as stream_old, Future as OldFuture, Stream};
use mercurial_derived_data::{HgChangesetIdMapping, MappedHgChangesetId};
use mononoke_types::{BonsaiChangeset, ChangesetId};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use unodes::{RootUnodeManifestId, RootUnodeManifestMapping};

pub const POSSIBLE_DERIVED_TYPES: &[&str] = &[
    RootUnodeManifestId::NAME,
    RootFastlog::NAME,
    MappedHgChangesetId::NAME,
    RootFsnodeId::NAME,
    BlameRoot::NAME,
    ChangesetInfo::NAME,
    RootDeletedManifestId::NAME,
    FilenodesOnlyPublic::NAME,
];

pub fn derive_data_for_csids(
    ctx: &CoreContext,
    repo: &BlobRepo,
    csids: Vec<ChangesetId>,
    derived_data_types: &[String],
) -> Result<impl Future<Output = Result<(), Error>>, Error> {
    let derivations = FuturesUnordered::new();

    for data_type in derived_data_types {
        let derived_utils = derived_data_utils(repo.clone(), data_type)?;

        let mut futs = vec![];
        for csid in &csids {
            let fut = derived_utils
                .derive(ctx.clone(), repo.clone(), *csid)
                .map(|_| ())
                .compat();
            futs.push(fut);
        }

        derivations.push(async move {
            // Call functions sequentially because derived data is sequential
            // so there's no point in trying to derive it in parallel
            for f in futs {
                f.await?;
            }
            Result::<_, Error>::Ok(())
        });
    }

    Ok(async move {
        derivations.try_for_each(|_| ready(Ok(()))).await?;
        Ok(())
    })
}

#[async_trait]
pub trait DerivedUtils: Send + Sync + 'static {
    /// Derive data for changeset
    fn derive(
        &self,
        ctx: CoreContext,
        repo: BlobRepo,
        csid: ChangesetId,
    ) -> BoxFuture<String, Error>;

    fn backfill_batch_dangerous(
        &self,
        ctx: CoreContext,
        repo: BlobRepo,
        csids: Vec<ChangesetId>,
    ) -> BoxFuture<(), Error>;

    /// Find pending changeset (changesets for which data have not been derived)
    fn pending(
        &self,
        ctx: CoreContext,
        repo: BlobRepo,
        csids: Vec<ChangesetId>,
    ) -> BoxFuture<Vec<ChangesetId>, Error>;

    /// Regenerate derived data for specified set of commits
    fn regenerate(&self, csids: &Vec<ChangesetId>);

    /// Get a name for this type of derived data
    fn name(&self) -> &'static str;

    async fn find_oldest_underived<'a>(
        &'a self,
        ctx: &'a CoreContext,
        repo: &'a BlobRepo,
        csids: &'a Vec<ChangesetId>,
    ) -> Result<Option<BonsaiChangeset>, Error>;
}

#[derive(Clone)]
struct DerivedUtilsFromMapping<M> {
    mapping: RegenerateMapping<M>,
    mode: DeriveMode,
}

impl<M> DerivedUtilsFromMapping<M> {
    fn new(mapping: M, mode: DeriveMode) -> Self {
        let mapping = RegenerateMapping::new(mapping);
        Self { mapping, mode }
    }
}

#[async_trait]
impl<M> DerivedUtils for DerivedUtilsFromMapping<M>
where
    M: BonsaiDerivedMapping + Clone + 'static,
    M::Value: BonsaiDerived + std::fmt::Debug,
{
    fn derive(
        &self,
        ctx: CoreContext,
        repo: BlobRepo,
        csid: ChangesetId,
    ) -> BoxFuture<String, Error> {
        // We call batch_derive so that we can pass
        // `self.mapping` there. This will allow us to
        // e.g. regenerate derived data for the commit
        // even if it was already generated (see RegenerateMapping call).

        let mode = self.mode;
        let mapping = self.mapping.clone();
        async move {
            let res = M::Value::batch_derive(&ctx, &repo, vec![csid], &mapping, mode).await?;
            let val = res
                .get(&csid)
                .ok_or(anyhow!("internal derived data error"))?;
            Ok(format!("{:?}", val))
        }
        .boxed()
        .compat()
        .boxify()
    }

    /// !!!!This function is dangerous and should be used with care!!!!
    /// In particular it might corrupt the data if it tries to derive data that
    /// depends on another derived data (e.g. blame depends on unodes) and both
    /// of them are not derived.
    /// For example, if unodes and blame are both underived and we are trying
    /// to derive blame then unodes mapping might be inserted in the blobstore
    /// before all unodes were derived.
    ///
    /// This function should be safe to use only if derived data doesn't depend
    /// on another derived data (e.g. unodes) or if this dependency is already derived
    /// (e.g. deriving blame when unodes are already derived).
    fn backfill_batch_dangerous(
        &self,
        ctx: CoreContext,
        repo: BlobRepo,
        csids: Vec<ChangesetId>,
    ) -> BoxFuture<(), Error> {
        let orig_mapping = self.mapping.clone();
        // With InMemoryMapping we can ensure that mapping entries are written only after
        // all corresponding blobs were successfully saved
        let in_memory_mapping = InMemoryMapping::new(self.mapping.clone());

        // Use `MemWritesBlobstore` to avoid blocking on writes to underlying blobstore.
        // `::persist` is later used to bulk write all pending data.
        let mut memblobstore = None;
        let repo = repo
            .dangerous_override(|_| Arc::new(DummyLease {}) as Arc<dyn LeaseOps>)
            .dangerous_override(|blobstore| -> Arc<dyn Blobstore> {
                let blobstore = Arc::new(MemWritesBlobstore::new(blobstore));
                memblobstore = Some(blobstore.clone());
                blobstore
            });
        let memblobstore = memblobstore.expect("memblobstore should have been updated");

        {
            cloned!(ctx, repo, in_memory_mapping);
            async move {
                M::Value::batch_derive(&ctx, &repo, csids, &in_memory_mapping, DeriveMode::Unsafe)
                    .await
            }
        }
        .boxed()
        .compat()
        .from_err()
        .and_then({
            cloned!(ctx, memblobstore);
            move |_| memblobstore.persist(ctx)
        })
        .and_then(move |_| {
            let buffer = in_memory_mapping.into_buffer();
            let buffer = buffer.lock().unwrap();
            let mut futs = vec![];
            for (cs_id, value) in buffer.iter() {
                futs.push(orig_mapping.put(ctx.clone(), *cs_id, value.clone()));
            }
            stream_old::futures_unordered(futs).for_each(|_| Ok(()))
        })
        .boxify()
    }

    fn pending(
        &self,
        ctx: CoreContext,
        _repo: BlobRepo,
        mut csids: Vec<ChangesetId>,
    ) -> BoxFuture<Vec<ChangesetId>, Error> {
        self.mapping
            .get(ctx, csids.clone())
            .map(move |derived| {
                csids.retain(|csid| !derived.contains_key(&csid));
                csids
            })
            .boxify()
    }

    async fn find_oldest_underived<'a>(
        &'a self,
        ctx: &'a CoreContext,
        repo: &'a BlobRepo,
        csids: &'a Vec<ChangesetId>,
    ) -> Result<Option<BonsaiChangeset>, Error> {
        let mut underived_ancestors = vec![];
        for cs_id in csids {
            underived_ancestors.push(M::Value::find_all_underived_ancestors(&ctx, &repo, cs_id));
        }

        let boxed_stream = stream::iter(underived_ancestors)
            .map(Result::<_, DeriveError>::Ok)
            .try_buffer_unordered(100)
            // boxed() is necessary to avoid "one type is more general than the other" error
            .boxed();

        let res = boxed_stream.try_collect::<Vec<_>>().await?;
        let oldest_changesets = stream::iter(
            res.into_iter()
                // The first element is the first underived ancestor in toposorted order.
                // Let's use it as a proxy for the oldest underived commit
                .map(|all_underived| all_underived.get(0).cloned())
                .flatten()
                .map(|cs_id| async move { cs_id.load(ctx.clone(), repo.blobstore()).await }),
        )
        .map(Ok)
        .try_buffer_unordered(100)
        // boxed() is necessary to avoid "one type is more general than the other" error
        .boxed();

        let oldest_changesets = oldest_changesets.try_collect::<Vec<_>>().await?;
        Ok(oldest_changesets
            .into_iter()
            .min_by_key(|bcs| *bcs.author_date()))
    }

    fn regenerate(&self, csids: &Vec<ChangesetId>) {
        self.mapping.regenerate(csids.iter().copied())
    }

    fn name(&self) -> &'static str {
        M::Value::NAME
    }
}

#[derive(Clone)]
struct InMemoryMapping<M: BonsaiDerivedMapping + Clone> {
    mapping: M,
    buffer: Arc<Mutex<HashMap<ChangesetId, M::Value>>>,
}

impl<M> InMemoryMapping<M>
where
    M: BonsaiDerivedMapping + Clone,
    <M as BonsaiDerivedMapping>::Value: Clone,
{
    fn new(mapping: M) -> Self {
        Self {
            mapping,
            buffer: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn into_buffer(self) -> Arc<Mutex<HashMap<ChangesetId, M::Value>>> {
        self.buffer
    }
}

impl<M> BonsaiDerivedMapping for InMemoryMapping<M>
where
    M: BonsaiDerivedMapping + Clone,
    <M as BonsaiDerivedMapping>::Value: Clone,
{
    type Value = M::Value;

    fn get(
        &self,
        ctx: CoreContext,
        mut csids: Vec<ChangesetId>,
    ) -> BoxFuture<HashMap<ChangesetId, Self::Value>, Error> {
        let buffer = self.buffer.lock().unwrap();
        let mut ans = HashMap::new();
        csids.retain(|cs_id| {
            if let Some(v) = buffer.get(cs_id) {
                ans.insert(*cs_id, v.clone());
                false
            } else {
                true
            }
        });

        self.mapping
            .get(ctx, csids)
            .map(move |fetched| ans.into_iter().chain(fetched.into_iter()).collect())
            .boxify()
    }

    fn put(&self, _ctx: CoreContext, csid: ChangesetId, id: Self::Value) -> BoxFuture<(), Error> {
        let mut buffer = self.buffer.lock().unwrap();
        buffer.insert(csid, id);
        future::ok(()).boxify()
    }
}

pub fn derived_data_utils(
    repo: BlobRepo,
    name: impl AsRef<str>,
) -> Result<Arc<dyn DerivedUtils>, Error> {
    derived_data_utils_impl(repo, name, DeriveMode::OnlyIfEnabled)
}

pub fn derived_data_utils_unsafe(
    repo: BlobRepo,
    name: impl AsRef<str>,
) -> Result<Arc<dyn DerivedUtils>, Error> {
    derived_data_utils_impl(repo, name, DeriveMode::Unsafe)
}

fn derived_data_utils_impl(
    repo: BlobRepo,
    name: impl AsRef<str>,
    mode: DeriveMode,
) -> Result<Arc<dyn DerivedUtils>, Error> {
    match name.as_ref() {
        RootUnodeManifestId::NAME => {
            let mapping = RootUnodeManifestMapping::new(
                repo.get_blobstore(),
                repo.get_derived_data_config().unode_version,
            );
            Ok(Arc::new(DerivedUtilsFromMapping::new(mapping, mode)))
        }
        RootFastlog::NAME => {
            let mapping = RootFastlogMapping::new(repo.get_blobstore().boxed());
            Ok(Arc::new(DerivedUtilsFromMapping::new(mapping, mode)))
        }
        MappedHgChangesetId::NAME => {
            let mapping = HgChangesetIdMapping::new(&repo);
            Ok(Arc::new(DerivedUtilsFromMapping::new(mapping, mode)))
        }
        RootFsnodeId::NAME => {
            let mapping = RootFsnodeMapping::new(repo.get_blobstore());
            Ok(Arc::new(DerivedUtilsFromMapping::new(mapping, mode)))
        }
        BlameRoot::NAME => {
            let mapping = BlameRootMapping::new(repo.get_blobstore().boxed());
            Ok(Arc::new(DerivedUtilsFromMapping::new(mapping, mode)))
        }
        ChangesetInfo::NAME => {
            let mapping = ChangesetInfoMapping::new(repo.get_blobstore().boxed());
            Ok(Arc::new(DerivedUtilsFromMapping::new(mapping, mode)))
        }
        RootDeletedManifestId::NAME => {
            let mapping = RootDeletedManifestMapping::new(repo.get_blobstore());
            Ok(Arc::new(DerivedUtilsFromMapping::new(mapping, mode)))
        }
        FilenodesOnlyPublic::NAME => {
            let mapping = FilenodesOnlyPublicMapping::new(repo);
            Ok(Arc::new(DerivedUtilsFromMapping::new(mapping, mode)))
        }
        name => Err(format_err!("Unsupported derived data type: {}", name)),
    }
}
