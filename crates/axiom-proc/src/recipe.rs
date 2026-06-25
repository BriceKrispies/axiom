//! [`Recipe`] — a procedural recipe: a versioned DAG of generic nodes, built by
//! append so inputs only ever reference earlier nodes.

use crate::node::{NodeOp, RecipeNode};

/// A procedural recipe: a `version` plus an ordered list of nodes. Built by the
/// `const_node`/`draw`/`add`/`xor` methods, which each return the new node's index
/// for wiring later nodes. The builder does **not** validate wiring — an
/// out-of-range input simply yields a recipe [`crate::ProcApi`] rejects as
/// invalid — so construction never panics.
#[derive(Debug)]
pub struct Recipe {
    version: u32,
    nodes: Vec<RecipeNode>,
}

impl Recipe {
    /// A new, empty recipe at generator `version`. Bumping the version re-keys the
    /// entropy stream and changes the artifact — versioning is a first-class input.
    pub fn new(version: u32) -> Self {
        Recipe {
            version,
            nodes: Vec::new(),
        }
    }

    fn push(&mut self, op: NodeOp, immediate: u64, inputs: [usize; 2]) -> usize {
        let index = self.nodes.len();
        self.nodes.push(RecipeNode { op, immediate, inputs });
        index
    }

    /// Append a literal node; returns its index.
    pub fn const_node(&mut self, value: u64) -> usize {
        self.push(NodeOp::Const, value, [0, 0])
    }

    /// Append an entropy-draw node; returns its index.
    pub fn draw(&mut self) -> usize {
        self.push(NodeOp::Draw, 0, [0, 0])
    }

    /// Append a wrapping-add of nodes `a` and `b`; returns its index.
    pub fn add(&mut self, a: usize, b: usize) -> usize {
        self.push(NodeOp::Add, 0, [a, b])
    }

    /// Append an xor of nodes `a` and `b`; returns its index.
    pub fn xor(&mut self, a: usize, b: usize) -> usize {
        self.push(NodeOp::Xor, 0, [a, b])
    }

    /// The recipe's generator version.
    pub fn version(&self) -> u32 {
        self.version
    }

    /// How many nodes the recipe has.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether the recipe has no nodes.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub(crate) fn nodes(&self) -> &[RecipeNode] {
        &self.nodes
    }
}
