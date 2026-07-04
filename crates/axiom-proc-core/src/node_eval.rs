//! The per-node evaluation context a domain evaluator receives.

use axiom_entropy::EntropyStream;
use axiom_recipe::Param;

/// Everything a domain evaluator needs to compute one node's output: the node's
/// operator code, its parameter words, the already-computed outputs of its
/// inputs (in input-slot order), and a deterministic per-node entropy stream.
/// Generic over the output type `Out` (a texture buffer, a mesh buffer, …).
#[derive(Debug)]
pub struct NodeEval<'a, Out> {
    op: u16,
    params: &'a [Param],
    inputs: &'a [Out],
    stream: EntropyStream,
}

impl<'a, Out> NodeEval<'a, Out> {
    /// Build a context (crate-internal — only the executor mints these).
    pub(crate) fn new(op: u16, params: &'a [Param], inputs: &'a [Out], stream: EntropyStream) -> Self {
        Self { op, params, inputs, stream }
    }

    /// The node's operator code.
    pub fn op(&self) -> u16 {
        self.op
    }

    /// The node's parameter words, in slot order.
    pub fn params(&self) -> &[Param] {
        self.params
    }

    /// The already-computed outputs of this node's inputs, in input-slot order.
    pub fn inputs(&self) -> &[Out] {
        self.inputs
    }

    /// The node's deterministic entropy stream — an operator that needs
    /// randomness (jitter, a noise seed) draws it here; a purely deterministic
    /// operator ignores it.
    pub fn stream(&mut self) -> &mut EntropyStream {
        &mut self.stream
    }
}
