/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::convert::TryInto;
use std::fmt;
use std::sync::Arc;

use anyhow::{format_err, Error, Result};
use clap::{App, Arg, ArgMatches, SubCommand};
use fbinit::FacebookInit;
use futures::{
    compat::Future01CompatExt,
    future::{FutureExt as _, TryFutureExt},
};
use futures_ext::{BoxFuture, FutureExt};
use futures_old::prelude::*;

use blobstore::{Blobstore, BlobstoreGetData};
use blobstore_factory::{make_blobstore, BlobstoreOptions, ReadOnlyStorage};
use cacheblob::{new_memcache_blobstore, CacheBlobstoreExt};
use cloned::cloned;
use cmdlib::args;
use context::CoreContext;
use futures_old::future;
use git_types::Tree as GitTree;
use mercurial_types::{HgChangesetEnvelope, HgFileEnvelope, HgManifestEnvelope};
use metaconfig_types::{BlobConfig, BlobstoreId, Redaction, ScrubAction, StorageConfig};
use mononoke_types::{FileContents, RepositoryId};
use prefixblob::PrefixBlobstore;
use redactedblobstore::{RedactedBlobstore, RedactedBlobstoreConfig, SqlRedactedContentStore};
use scuba_ext::{ScubaSampleBuilder, ScubaSampleBuilderExt};
use slog::{info, warn, Logger};
use sql_ext::facebook::MysqlOptions;
use std::collections::HashMap;
use std::iter::FromIterator;
use std::str::FromStr;

use crate::error::SubcommandError;

pub const BLOBSTORE_FETCH: &str = "blobstore-fetch";
const SCRUB_BLOBSTORE_ACTION_ARG: &str = "scrub-blobstore-action";

pub fn build_subcommand<'a, 'b>() -> App<'a, 'b> {
    SubCommand::with_name(BLOBSTORE_FETCH)
        .about("fetches blobs from manifold")
        .args_from_usage("[KEY]    'key of the blob to be fetched'")
        .arg(
            Arg::with_name("decode-as")
                .long("decode-as")
                .short("d")
                .takes_value(true)
                .possible_values(&["auto", "changeset", "manifest", "file", "contents", "git-tree"])
                .required(false)
                .help("if provided decode the value"),
        )
        .arg(
            Arg::with_name("use-memcache")
                .long("use-memcache")
                .short("m")
                .takes_value(true)
                .possible_values(&["cache-only", "no-fill", "fill-mc"])
                .required(false)
                .help("Use memcache to cache access to the blob store"),
        )
        .arg(
            Arg::with_name("no-prefix")
                .long("no-prefix")
                .short("P")
                .takes_value(false)
                .required(false)
                .help("Don't prepend a prefix based on the repo id to the key"),
        )
        .arg(
            Arg::with_name("inner-blobstore-id")
                .long("inner-blobstore-id")
                .takes_value(true)
                .required(false)
                .help("If main blobstore in the storage config is a multiplexed one, use inner blobstore with this id")
        )
        .arg(
            Arg::with_name(SCRUB_BLOBSTORE_ACTION_ARG)
                .long(SCRUB_BLOBSTORE_ACTION_ARG)
                .takes_value(true)
                .required(false)
                .help("Enable ScrubBlobstore with the given action. Checks for keys missing from stores. In ReportOnly mode this logs only, otherwise it performs a copy to the missing stores."),
        )
}

fn get_blobconfig(
    blob_config: BlobConfig,
    inner_blobstore_id: Option<u64>,
    scrub_action: Option<ScrubAction>,
) -> Result<BlobConfig> {
    match inner_blobstore_id {
        None => Ok(blob_config),
        Some(inner_blobstore_id) => match blob_config {
            BlobConfig::Multiplexed { blobstores, .. } => {
                let seeked_id = BlobstoreId::new(inner_blobstore_id);
                blobstores
                    .into_iter()
                    .find_map(|(blobstore_id, blobstore)| {
                        if blobstore_id == seeked_id {
                            Some(blobstore)
                        } else {
                            None
                        }
                    })
                    .ok_or(format_err!(
                        "could not find a blobstore with id {}",
                        inner_blobstore_id
                    ))
            }
            _ => Err(format_err!(
                "inner-blobstore-id supplied but blobstore is not multiplexed"
            )),
        },
    }
    .map(|mut config| {
        scrub_action.map(|action| config.set_scrubbed(action));
        config
    })
}

