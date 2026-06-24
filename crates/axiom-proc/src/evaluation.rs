//! [`Evaluation`] — a resumable, budgeted recipe evaluation.
//!
//! Evaluation processes nodes in order, drawing from the entropy stream as it
//! goes. [`Self::step`] advances by a bounded number of nodes and yields, so a
//! caller can spread one evaluation across many frames without ever running
//! unbounded. Because nodes are always processed in the same order regardless of
//! the step budget, the artifact + trace are **independent of the budget**:
//! evaluating one node at a time produces byte-identical output to evaluating all
//! at once. Branchless (the per-step body is an `Option::into_iter().for_each`, so
//! a finished evaluation simply skips it).

use axiom_entropy::EntropyStream;

use crate::artifact::Artifact;
use crate::node::{apply, op_code, RecipeNode};
use crate::trace::ProcTrace;

/// A resumable recipe evaluation. Drive it with [`Self::step`] until
/// [`Self::is_done`], then take [`Self::into_output`].
#[derive(Debug)]
pub struct Evaluation {
    nodes: Vec<RecipeNode>,
    generator_version: u32,
    stream: EntropyStream,
    values: Vec<u64>,
    steps: Vec<(u32, u64)>,
    cursor: usize,
}

impl Evaluation {
    pub(crate) fn new(
        nodes: Vec<RecipeNode>,
        generator_version: u32,
        stream: EntropyStream,
    ) -> Self {
        Evaluation {
            nodes,
            generator_version,
            stream,
            values: Vec::new(),
            steps: Vec::new(),
            cursor: 0,
        }
    }

    /// Whether every node has been evaluated.
    pub fn is_done(&self) -> bool {
        self.cursor == self.nodes.len()
    }

    /// Advance the evaluation by up to `budget` nodes, then yield. Stepping past
    /// the end is a harmless no-op, so an over-budget step simply finishes.
    pub fn step(&mut self, budget: usize) {
        (0..budget).for_each(|_| self.process_one());
    }

    /// Evaluate the node at the cursor (if any) and advance. Branchless: a
    /// finished evaluation has no node at the cursor, so the body is skipped and
    /// the clamped cursor stays put.
    fn process_one(&mut self) {
        let next = self.nodes.get(self.cursor).copied();
        next.into_iter().for_each(|node| {
            let value = apply(&node, &self.values, &mut self.stream);
            self.values.push(value);
            self.steps.push((op_code(&node), value));
        });
        self.cursor = (self.cursor + 1).min(self.nodes.len());
    }

    /// Consume the evaluation into its `(Artifact, ProcTrace)`. Reflects the work
    /// done so far; call it once [`Self::is_done`].
    pub fn into_output(self) -> (Artifact, ProcTrace) {
        (
            Artifact::new(self.generator_version, self.values),
            ProcTrace::new(self.steps),
        )
    }
}
