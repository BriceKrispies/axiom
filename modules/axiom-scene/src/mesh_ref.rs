//! Opaque reference to a mesh resource owned outside the scene module.

use axiom_kernel::{BinaryReader, BinaryWriter, KernelResult, Reflect, TypeSchema};

/// An opaque, stable reference to a mesh resource.
///
/// `axiom-scene` does not own meshes; a [`MeshRef`] is just a `u64`
/// identity that a future resource/render module (or app composition
/// layer) will resolve to actual GPU mesh data. The scene module makes
/// no assumption about mesh storage or format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MeshRef(u64);

impl MeshRef {
    pub const INVALID: MeshRef = MeshRef(0);

    pub const fn from_raw(raw: u64) -> Self {
        MeshRef(raw)
    }

    pub const fn raw(self) -> u64 {
        self.0
    }

    pub const fn is_valid(self) -> bool {
        self.0 != 0
    }
}

impl Reflect for MeshRef {
    const SCHEMA: TypeSchema = TypeSchema::new("MeshRef", &[]);

    fn reflect_write(&self, writer: &mut BinaryWriter) {
        self.0.reflect_write(writer);
    }

    fn reflect_read(reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
        u64::reflect_read(reader).map(MeshRef)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_is_zero() {
        assert!(!MeshRef::INVALID.is_valid());
    }

    #[test]
    fn non_zero_is_valid_and_stable() {
        let a = MeshRef::from_raw(11);
        let b = MeshRef::from_raw(11);
        assert!(a.is_valid());
        assert_eq!(a, b);
    }

    #[test]
    fn ordering_is_numeric() {
        assert!(MeshRef::from_raw(1) < MeshRef::from_raw(2));
    }

    #[test]
    fn reflect_round_trips_and_rejects_truncation() {
        let m = MeshRef::from_raw(11);
        let mut w = BinaryWriter::new();
        m.reflect_write(&mut w);
        assert_eq!(
            MeshRef::reflect_read(&mut BinaryReader::new(&w.into_bytes())).unwrap(),
            m
        );
        assert!(MeshRef::reflect_read(&mut BinaryReader::new(&[])).is_err());
        assert_eq!(<MeshRef as Reflect>::SCHEMA.name(), "MeshRef");
    }
}
