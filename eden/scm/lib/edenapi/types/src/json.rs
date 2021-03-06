/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Utilities for parsing requests from JSON.
//!
//! This module provides the ability to create various EdenAPI request
//! types from human-editable JSON. This is primarily useful for testing
//! debugging, since it provides a convenient way for a developer to
//! create ad-hoc requests. Some of the EdenAPI testing tools accept
//! requests in this format.
//!
//! Note that even though the request structs implement `Deserialize`,
//! we are explicitly not using their `Deserialize` implementations
//! since the format used here does not correspond exactly to the actual
//! representation used in production. (For examples, hashes are
//! represented as hexadecimal strings rather than as byte arrays.)

use std::convert::TryFrom;
use std::str::FromStr;

use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};

use types::{HgId, Key, RepoPathBuf};

use crate::data::DataRequest;
use crate::history::HistoryRequest;
use crate::tree::CompleteTreeRequest;

/// Parse a `DataRequest` from JSON.
///
/// The request is represented as a JSON object containing a "keys" field
/// consisting of an array of path/filenode pairs.
///
/// Example request:
///
/// ```json
/// {
///   "keys": [
///     ["path/to/file_1", "48f43af456d770b6a78e1ace628319847e05cc24"],
///     ["path/to/file_2", "7dcd6ede35eaaa5b1b16a341b19993e59f9b0dbf"],
///     ["path/to/file_3", "218d708a9f8c3e37cfd7ab916c537449ac5419cd"],
///   ]
/// }
/// ```
///
pub fn parse_data_req(json: &Value) -> Result<DataRequest> {
    let json = json.as_object().context("input must be a JSON object")?;
    let keys = json.get("keys").context("missing field: keys")?;

    Ok(DataRequest {
        keys: parse_keys(keys)?,
    })
}

/// Parse a `HistoryRequest` from JSON.
///
/// The request is represented as a JSON object containing a required
/// "keys" field consisting of an array of path/filenode pairs (similar
/// to a data request) as well as an optional length parameter.
///
/// Example request:
///
/// ```json
/// {
///   "keys": [
///     ["path/to/file_1", "48f43af456d770b6a78e1ace628319847e05cc24"],
///     ["path/to/file_2", "7dcd6ede35eaaa5b1b16a341b19993e59f9b0dbf"],
///     ["path/to/file_3", "218d708a9f8c3e37cfd7ab916c537449ac5419cd"],
///   ],
///   "length": 1,
/// }
/// ```
pub fn parse_history_req(value: &Value) -> Result<HistoryRequest> {
    let value = value.as_object().context("input must be a JSON object")?;
    let length = value
        .get("length")
        .and_then(|d| d.as_u64())
        .map(|d| d as u32);
    let keys = {
        let json_keys = value.get("keys").context("missing field: keys")?;
        parse_keys(json_keys)?
    };

    Ok(HistoryRequest { keys, length })
}

/// Parse a `CompleteTreeRequest` from JSON.
///
/// The request is represented as a JSON object containing the fields
/// needed for a "gettreepack"-style complete tree request. Note that
/// it is generally preferred to request trees using a `DataRequest`
/// for the desired tree nodes, as this is a lot less expensive than
/// fetching complete trees.
///
/// Example request:
///
/// ```json
/// {
///     "rootdir": "path/to/root/dir",
///     "mfnodes": [
///         "8722607999fc5ce35e9af56e6da2c823923291dd",
///         "b7d7ffb1a37c86f00558ff132e57c56bca29dc04"
///     ],
///     "basemfnodes": [
///         "26d6acbabf823b844917f04cfbe6747c80983119",
///         "111caaed68164b939f6e2f58680b462ebc3174c7"
///     ],
///     "depth": 1
/// }
/// ```
///
pub fn parse_tree_req(value: &Value) -> Result<CompleteTreeRequest> {
    let obj = value.as_object().context("input must be a JSON object")?;

    let rootdir = obj.get("rootdir").context("missing field: rootdir")?;
    let rootdir = rootdir.as_str().context("rootdir field must be a string")?;
    let rootdir = RepoPathBuf::from_string(rootdir.to_string())?;

    let mfnodes = obj.get("mfnodes").context("missing field: mfnodes")?;
    let mfnodes = parse_hashes(mfnodes)?;

    let basemfnodes = obj
        .get("basemfnodes")
        .context("missing field: basemfnodes")?;
    let basemfnodes = parse_hashes(basemfnodes)?;

    let depth = obj
        .get("depth")
        .and_then(|d| d.as_u64())
        .map(|d| d as usize);

    Ok(CompleteTreeRequest {
        rootdir,
        mfnodes,
        basemfnodes,
        depth,
    })
}

