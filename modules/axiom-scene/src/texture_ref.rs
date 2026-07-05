//! Opaque reference to a texture resource owned outside the scene module.

use axiom_kernel::{BinaryReader, BinaryWriter, KernelResult, Reflect, TypeSchema};

/// An opaque, stable reference to a texture (albedo) resource.
///
/// `axiom-scene` does not own textures; a [`TextureRef`] is just a `u64`
/// identity that a future resource/render module (or app composition layer)
/// resolves to actual pixel data. It shares the same numeric identity space as
/// the material/mesh refs (an app assigns each resource a stable `u64` and uses
/// that `u64` verbatim as the scene ref). `INVALID` (`0`) means "no texture" —
/// an object binds a texture only when its ref is valid, so an untextured
/// renderable is the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TextureRef(u64);

impl TextureRef {
    pub const INVALID: TextureRef = TextureRef(0);

    pub const fn from_raw(raw: u64) -> Self {
        TextureRef(raw)
    }

    pub const fn raw(self) -> u64 {
        self.0
    }

    pub const fn is_valid(self) -> bool {
        self.0 != 0
    }
}

impl Reflect for TextureRef {
    const SCHEMA: TypeSchema = TypeSchema::new("TextureRef", &[]);

    fn reflect_write(&self, writer: &mut BinaryWriter) {
        self.0.reflect_write(writer);
    }

    fn reflect_read(reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
        u64::reflect_read(reader).map(TextureRef)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_is_zero() {
        assert!(!TextureRef::INVALID.is_valid());
    }

    #[test]
    fn non_zero_is_valid_and_stable() {
        let a = TextureRef::from_raw(42);
        let b = TextureRef::from_raw(42);
        assert!(a.is_valid());
        assert_eq!(a, b);
    }

    #[test]
    fn ordering_is_numeric() {
        assert!(TextureRef::from_raw(1) < TextureRef::from_raw(2));
    }

    #[test]
    fn reflect_round_trips_and_rejects_truncation() {
        let t = TextureRef::from_raw(42);
        let mut w = BinaryWriter::new();
        t.reflect_write(&mut w);
        assert_eq!(
            TextureRef::reflect_read(&mut BinaryReader::new(&w.into_bytes())).unwrap(),
            t
        );
        assert!(TextureRef::reflect_read(&mut BinaryReader::new(&[])).is_err());
        assert_eq!(<TextureRef as Reflect>::SCHEMA.name(), "TextureRef");
    }
}
