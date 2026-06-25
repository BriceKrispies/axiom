//! [`ProcTrace`] — the recorded decision log of a recipe evaluation.
//!
//! One step per node, in evaluation order: the node's op discriminant and the
//! value it produced. The trace explains *how* the artifact was made; it
//! serializes to canonical bytes and carries a stable digest, exactly like the
//! artifact, so the two boundaries can be golden-compared independently.

use axiom_kernel::{BinaryWriter, StableHash};

/// The ordered decision log: `(op_code, value)` per evaluated node.
#[derive(Debug, PartialEq, Eq)]
pub struct ProcTrace {
    steps: Vec<(u32, u64)>,
}

impl ProcTrace {
    pub(crate) fn new(steps: Vec<(u32, u64)>) -> Self {
        ProcTrace { steps }
    }

    /// The recorded steps, in evaluation order.
    pub fn steps(&self) -> &[(u32, u64)] {
        &self.steps
    }

    /// How many steps the trace recorded (one per evaluated node).
    pub fn len(&self) -> usize {
        self.steps.len()
    }

    /// Whether the trace is empty (an empty recipe).
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// The canonical bytes: length, then each `(op_code, value)` pair.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut writer = BinaryWriter::new();
        writer.write_u64(self.steps.len() as u64);
        self.steps.iter().for_each(|&(op_code, value)| {
            writer.write_u32(op_code);
            writer.write_u64(value);
        });
        writer.into_bytes()
    }

    /// The stable digest over [`Self::to_bytes`].
    pub fn digest(&self) -> StableHash {
        StableHash::of_bytes(&self.to_bytes())
    }
}
