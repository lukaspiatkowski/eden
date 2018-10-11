// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

mod model;
mod query;
mod repo;
mod response;
mod lfs;

use std::collections::HashMap;

use futures::IntoFuture;
use futures_ext::{BoxFuture, FutureExt};
use slog::Logger;
use tokio::runtime::TaskExecutor;

use metaconfig::repoconfig::RepoConfigs;

use errors::ErrorKind;

pub use self::lfs::BatchRequest;
pub use self::query::{MononokeQuery, MononokeRepoQuery};
pub use self::repo::MononokeRepo;
pub use self::response::MononokeRepoResponse;

pub struct Mononoke {
    repos: HashMap<String, MononokeRepo>,
}

impl Mononoke {
    pub fn new(
        logger: Logger,
        config: RepoConfigs,
        myrouter_port: Option<u16>,
        executor: TaskExecutor,
    ) -> Self {
        let logger = logger.clone();
        let repos = config
            .repos
            .into_iter()
            .filter(move |&(_, ref config)| config.enabled)
            .map(move |(name, config)| {
                cloned!(logger, executor);
                let repo = MononokeRepo::new(logger, config, myrouter_port, executor)
                    .expect("Unable to initialize repo");
                (name, repo)
            })
            .collect();

        Self { repos }
    }

    pub fn send_query(
        &self,
        MononokeQuery { repo, kind, .. }: MononokeQuery,
    ) -> BoxFuture<MononokeRepoResponse, ErrorKind> {
        match self.repos.get(&repo) {
            Some(repo) => repo.send_query(kind),
            None => match kind {
                MononokeRepoQuery::LfsBatch { .. } => {
                    // LFS batch request require error in the different format:
                    // json: {"message": "Error message here"}
                    Err(ErrorKind::LFSNotFound(repo)).into_future().boxify()
                }
                _ => Err(ErrorKind::NotFound(repo, None)).into_future().boxify(),
            },
        }
    }
}