fn get_blobstore(
    fb: FacebookInit,
    storage_config: StorageConfig,
    inner_blobstore_id: Option<u64>,
    scrub_action: Option<ScrubAction>,
    mysql_options: MysqlOptions,
    logger: Logger,
    readonly_storage: ReadOnlyStorage,
    blobstore_options: BlobstoreOptions,
) -> BoxFuture<Arc<dyn Blobstore>, Error> {
    async move {
        let blobconfig =
            get_blobconfig(storage_config.blobstore, inner_blobstore_id, scrub_action)?;

        make_blobstore(
            fb,
            blobconfig,
            mysql_options,
            readonly_storage,
            &blobstore_options,
            &logger,
        )
        .await
    }
    .boxed()
    .compat()
    .boxify()
}

pub async fn subcommand_blobstore_fetch<'a>(
    fb: FacebookInit,
    logger: Logger,
    matches: &'a ArgMatches<'_>,
    sub_m: &'a ArgMatches<'_>,
) -> Result<(), SubcommandError> {
    let repo_id = args::get_repo_id(fb, &matches)?;
    let (_, config) = args::get_config(fb, &matches)?;
    let redaction = config.redaction;
    let storage_config = config.storage_config;
    let inner_blobstore_id = args::get_u64_opt(&sub_m, "inner-blobstore-id");
    let scrub_action = sub_m
        .value_of(SCRUB_BLOBSTORE_ACTION_ARG)
        .map(ScrubAction::from_str)
        .transpose()?;
    let mysql_options = args::parse_mysql_options(&matches);
    let blobstore_options = args::parse_blobstore_options(&matches);
    let readonly_storage = args::parse_readonly_storage(&matches);
    let blobstore_fut = get_blobstore(
        fb,
        storage_config,
        inner_blobstore_id,
        scrub_action,
        mysql_options,
        logger.clone(),
        readonly_storage,
        blobstore_options,
    );

    let common_config = args::load_common_config(fb, &matches)?;
    let scuba_censored_table = common_config.scuba_censored_table;
    let scuba_redaction_builder = ScubaSampleBuilder::with_opt_table(fb, scuba_censored_table);

    let ctx = CoreContext::new_with_logger(fb, logger.clone());
    let key = sub_m.value_of("KEY").unwrap().to_string();
    let decode_as = sub_m.value_of("decode-as").map(|val| val.to_string());
    let use_memcache = sub_m.value_of("use-memcache").map(|val| val.to_string());
    let no_prefix = sub_m.is_present("no-prefix");

    let maybe_redacted_blobs_fut = match redaction {
        Redaction::Enabled => args::open_sql::<SqlRedactedContentStore>(fb, &matches)
            .and_then(|redacted_blobs| {
                redacted_blobs
                    .get_all_redacted_blobs()
                    .map_err(Error::from)
                    .map(HashMap::from_iter)
                    .map(Some)
            })
            .left_future(),
        Redaction::Disabled => future::ok(None).right_future(),
    };

    let value_fut = blobstore_fut.join(maybe_redacted_blobs_fut).and_then({
        cloned!(logger, key, ctx);
        move |(blobstore, maybe_redacted_blobs)| {
            info!(logger, "using blobstore: {:?}", blobstore);
            get_from_sources(
                fb,
                use_memcache,
                blobstore,
                no_prefix,
                key.clone(),
                ctx,
                maybe_redacted_blobs,
                scuba_redaction_builder,
                repo_id,
            )
        }
    });

    value_fut
        .map({
            cloned!(key);
            move |value| {
                println!("{:?}", value);
                if let Some(value) = value {
                    let decode_as = decode_as.as_ref().and_then(|val| {
                        let val = val.as_str();
                        if val == "auto" {
                            detect_decode(&key, &logger)
                        } else {
                            Some(val)
                        }
                    });

                    match decode_as {
                        Some("changeset") => display(&HgChangesetEnvelope::from_blob(value.into())),
                        Some("manifest") => display(&HgManifestEnvelope::from_blob(value.into())),
                        Some("file") => display(&HgFileEnvelope::from_blob(value.into())),
                        // TODO: (rain1) T30974137 add a better way to print out file contents
                        Some("contents") => println!(
                            "{:?}",
                            FileContents::from_encoded_bytes(value.into_raw_bytes())
                        ),
                        Some("git-tree") => display::<GitTree>(&value.try_into()),
                        _ => (),
                    }
                }
            }
        })
        .from_err()
        .compat()
        .await
}

