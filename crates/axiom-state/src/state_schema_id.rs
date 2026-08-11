//! The identity of a schema at a particular version.

use axiom_kernel::{SchemaVersion, StableHash};

/// A digest of "which schema, at which version, with which shape".
///
/// Stamped into every snapshot, so a snapshot decoded against the wrong schema —
/// or against the right schema after its shape changed — is a deterministic
/// error rather than a silent misread.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StateSchemaId(u64);

/// Fold a version into one digest word: major in the high half, minor in the low.
pub(crate) const fn version_word(version: SchemaVersion) -> u64 {
    ((version.major() as u64) << 16) | (version.minor() as u64)
}

impl StateSchemaId {
    /// Derive the identity from a schema's name, version, and shape digest.
    pub fn of(name: &str, version: SchemaVersion, structure: StableHash) -> Self {
        StateSchemaId(
            StableHash::of_words(&[
                StableHash::of_bytes(name.as_bytes()).raw(),
                version_word(version),
                structure.raw(),
            ])
            .raw(),
        )
    }

    /// Rebuild from a raw digest (decoding a stored snapshot).
    pub const fn from_raw(raw: u64) -> Self {
        StateSchemaId(raw)
    }

    /// The raw 64-bit digest.
    pub const fn raw(self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shape() -> StableHash {
        StableHash::of_words(&[7, 9])
    }

    #[test]
    fn the_same_inputs_always_digest_the_same_way() {
        let one = StateSchemaId::of("puzzle", SchemaVersion::new(1, 0), shape());
        let other = StateSchemaId::of("puzzle", SchemaVersion::new(1, 0), shape());
        assert_eq!(one, other);
    }

    #[test]
    fn the_name_the_version_and_the_shape_each_change_the_identity() {
        let base = StateSchemaId::of("puzzle", SchemaVersion::new(1, 0), shape());
        assert_ne!(base, StateSchemaId::of("other", SchemaVersion::new(1, 0), shape()));
        assert_ne!(base, StateSchemaId::of("puzzle", SchemaVersion::new(2, 0), shape()));
        assert_ne!(base, StateSchemaId::of("puzzle", SchemaVersion::new(1, 1), shape()));
        assert_ne!(
            base,
            StateSchemaId::of("puzzle", SchemaVersion::new(1, 0), StableHash::of_words(&[1]))
        );
    }

    #[test]
    fn identity_round_trips_through_its_raw_digest() {
        let id = StateSchemaId::of("puzzle", SchemaVersion::new(1, 0), shape());
        assert_eq!(StateSchemaId::from_raw(id.raw()), id);
    }

    #[test]
    fn the_version_word_separates_major_from_minor() {
        assert_eq!(version_word(SchemaVersion::new(1, 0)), 1 << 16);
        assert_eq!(version_word(SchemaVersion::new(0, 1)), 1);
        assert_ne!(
            version_word(SchemaVersion::new(1, 0)),
            version_word(SchemaVersion::new(0, 1))
        );
    }
}
