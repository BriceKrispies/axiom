//! The per-node context for an evaluator that brings its own entropy.

use axiom_recipe::{NodeId, Param};

/// One node, handed to an evaluator that supplies its own randomness.
///
/// The difference from [`crate::NodeEval`] is what it does *not* carry. There is
/// no entropy stream, because this context is for domains whose randomness is a
/// single sequential source shared across the whole frame rather than a
/// per-node one keyed by address; and the inputs are not cloned into a fresh
/// buffer, because a domain that evaluates a graph once per emitted item cannot
/// afford an allocation per node.
///
/// Both of those are the same distinction seen from two sides. An address-keyed
/// stream lets a graph be evaluated in any order, or partially, or in parallel —
/// which is what a texture or a mesh wants. A shared sequential stream makes the
/// *order* of the draws the contract, which is what a simulation wants, where
/// one extra draw shifts every later effect. Neither model is more correct; a
/// layer that only offered the first was simply incomplete.
#[derive(Debug)]
pub struct NodeStep<'a, Out> {
    op: u16,
    params: &'a [Param],
    inputs: &'a [NodeId],
    cache: &'a [Out],
}

impl<'a, Out> NodeStep<'a, Out> {
    /// Build a context (crate-internal — only the executor mints these).
    pub(crate) fn new(
        op: u16,
        params: &'a [Param],
        inputs: &'a [NodeId],
        cache: &'a [Out],
    ) -> Self {
        Self {
            op,
            params,
            inputs,
            cache,
        }
    }

    /// The node's operator code.
    pub fn op(&self) -> u16 {
        self.op
    }

    /// The node's parameter words, in slot order.
    pub fn params(&self) -> &[Param] {
        self.params
    }

    /// How many inputs this node declares.
    pub fn input_count(&self) -> usize {
        self.inputs.len()
    }

    /// The output of the node in input slot `slot`, borrowed from the cache
    /// rather than copied out of it.
    ///
    /// `None` for a slot this node does not have. It cannot be `None` for a
    /// slot it *does* have: the graph is validated before evaluation begins and
    /// every input of node *i* is a node before *i*, so its output is already
    /// in the cache.
    pub fn input(&self, slot: usize) -> Option<&Out> {
        self.inputs
            .get(slot)
            .and_then(|id| self.cache.get(id.raw() as usize))
    }

    /// Every input's output, in slot order.
    pub fn inputs(&self) -> impl Iterator<Item = &Out> {
        self.inputs
            .iter()
            .filter_map(|id| self.cache.get(id.raw() as usize))
    }
}