fn get_from_sources<T: Blobstore + Clone>(
    fb: FacebookInit,
    use_memcache: Option<String>,
    blobstore: T,
    no_prefix: bool,
    key: String,
    ctx: CoreContext,
    redacted_blobs: Option<HashMap<String, String>>,
    scuba_redaction_builder: ScubaSampleBuilder,
    repo_id: RepositoryId,
) -> BoxFuture<Option<BlobstoreGetData>, Error> {
    let empty_prefix = "".to_string();

    match use_memcache {
        Some(mode) => {
            let blobstore = new_memcache_blobstore(fb, blobstore, "multiplexed", "").unwrap();
            let blobstore = match no_prefix {
                false => PrefixBlobstore::new(blobstore, repo_id.prefix()),
                true => PrefixBlobstore::new(blobstore, empty_prefix),
            };
            let blobstore = RedactedBlobstore::new(
                blobstore,
                RedactedBlobstoreConfig::new(redacted_blobs, scuba_redaction_builder),
            );
            get_cache(ctx.clone(), &blobstore, key.clone(), mode)
                .map(|opt_blob| opt_blob.map(Into::into))
                .boxify()
        }
        None => {
            let blobstore = match no_prefix {
                false => PrefixBlobstore::new(blobstore, repo_id.prefix()),
                true => PrefixBlobstore::new(blobstore, empty_prefix),
            };
            let blobstore = RedactedBlobstore::new(
                blobstore,
                RedactedBlobstoreConfig::new(redacted_blobs, scuba_redaction_builder),
            );
            blobstore.get(ctx, key).compat().boxify()
        }
    }
}

fn display<T>(res: &Result<T>)
where
    T: fmt::Display + fmt::Debug,
{
    match res {
        Ok(val) => println!("---\n{}---", val),
        err => println!("{:?}", err),
    }
}

fn detect_decode(key: &str, logger: &Logger) -> Option<&'static str> {
    // Use a simple heuristic to figure out how to decode this key.
    if key.find("hgchangeset.").is_some() {
        info!(logger, "Detected changeset key");
        Some("changeset")
    } else if key.find("hgmanifest.").is_some() {
        info!(logger, "Detected manifest key");
        Some("manifest")
    } else if key.find("hgfilenode.").is_some() {
        info!(logger, "Detected file key");
        Some("file")
    } else if key.find("content.").is_some() {
        info!(logger, "Detected content key");
        Some("contents")
    } else if key.find("git.tree.").is_some() {
        info!(logger, "Detected git-tree key");
        Some("git-tree")
    } else {
        warn!(
            logger,
            "Unable to detect how to decode this blob based on key";
            "key" => key,
        );
        None
    }
}

fn get_cache<B: CacheBlobstoreExt>(
    ctx: CoreContext,
    blobstore: &B,
    key: String,
    mode: String,
) -> BoxFuture<Option<BlobstoreGetData>, Error> {
    if mode == "cache-only" {
        blobstore.get_cache_only(ctx, key)
    } else if mode == "no-fill" {
        blobstore.get_no_cache_fill(ctx, key)
    } else {
        blobstore.get(ctx, key).compat().boxify()
    }
}
