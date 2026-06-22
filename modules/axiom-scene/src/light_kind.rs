//! Coarse light-type enumeration.

use axiom_kernel::{BinaryReader, BinaryWriter, KernelResult, Reflect, TypeSchema};

/// The coarse type of a [`crate::Light`].
///
/// `axiom-scene` only models the two light shapes a deterministic engine
/// frame needs to *describe*: directional and point. Shadowing, area
/// lights, IES profiles, photometry, and image-based lighting are
/// renderer concerns, not scene-module concerns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LightKind {
    Directional,
    Point,
}

impl Reflect for LightKind {
    const SCHEMA: TypeSchema = TypeSchema::new("LightKind", &[]);

    /// Write the discriminant (`Directional` = 0, `Point` = 1) without a `match`:
    /// the equality test yields the code directly.
    fn reflect_write(&self, writer: &mut BinaryWriter) {
        u32::from(*self == LightKind::Point).reflect_write(writer);
    }

    /// Read the discriminant back via a table select (any non-zero code reads as
    /// `Point`), avoiding a branch.
    fn reflect_read(reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
        u32::reflect_read(reader)
            .map(|code| [LightKind::Directional, LightKind::Point][usize::from(code != 0)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variants_are_distinct() {
        assert_ne!(LightKind::Directional, LightKind::Point);
    }

    #[test]
    fn variants_are_copy_and_equal() {
        let a = LightKind::Directional;
        let b = a;
        assert_eq!(a, b);
    }

    #[test]
    fn reflect_round_trips_both_variants_and_rejects_truncation() {
        for kind in [LightKind::Directional, LightKind::Point] {
            let mut w = BinaryWriter::new();
            kind.reflect_write(&mut w);
            assert_eq!(
                LightKind::reflect_read(&mut BinaryReader::new(&w.into_bytes())).unwrap(),
                kind
            );
        }
        assert!(LightKind::reflect_read(&mut BinaryReader::new(&[])).is_err());
        assert_eq!(<LightKind as Reflect>::SCHEMA.name(), "LightKind");
    }
}
