/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{Context, Error};
use futures::{stream, Stream, StreamExt};
use gotham::state::{FromState, State};
use gotham_derive::{StateData, StaticResponseExtender};
use serde::Deserialize;

use edenapi_types::{DataEntry, DataRequest};
use gotham_ext::{error::HttpError, response::TryIntoResponse};
use mercurial_types::{HgFileNodeId, HgManifestId, HgNodeHash};
use mononoke_api::hg::{HgDataContext, HgDataId, HgRepoContext};
use revisionstore_types::Metadata;
use types::Key;

use crate::context::ServerContext;
use crate::errors::ErrorKind;
use crate::middleware::RequestContext;
use crate::utils::{cbor_stream, get_repo, parse_cbor_request};

/// XXX: This number was chosen arbitrarily.
const MAX_CONCURRENT_FETCHES_PER_REQUEST: usize = 10;

#[derive(Debug, Deserialize, StateData, StaticResponseExtender)]
pub struct DataParams {
    repo: String,
}

/// Fetch the content of the files requested by the client.
pub async fn files(state: &mut State) -> Result<impl TryIntoResponse, HttpError> {
    data::<HgFileNodeId>(state).await
}

/// Fetch the tree nodes requested by the client.
pub async fn trees(state: &mut State) -> Result<impl TryIntoResponse, HttpError> {
    data::<HgManifestId>(state).await
}

/// Generic async function to fetch any kind of data blob
/// whose identifier implements the `HgDataID` trait.
async fn data<ID: HgDataId>(state: &mut State) -> Result<impl TryIntoResponse, HttpError> {
    let rctx = RequestContext::borrow_from(state);
    let sctx = ServerContext::borrow_from(state);
    let params = DataParams::borrow_from(state);

    let repo = get_repo(&sctx, &rctx, &params.repo).await?;
    let request = parse_cbor_request(state).await?;

    Ok(cbor_stream(fetch_all::<ID>(repo, request)))
}

/// Fetch data for all of the requested keys concurrently.
fn fetch_all<ID: HgDataId>(
    repo: HgRepoContext,
    request: DataRequest,
) -> impl Stream<Item = Result<DataEntry, Error>> {
    let fetches = request
        .keys
        .into_iter()
        .map(move |key| fetch::<ID>(repo.clone(), key));

    stream::iter(fetches).buffer_unordered(MAX_CONCURRENT_FETCHES_PER_REQUEST)
}

/// Fetch requested data for a single key.
/// Note that this function consumes the repo context in order
/// to construct a file/tree context for the requested blob.
async fn fetch<ID: HgDataId>(repo: HgRepoContext, key: Key) -> Result<DataEntry, Error> {
    let id = ID::from_node_hash(HgNodeHash::from(key.hgid));

    let ctx = id
        .context(repo)
        .await
        .with_context(|| ErrorKind::DataFetchFailed(key.clone()))?
        .with_context(|| ErrorKind::KeyDoesNotExist(key.clone()))?;

    let data = ctx
        .content()
        .await
        .with_context(|| ErrorKind::DataFetchFailed(key.clone()))?;
    let parents = ctx.hg_parents().into();
    let metadata = Metadata::default();

    Ok(DataEntry::new(key, data, parents, metadata))
}
