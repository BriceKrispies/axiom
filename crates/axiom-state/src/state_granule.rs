//! Which part of a state a change touches.

use axiom_kernel::StableHash;

use crate::state_kind::StateKind;

/// The addressed part of a state: the whole thing, one table row, or one
/// position in a sequence.
///
/// A tagged struct rather than an enum with payloads, for the same reason
/// [`crate::StateOp`] is: reading it back would otherwise need a `match`. `kind`
/// says which field carries meaning, and both are always present.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StateGranule {
    kind: StateKind,
    key: Vec<u8>,
    index: u32,
}

impl StateGranule {
    /// The whole state — a cell, or a state that appeared or disappeared.
    pub const fn whole(kind: StateKind) -> Self {
        StateGranule {
            kind,
            key: Vec::new(),
            index: 0,
        }
    }

    /// One table row, addressed by its encoded key.
    pub const fn row(key: Vec<u8>) -> Self {
        StateGranule {
            kind: StateKind::Table,
            key,
            index: 0,
        }
    }

    /// One position in a sequence.
    pub const fn position(index: u32) -> Self {
        StateGranule {
            kind: StateKind::Sequence,
            key: Vec::new(),
            index,
        }
    }

    /// The storage shape this granule belongs to.
    pub const fn kind(&self) -> StateKind {
        self.kind
    }

    /// The encoded row key, for a table row.
    pub fn key(&self) -> &[u8] {
        &self.key
    }

    /// The position, for a sequence item.
    pub const fn index(&self) -> u32 {
        self.index
    }

    /// The granule's digest, for a compact stable label.
    pub fn hash(&self) -> StableHash {
        StableHash::of_words(&[
            u64::from(self.kind.code()),
            u64::from(self.index),
            StableHash::of_bytes(&self.key).raw(),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_whole_state_granule_carries_neither_key_nor_position() {
        let whole = StateGranule::whole(StateKind::Cell);
        assert_eq!(whole.kind(), StateKind::Cell);
        assert!(whole.key().is_empty());
        assert_eq!(whole.index(), 0);
    }

    #[test]
    fn a_row_granule_carries_its_key() {
        let row = StateGranule::row(vec![1, 2]);
        assert_eq!(row.kind(), StateKind::Table);
        assert_eq!(row.key(), &[1, 2]);
    }

    #[test]
    fn a_position_granule_carries_its_index() {
        let position = StateGranule::position(4);
        assert_eq!(position.kind(), StateKind::Sequence);
        assert_eq!(position.index(), 4);
        assert!(position.key().is_empty());
    }

    #[test]
    fn distinct_granules_digest_distinctly_and_stably() {
        assert_eq!(
            StateGranule::row(vec![1]).hash(),
            StateGranule::row(vec![1]).hash()
        );
        assert_ne!(
            StateGranule::row(vec![1]).hash(),
            StateGranule::row(vec![2]).hash()
        );
        assert_ne!(
            StateGranule::position(1).hash(),
            StateGranule::position(2).hash()
        );
        assert_ne!(
            StateGranule::whole(StateKind::Cell).hash(),
            StateGranule::whole(StateKind::Table).hash()
        );
    }

    #[test]
    fn granules_order_deterministically() {
        let mut granules = vec![
            StateGranule::row(vec![2]),
            StateGranule::row(vec![1]),
        ];
        granules.sort();
        assert_eq!(granules[0].key(), &[1]);
    }
}
