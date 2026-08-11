//! One proposed change, as a tagged struct.

use crate::state_id::StateId;
use crate::state_op_kind::StateOpKind;
use crate::state_origin::StateOrigin;

/// A single change to one state.
///
/// A **tagged struct**, not an enum with per-variant payloads: `kind` selects
/// which of the always-present fields carry meaning. That shape is what lets the
/// applier dispatch through a table of function pointers instead of a `match`,
/// and it is the difference between an applier that can be written at all under
/// the Branchless Law and one that cannot.
///
/// | kind | `key` | `index` | `value` |
/// |---|---|---|---|
/// | `SetCell` | — | — | the new value |
/// | `TableInsert` / `TableUpdate` | the row key | — | the row value |
/// | `TableRemove` | the row key | — | — |
/// | `SequenceInsert` / `SequenceReplace` | — | the position | the item |
/// | `SequenceRemove` | — | the position | — |
/// | `SequenceAppend` | — | — | the item |
///
/// Unused fields are empty or zero. Every byte string here is a canonical
/// `Reflect` encoding, so an operation crossing a process boundary needs no type
/// knowledge to be carried, stored, or hashed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateOp {
    kind: StateOpKind,
    target: StateId,
    origin: StateOrigin,
    key: Vec<u8>,
    index: u32,
    value: Vec<u8>,
}

impl StateOp {
    /// Build an operation from its parts.
    pub(crate) const fn new(
        kind: StateOpKind,
        target: StateId,
        origin: StateOrigin,
        key: Vec<u8>,
        index: u32,
        value: Vec<u8>,
    ) -> Self {
        StateOp {
            kind,
            target,
            origin,
            key,
            index,
            value,
        }
    }

    /// Which change this is.
    pub const fn kind(&self) -> StateOpKind {
        self.kind
    }

    /// Which state it changes.
    pub const fn target(&self) -> StateId {
        self.target
    }

    /// Who wrote it.
    pub const fn origin(&self) -> StateOrigin {
        self.origin
    }

    /// The encoded row key, for table operations.
    pub fn key(&self) -> &[u8] {
        &self.key
    }

    /// The position, for sequence operations.
    pub const fn index(&self) -> u32 {
        self.index
    }

    /// The encoded value or item.
    pub fn value(&self) -> &[u8] {
        &self.value
    }

    /// The finest part of the target this operation addresses.
    ///
    /// Empty for whole-state operations, the row key for table operations, and
    /// the position for a sequence replace. Two operations on the same target
    /// collide when either spans the whole state, or when their granules match.
    pub(crate) fn granule(&self) -> Vec<u8> {
        let position = self.index.to_le_bytes();
        [self.key.as_slice(), position.as_slice()]
            [usize::from(self.kind == StateOpKind::SequenceReplace)]
            .to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn origin() -> StateOrigin {
        StateOrigin::of_name("test")
    }

    fn target() -> StateId {
        StateId::of_path("op/target")
    }

    #[test]
    fn an_operation_carries_every_part_it_was_built_from() {
        let op = StateOp::new(
            StateOpKind::TableInsert,
            target(),
            origin(),
            vec![1, 2],
            0,
            vec![3, 4],
        );
        assert_eq!(op.kind(), StateOpKind::TableInsert);
        assert_eq!(op.target(), target());
        assert_eq!(op.origin(), origin());
        assert_eq!(op.key(), &[1, 2]);
        assert_eq!(op.index(), 0);
        assert_eq!(op.value(), &[3, 4]);
    }

    #[test]
    fn a_table_operation_is_addressed_by_its_row_key() {
        let op = StateOp::new(
            StateOpKind::TableUpdate,
            target(),
            origin(),
            vec![7],
            0,
            vec![],
        );
        assert_eq!(op.granule(), vec![7]);
    }

    #[test]
    fn a_sequence_replace_is_addressed_by_its_position() {
        let op = StateOp::new(
            StateOpKind::SequenceReplace,
            target(),
            origin(),
            vec![],
            3,
            vec![],
        );
        assert_eq!(op.granule(), 3_u32.to_le_bytes().to_vec());
    }

    #[test]
    fn two_sequence_replaces_at_different_positions_address_different_granules() {
        let at = |index| {
            StateOp::new(
                StateOpKind::SequenceReplace,
                target(),
                origin(),
                vec![],
                index,
                vec![],
            )
        };
        assert_ne!(at(1).granule(), at(2).granule());
    }

    #[test]
    fn a_cell_write_carries_no_key_and_no_position() {
        let op = StateOp::new(StateOpKind::SetCell, target(), origin(), vec![], 0, vec![9]);
        assert!(op.key().is_empty());
        assert_eq!(op.index(), 0);
        assert!(op.granule().is_empty());
    }
}
