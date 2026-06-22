//! Opaque reference to a material resource owned outside the scene module.

use axiom_kernel::{BinaryReader, BinaryWriter, KernelResult, Reflect, TypeSchema};

/// An opaque, stable reference to a material resource.
///
/// `axiom-scene` does not own materials; a [`MaterialRef`] is just a
/// `u64` identity that future resource/render modules (or apps) will
/// resolve to actual material data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MaterialRef(u64);

impl MaterialRef {
    pub const INVALID: MaterialRef = MaterialRef(0);

    pub const fn from_raw(raw: u64) -> Self {
        MaterialRef(raw)
    }

    pub const fn raw(self) -> u64 {
        self.0
    }

    pub const fn is_valid(self) -> bool {
        self.0 != 0
    }
}

impl Reflect for MaterialRef {
    const SCHEMA: TypeSchema = TypeSchema::new("MaterialRef", &[]);

    fn reflect_write(&self, writer: &mut BinaryWriter) {
        self.0.reflect_write(writer);
    }

    fn reflect_read(reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
        u64::reflect_read(reader).map(MaterialRef)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_is_zero() {
        assert!(!MaterialRef::INVALID.is_valid());
    }

    #[test]
    fn non_zero_is_valid_and_stable() {
        let a = MaterialRef::from_raw(99);
        let b = MaterialRef::from_raw(99);
        assert!(a.is_valid());
        assert_eq!(a, b);
    }

    #[test]
    fn ordering_is_numeric() {
        assert!(MaterialRef::from_raw(1) < MaterialRef::from_raw(2));
    }

    #[test]
    fn reflect_round_trips_and_rejects_truncation() {
        let m = MaterialRef::from_raw(99);
        let mut w = BinaryWriter::new();
        m.reflect_write(&mut w);
        assert_eq!(
            MaterialRef::reflect_read(&mut BinaryReader::new(&w.into_bytes())).unwrap(),
            m
        );
        assert!(MaterialRef::reflect_read(&mut BinaryReader::new(&[])).is_err());
        assert_eq!(<MaterialRef as Reflect>::SCHEMA.name(), "MaterialRef");
    }
}
