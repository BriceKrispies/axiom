//! One stored state: its shape, and its canonical bytes.

use axiom_kernel::StableHash;

use crate::state_kind::StateKind;
use crate::state_shape_id::StateShapeId;

/// The stored contents of one declared state.
///
/// Private to the layer: callers reach state through typed keys, never through
/// this. It exists so the substrate can hold heterogeneous typed values without
/// runtime type erasure — the bytes are opaque, and [`StateShapeId`] is what says
/// whether decoding them as a given type is legitimate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StateEntry {
    kind: StateKind,
    shape: StateShapeId,
    payload: Vec<u8>,
}

impl StateEntry {
    /// Store a payload under a shape.
    pub(crate) const fn new(kind: StateKind, shape: StateShapeId, payload: Vec<u8>) -> Self {
        StateEntry {
            kind,
            shape,
            payload,
        }
    }

    /// The storage shape.
    pub(crate) const fn kind(&self) -> StateKind {
        self.kind
    }

    /// The shape identity of the stored values.
    pub(crate) const fn shape(&self) -> StateShapeId {
        self.shape
    }

    /// The canonical bytes.
    pub(crate) fn payload(&self) -> &[u8] {
        &self.payload
    }

    /// The same entry carrying a different payload.
    pub(crate) fn with_payload(&self, payload: Vec<u8>) -> Self {
        StateEntry {
            kind: self.kind,
            shape: self.shape,
            payload,
        }
    }

    /// This entry's digest: shape and contents together, so a payload that moves
    /// between two states of different shape is not mistaken for the same thing.
    pub(crate) fn digest(&self) -> StableHash {
        StableHash::of_words(&[
            u64::from(self.kind.code()),
            self.shape.raw(),
            StableHash::of_bytes(&self.payload).raw(),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry() -> StateEntry {
        StateEntry::new(
            StateKind::Cell,
            StateShapeId::cell_of::<u64>(),
            vec![1, 2, 3],
        )
    }

    #[test]
    fn an_entry_carries_its_shape_and_bytes() {
        let entry = entry();
        assert_eq!(entry.kind(), StateKind::Cell);
        assert_eq!(entry.shape(), StateShapeId::cell_of::<u64>());
        assert_eq!(entry.payload(), &[1, 2, 3]);
    }

    #[test]
    fn replacing_the_payload_keeps_the_shape() {
        let replaced = entry().with_payload(vec![9]);
        assert_eq!(replaced.kind(), StateKind::Cell);
        assert_eq!(replaced.shape(), StateShapeId::cell_of::<u64>());
        assert_eq!(replaced.payload(), &[9]);
    }

    #[test]
    fn the_digest_is_stable_for_identical_entries() {
        assert_eq!(entry().digest(), entry().digest());
    }

    #[test]
    fn the_digest_covers_the_payload_the_kind_and_the_type() {
        let base = entry();
        assert_ne!(base.digest(), base.with_payload(vec![4]).digest());
        assert_ne!(
            base.digest(),
            StateEntry::new(
                StateKind::Sequence,
                StateShapeId::cell_of::<u64>(),
                vec![1, 2, 3]
            )
            .digest()
        );
        assert_ne!(
            base.digest(),
            StateEntry::new(
                StateKind::Cell,
                StateShapeId::cell_of::<u32>(),
                vec![1, 2, 3]
            )
            .digest()
        );
    }
}
