/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![type_length_limit = "4522397"]
use anyhow::Error;
use blobrepo::{save_bonsai_changesets, BlobRepo};
use clap::Arg;
use cmdlib::args;
use cmdlib::helpers::block_execute;
use context::CoreContext;
use cross_repo_sync::rewrite_commit;
use derived_data_utils::derived_data_utils;
use fbinit::FacebookInit;
use futures::{
    compat::Future01CompatExt,
    future::TryFutureExt,
    stream::{self, StreamExt, TryStreamExt},
};
use import_tools::{GitimportPreferences, GitimportTarget};
use mercurial_types::MPath;
use mononoke_types::{BonsaiChangeset, ChangesetId};
use movers::DefaultAction;
use slog::info;
use std::collections::HashMap;
use std::path::Path;

const ARG_GIT_REPOSITORY_PATH: &str = "git-repository-path";
const ARG_DEST_PATH_PREFIX: &str = "destination-path-prefix";

async fn rewrite_file_paths(
    ctx: &CoreContext,
    repo: &BlobRepo,
    path: &Path,
    prefix: &str,
) -> Result<Vec<BonsaiChangeset>, Error> {
    let prefs = GitimportPreferences::default();
    let target = GitimportTarget::FullRepo;
    let import_map = import_tools::gitimport(ctx, repo, path, target, prefs).await?;
    let mut remapped_parents: HashMap<ChangesetId, ChangesetId> = HashMap::new();
    let mover = movers::mover_factory(
        HashMap::new(),
        DefaultAction::PrependPrefix(MPath::new(prefix).unwrap()),
    )?;
    let mut bonsai_changesets = vec![];

    for (_id, (bcs_id, bcs)) in import_map {
        let bcs_mut = bcs.into_mut();
        let rewritten_bcs_opt = rewrite_commit(
            ctx.clone(),
            bcs_mut,
            &remapped_parents,
            mover.clone(),
            repo.clone(),
        )
        .await?;

        if let Some(rewritten_bcs_mut) = rewritten_bcs_opt {
            let rewritten_bcs = rewritten_bcs_mut.freeze()?;
            remapped_parents.insert(bcs_id, rewritten_bcs.get_changeset_id());
            info!(
                ctx.logger(),
                "Remapped {:?} => {:?}",
                bcs_id,
                rewritten_bcs.get_changeset_id(),
            );
            bonsai_changesets.push(rewritten_bcs);
        }
    }
    save_bonsai_changesets(bonsai_changesets.clone(), ctx.clone(), repo.clone())
        .compat()
        .await?;
    Ok(bonsai_changesets)
}

async fn derive_bonsais(
    ctx: &CoreContext,
    repo: &BlobRepo,
    shifted_bcs: &[BonsaiChangeset],
) -> Result<(), Error> {
    let derived_data_types = &repo.get_derived_data_config().derived_data_types;

    let len = derived_data_types.len();
    let mut derived_utils = vec![];
    for ty in derived_data_types {
        let utils = derived_data_utils(repo.clone(), ty)?;
        derived_utils.push(utils);
    }

    stream::iter(derived_utils)
        .map(Ok)
        .try_for_each_concurrent(len, |derived_util| async move {
            for bcs in shifted_bcs {
                let csid = bcs.get_changeset_id();
                derived_util
                    .derive(ctx.clone(), repo.clone(), csid)
                    .compat()
                    .map_ok(|_| ())
                    .await?;
            }
            Result::<(), Error>::Ok(())
        })
        .await
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    let app = args::MononokeApp::new("Import Repository")
        .with_advanced_args_hidden()
        .build()
        .version("0.0.0")
        .about("Automating repository imports")
        .arg(
            Arg::with_name(ARG_GIT_REPOSITORY_PATH)
                .required(true)
                .help("Path to a git repository to import"),
        )
        .arg(
            Arg::with_name(ARG_DEST_PATH_PREFIX)
                .long(ARG_DEST_PATH_PREFIX)
                .required(true)
                .takes_value(true)
                .help("Prefix of the destination folder we import to"),
        );

    let matches = app.get_matches();

    let path = Path::new(matches.value_of(ARG_GIT_REPOSITORY_PATH).unwrap());
    let prefix = matches.value_of(ARG_DEST_PATH_PREFIX).unwrap();
    args::init_cachelib(fb, &matches, None);

    let logger = args::init_logging(fb, &matches);
    let ctx = CoreContext::new_with_logger(fb, logger.clone());
    let repo = args::create_repo(fb, &logger, &matches);
    block_execute(
        async {
            let repo = repo.compat().await?;
            let shifted_bcs = rewrite_file_paths(&ctx, &repo, &path, &prefix).await?;
            derive_bonsais(&ctx, &repo, &shifted_bcs).await
        },
        fb,
        "repo_import",
        &logger,
        &matches,
        cmdlib::monitoring::AliveService,
    )
}
