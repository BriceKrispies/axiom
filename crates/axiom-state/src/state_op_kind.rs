//! Which change an operation makes.

use crate::state_kind::StateKind;

/// The eight changes a patch can describe.
///
/// Fieldless on purpose. The payload a given kind needs lives in
/// [`crate::StateOp`]'s always-present fields rather than in enum variants,
/// because destructuring a data-carrying enum is the one thing safe Rust offers
/// no combinator for — and the applier has to dispatch on this on every
/// operation. As a fieldless enum, `self as usize` indexes a table of function
/// pointers instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum StateOpKind {
    /// Replace a cell's value.
    SetCell = 0,
    /// Add a row that must not already exist.
    TableInsert = 1,
    /// Replace a row that must already exist.
    TableUpdate = 2,
    /// Remove a row that must already exist.
    TableRemove = 3,
    /// Insert an item at a position, shifting later items right.
    SequenceInsert = 4,
    /// Replace the item at a position.
    SequenceReplace = 5,
    /// Remove the item at a position, shifting later items left.
    SequenceRemove = 6,
    /// Add an item at the end.
    SequenceAppend = 7,
}

/// The wire codes, in declaration order. Index = `StateOpKind as usize`.
const CODES: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];

/// The stable names, in declaration order.
const NAMES: [&str; 8] = [
    "set-cell",
    "table-insert",
    "table-update",
    "table-remove",
    "sequence-insert",
    "sequence-replace",
    "sequence-remove",
    "sequence-append",
];

/// Which storage shape each operation may target.
const TARGET_KIND: [StateKind; 8] = [
    StateKind::Cell,
    StateKind::Table,
    StateKind::Table,
    StateKind::Table,
    StateKind::Sequence,
    StateKind::Sequence,
    StateKind::Sequence,
    StateKind::Sequence,
];

/// Whether an operation's effect spans the whole state rather than one granule.
///
/// A cell has only one granule. A sequence insert, remove or append shifts or
/// extends positions, so it cannot be reasoned about independently of any other
/// change to that sequence. A table row and a sequence *replace* are addressable
/// on their own.
const WHOLE_ENTRY: [bool; 8] = [true, false, false, false, true, false, true, true];

/// Decode table; index = wire code.
const BY_CODE: [Option<StateOpKind>; 9] = [
    None,
    Some(StateOpKind::SetCell),
    Some(StateOpKind::TableInsert),
    Some(StateOpKind::TableUpdate),
    Some(StateOpKind::TableRemove),
    Some(StateOpKind::SequenceInsert),
    Some(StateOpKind::SequenceReplace),
    Some(StateOpKind::SequenceRemove),
    Some(StateOpKind::SequenceAppend),
];

impl StateOpKind {
    /// The stable wire code. Never `0`.
    pub const fn code(self) -> u8 {
        CODES[self as usize]
    }

    /// The stable kebab-case name.
    pub const fn name(self) -> &'static str {
        NAMES[self as usize]
    }

    /// The storage shape this operation may be applied to.
    pub const fn target_kind(self) -> StateKind {
        TARGET_KIND[self as usize]
    }

    /// Whether this operation's effect spans the whole state.
    pub const fn is_whole_entry(self) -> bool {
        WHOLE_ENTRY[self as usize]
    }

    /// Decode a wire code, or `None` when it names no operation.
    pub fn from_code(code: u8) -> Option<StateOpKind> {
        BY_CODE.get(usize::from(code)).copied().flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [StateOpKind; 8] = [
        StateOpKind::SetCell,
        StateOpKind::TableInsert,
        StateOpKind::TableUpdate,
        StateOpKind::TableRemove,
        StateOpKind::SequenceInsert,
        StateOpKind::SequenceReplace,
        StateOpKind::SequenceRemove,
        StateOpKind::SequenceAppend,
    ];

    #[test]
    fn every_kind_round_trips_through_its_code() {
        ALL.into_iter()
            .for_each(|kind| assert_eq!(StateOpKind::from_code(kind.code()), Some(kind)));
    }

    #[test]
    fn codes_are_one_based_and_contiguous() {
        let codes: Vec<u8> = ALL.iter().map(|k| k.code()).collect();
        assert_eq!(codes, (1..=8).collect::<Vec<u8>>());
    }

    #[test]
    fn zero_and_out_of_range_codes_decode_to_nothing() {
        assert_eq!(StateOpKind::from_code(0), None);
        assert_eq!(StateOpKind::from_code(9), None);
        assert_eq!(StateOpKind::from_code(255), None);
    }

    #[test]
    fn names_are_distinct_and_non_empty() {
        let mut names: Vec<&str> = ALL.iter().map(|k| k.name()).collect();
        assert!(names.iter().all(|n| !n.is_empty()));
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), 8);
    }

    #[test]
    fn each_operation_targets_the_shape_it_belongs_to() {
        assert_eq!(StateOpKind::SetCell.target_kind(), StateKind::Cell);
        [
            StateOpKind::TableInsert,
            StateOpKind::TableUpdate,
            StateOpKind::TableRemove,
        ]
        .into_iter()
        .for_each(|kind| assert_eq!(kind.target_kind(), StateKind::Table));
        [
            StateOpKind::SequenceInsert,
            StateOpKind::SequenceReplace,
            StateOpKind::SequenceRemove,
            StateOpKind::SequenceAppend,
        ]
        .into_iter()
        .for_each(|kind| assert_eq!(kind.target_kind(), StateKind::Sequence));
    }

    #[test]
    fn only_row_and_position_addressable_operations_are_granular() {
        assert!(StateOpKind::SetCell.is_whole_entry());
        assert!(!StateOpKind::TableInsert.is_whole_entry());
        assert!(!StateOpKind::TableUpdate.is_whole_entry());
        assert!(!StateOpKind::TableRemove.is_whole_entry());
        assert!(!StateOpKind::SequenceReplace.is_whole_entry());
        // Insert, remove and append move positions, so they span the sequence.
        assert!(StateOpKind::SequenceInsert.is_whole_entry());
        assert!(StateOpKind::SequenceRemove.is_whole_entry());
        assert!(StateOpKind::SequenceAppend.is_whole_entry());
    }
}