fn parse_keys(value: &Value) -> Result<Vec<Key>> {
    let arr = value.as_array().context("input must be a JSON array")?;

    let mut keys = Vec::new();
    for i in arr.iter() {
        let json_key = i
            .as_array()
            .context("array items must be [path, hash] arrays")?;

        ensure!(
            json_key.len() == 2,
            "array items must be [path, hash] arrays"
        );

        // Cast slice into 2-element array reference so we can destructure it.
        let [path, hash] = <&[_; 2]>::try_from(&json_key[..2])?;

        let path = path.as_str().context("path must be a string")?;
        let hash = hash.as_str().context("hash must be a string")?;

        let key = make_key(&path, hash)?;
        keys.push(key);
    }

    Ok(keys)
}

fn parse_hashes(value: &Value) -> Result<Vec<HgId>> {
    let array = value
        .as_array()
        .context("node hashes must be a passed as an array")?;
    let mut hashes = Vec::new();
    for hex in array {
        let hex = hex.as_str().context("node hashes must be strings")?;
        let hash = HgId::from_str(hex)?;
        hashes.push(hash);
    }
    Ok(hashes)
}

fn make_key(path: &str, hash: &str) -> Result<Key> {
    let path = if path.is_empty() {
        RepoPathBuf::new()
    } else {
        RepoPathBuf::from_string(path.to_string())?
    };
    let hgid = HgId::from_str(hash)?;
    Ok(Key::new(path, hgid))
}

pub trait FromJson: Sized {
    fn from_json(json: &Value) -> Result<Self>;
}

impl FromJson for DataRequest {
    fn from_json(json: &Value) -> Result<Self> {
        parse_data_req(json)
    }
}

impl FromJson for HistoryRequest {
    fn from_json(json: &Value) -> Result<Self> {
        parse_history_req(json)
    }
}

impl FromJson for CompleteTreeRequest {
    fn from_json(json: &Value) -> Result<Self> {
        parse_tree_req(json)
    }
}

pub trait ToJson {
    fn to_json(&self) -> Value;
}

impl ToJson for HgId {
    fn to_json(&self) -> Value {
        json!(self.to_hex())
    }
}

impl ToJson for Key {
    fn to_json(&self) -> Value {
        json!([&self.path, self.hgid.to_json()])
    }
}

impl<T: ToJson> ToJson for Vec<T> {
    fn to_json(&self) -> Value {
        self.iter().map(ToJson::to_json).collect::<Vec<_>>().into()
    }
}

impl ToJson for DataRequest {
    fn to_json(&self) -> Value {
        json!({ "keys": self.keys.to_json() })
    }
}

impl ToJson for HistoryRequest {
    fn to_json(&self) -> Value {
        json!({ "keys": self.keys.to_json(), "length": self.length })
    }
}

impl ToJson for CompleteTreeRequest {
    fn to_json(&self) -> Value {
        json!({
            "rootdir": self.rootdir,
            "mfnodes": self.mfnodes.to_json(),
            "basemfnodes": self.basemfnodes.to_json(),
            "depth": self.depth,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use quickcheck_macros::quickcheck;

    #[quickcheck]
    fn test_data_req_roundtrip(req: DataRequest) -> bool {
        let json = req.to_json();
        req == DataRequest::from_json(&json).unwrap()
    }

    #[quickcheck]
    fn test_history_req_roundtrip(req: HistoryRequest) -> bool {
        let json = req.to_json();
        req == HistoryRequest::from_json(&json).unwrap()
    }

    #[quickcheck]
    fn test_tree_req_roundtrip(req: CompleteTreeRequest) -> bool {
        let json = req.to_json();
        req == CompleteTreeRequest::from_json(&json).unwrap()
    }
}
