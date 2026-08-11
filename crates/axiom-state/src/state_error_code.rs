//! The machine identity of a state failure.

/// What went wrong, as a stable code rather than a string.
///
/// A fieldless enum so `self as usize` indexes the code table — the same shape
/// the recipe layer's error uses, and the reason one exhaustive test covers
/// every variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u16)]
pub enum StateErrorCode {
    /// A schema declaration is malformed (empty name, empty path).
    InvalidSchema = 0,
    /// Two declarations share a path, or two paths digest to one [`crate::StateId`].
    DuplicateStateIdentity = 1,
    /// An identity the schema does not declare, or the snapshot does not hold.
    UnknownStateIdentity = 2,
    /// The stored type does not match the requested one, or a payload failed to
    /// decode as the requested type.
    StateTypeMismatch = 3,
    /// An operation illegal for its target's kind, or against an undeclared target.
    InvalidPatch = 4,
    /// Two origins wrote the same granule.
    ConflictingWrites = 5,
    /// Insert on an existing key, or update/remove on a missing one.
    InvalidTableOperation = 6,
    /// A sequence index outside the valid range for the operation.
    InvalidSequenceOperation = 7,
    /// A value's `Reflect` encode or decode failed.
    SerializationFailed = 8,
    /// An incompatible schema major version.
    SchemaVersionMismatch = 9,
    /// No sequential migration path from the source version to the target.
    UnsupportedMigration = 10,
    /// Bad magic, truncation, or an out-of-range discriminant in stored bytes.
    CorruptedSnapshot = 11,
    /// A view read or wrote a state outside its declared access.
    UndeclaredAccess = 12,
    /// A snapshot was built leaving a declared state unset.
    IncompleteSnapshot = 13,
}

/// The stable wire codes, in declaration order. Index = `StateErrorCode as usize`.
const CODES: [u16; 14] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14];

/// The stable diagnostic names, in declaration order.
const NAMES: [&str; 14] = [
    "invalid-schema",
    "duplicate-state-identity",
    "unknown-state-identity",
    "state-type-mismatch",
    "invalid-patch",
    "conflicting-writes",
    "invalid-table-operation",
    "invalid-sequence-operation",
    "serialization-failed",
    "schema-version-mismatch",
    "unsupported-migration",
    "corrupted-snapshot",
    "undeclared-access",
    "incomplete-snapshot",
];

impl StateErrorCode {
    /// The stable wire code. Never `0`, so a zero in a corrupt stream is not a
    /// valid code.
    pub const fn code(self) -> u16 {
        CODES[self as usize]
    }

    /// The stable kebab-case diagnostic name.
    pub const fn name(self) -> &'static str {
        NAMES[self as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every variant, in declaration order — the list one exhaustive test walks.
    const ALL: [StateErrorCode; 14] = [
        StateErrorCode::InvalidSchema,
        StateErrorCode::DuplicateStateIdentity,
        StateErrorCode::UnknownStateIdentity,
        StateErrorCode::StateTypeMismatch,
        StateErrorCode::InvalidPatch,
        StateErrorCode::ConflictingWrites,
        StateErrorCode::InvalidTableOperation,
        StateErrorCode::InvalidSequenceOperation,
        StateErrorCode::SerializationFailed,
        StateErrorCode::SchemaVersionMismatch,
        StateErrorCode::UnsupportedMigration,
        StateErrorCode::CorruptedSnapshot,
        StateErrorCode::UndeclaredAccess,
        StateErrorCode::IncompleteSnapshot,
    ];

    #[test]
    fn every_code_is_distinct_non_zero_and_ordered() {
        let codes: Vec<u16> = ALL.iter().map(|c| c.code()).collect();
        assert_eq!(codes, (1..=14).collect::<Vec<u16>>());
    }

    #[test]
    fn every_name_is_distinct_and_non_empty() {
        let mut names: Vec<&str> = ALL.iter().map(|c| c.name()).collect();
        assert_eq!(names.len(), 14);
        assert!(names.iter().all(|n| !n.is_empty()));
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), 14, "two error codes share a name");
    }

    #[test]
    fn codes_order_by_declaration() {
        assert!(StateErrorCode::InvalidSchema < StateErrorCode::IncompleteSnapshot);
    }
}
