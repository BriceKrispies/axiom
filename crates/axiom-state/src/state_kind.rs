//! The three shapes a declared state can take.

/// Which storage shape a declared state uses.
///
/// A fieldless enum, deliberately: `self as usize` indexes a table, which is how
/// every classification in this layer is written without a `match`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum StateKind {
    /// Exactly one typed value.
    Cell = 0,
    /// A deterministically ordered keyed collection.
    Table = 1,
    /// An explicitly ordered sequence, where position is the meaning.
    Sequence = 2,
}

/// The wire codes, in declaration order. Index = `StateKind as usize`.
const CODES: [u8; 3] = [1, 2, 3];

/// The stable names, in declaration order.
const NAMES: [&str; 3] = ["cell", "table", "sequence"];

/// Decode table for [`StateKind::from_code`]; index = wire code.
const BY_CODE: [Option<StateKind>; 4] = [
    None,
    Some(StateKind::Cell),
    Some(StateKind::Table),
    Some(StateKind::Sequence),
];

impl StateKind {
    /// The stable wire code. Never `0`, so a zero byte in a corrupt stream
    /// decodes to `None` rather than to a valid kind.
    pub const fn code(self) -> u8 {
        CODES[self as usize]
    }

    /// The stable lower-case name, for diagnostics and introspection.
    pub const fn name(self) -> &'static str {
        NAMES[self as usize]
    }

    /// Decode a wire code, or `None` when it names no kind.
    pub fn from_code(code: u8) -> Option<StateKind> {
        BY_CODE.get(usize::from(code)).copied().flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_round_trips_through_its_code() {
        [StateKind::Cell, StateKind::Table, StateKind::Sequence]
            .into_iter()
            .for_each(|kind| assert_eq!(StateKind::from_code(kind.code()), Some(kind)));
    }

    #[test]
    fn codes_and_names_are_distinct_and_stable() {
        assert_eq!(StateKind::Cell.code(), 1);
        assert_eq!(StateKind::Table.code(), 2);
        assert_eq!(StateKind::Sequence.code(), 3);
        assert_eq!(StateKind::Cell.name(), "cell");
        assert_eq!(StateKind::Table.name(), "table");
        assert_eq!(StateKind::Sequence.name(), "sequence");
    }

    #[test]
    fn zero_and_out_of_range_codes_decode_to_nothing() {
        assert_eq!(StateKind::from_code(0), None);
        assert_eq!(StateKind::from_code(4), None);
        assert_eq!(StateKind::from_code(255), None);
    }

    #[test]
    fn kinds_order_by_declaration() {
        assert!(StateKind::Cell < StateKind::Table);
        assert!(StateKind::Table < StateKind::Sequence);
    }
}
