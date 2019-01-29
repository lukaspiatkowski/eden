// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use serde_derive::{Deserialize, Serialize};

use types::node::Node;

#[derive(
    Clone,
    Debug,
    Default,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
    Serialize,
    Deserialize
)]
pub struct Key {
    // Name is usually a file or directory path
    name: Vec<u8>,
    // Node is always a 20 byte hash. This will be changed to a fix length array later.
    node: Node,
}

impl Key {
    pub fn new(name: Vec<u8>, node: Node) -> Self {
        Key { name, node }
    }

    pub fn name(&self) -> &[u8] {
        &self.name
    }

    pub fn node(&self) -> &Node {
        &self.node
    }
}

#[cfg(test)]
use quickcheck;

#[cfg(test)]
impl quickcheck::Arbitrary for Key {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        Key::new(Vec::arbitrary(g), Node::arbitrary(g))
    }
}
