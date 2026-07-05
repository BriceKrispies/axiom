//! Opaque reference binding a scene node to an animation/figure the app drives.

use axiom_kernel::{BinaryReader, BinaryWriter, KernelResult, Reflect, TypeSchema};

/// An opaque, stable reference from a renderable node to the posed articulated
/// figure (or clip-driven animation) that animates it.
///
/// `axiom-scene` owns no skeletons, clips, or figures — those live in
/// `axiom-animation` / `axiom-figure`. An [`AnimationRef`] is just a `u64`
/// handle an app assigns to one authored figure+clip binding and stamps onto
/// the renderable node it drives, so a "character" is **one** engine object
/// (identity + transform + mesh + material + texture + this animation binding)
/// rather than a scene node plus a side registry the app hand-syncs each frame.
/// `INVALID` (`0`) means the renderable is static (no bound animation).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AnimationRef(u64);

impl AnimationRef {
    pub const INVALID: AnimationRef = AnimationRef(0);

    pub const fn from_raw(raw: u64) -> Self {
        AnimationRef(raw)
    }

    pub const fn raw(self) -> u64 {
        self.0
    }

    pub const fn is_valid(self) -> bool {
        self.0 != 0
    }
}

impl Reflect for AnimationRef {
    const SCHEMA: TypeSchema = TypeSchema::new("AnimationRef", &[]);

    fn reflect_write(&self, writer: &mut BinaryWriter) {
        self.0.reflect_write(writer);
    }

    fn reflect_read(reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
        u64::reflect_read(reader).map(AnimationRef)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_is_zero() {
        assert!(!AnimationRef::INVALID.is_valid());
    }

    #[test]
    fn non_zero_is_valid_and_stable() {
        let a = AnimationRef::from_raw(7);
        let b = AnimationRef::from_raw(7);
        assert!(a.is_valid());
        assert_eq!(a, b);
    }

    #[test]
    fn ordering_is_numeric() {
        assert!(AnimationRef::from_raw(1) < AnimationRef::from_raw(2));
    }

    #[test]
    fn reflect_round_trips_and_rejects_truncation() {
        let a = AnimationRef::from_raw(7);
        let mut w = BinaryWriter::new();
        a.reflect_write(&mut w);
        assert_eq!(
            AnimationRef::reflect_read(&mut BinaryReader::new(&w.into_bytes())).unwrap(),
            a
        );
        assert!(AnimationRef::reflect_read(&mut BinaryReader::new(&[])).is_err());
        assert_eq!(<AnimationRef as Reflect>::SCHEMA.name(), "AnimationRef");
    }
}
