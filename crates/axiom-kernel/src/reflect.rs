//! Reflection + serialization: a type describes its shape and round-trips its
//! values through the binary primitives.

use crate::binary_reader::BinaryReader;
use crate::binary_writer::BinaryWriter;
use crate::entity_id::EntityId;
use crate::result::KernelResult;
use crate::type_schema::TypeSchema;

/// A type that can describe its own shape and serialize/deserialize its values.
///
/// This formalizes the engine's hand-rolled `write_to`/`read_from` idiom into a
/// composable contract: leaf types (scalars) reflect themselves directly;
/// composites compose their fields' reflects (and their [`TypeSchema`] lists
/// those fields). It is a generic bound, not a trait object — `reflect_read`
/// returns `Self` and `SCHEMA` is an associated const — which is what keeps it
/// allocation-free, deterministic, and free of runtime type erasure.
pub trait Reflect: Sized {
    /// A static, flat description of this type's shape.
    const SCHEMA: TypeSchema;

    /// Append this value to the writer.
    fn reflect_write(&self, writer: &mut BinaryWriter);

    /// Read a value previously written with [`Self::reflect_write`].
    fn reflect_read(reader: &mut BinaryReader<'_>) -> KernelResult<Self>;
}

macro_rules! impl_reflect_scalar {
    ($ty:ty, $name:literal, $write:ident, $read:ident) => {
        impl Reflect for $ty {
            const SCHEMA: TypeSchema = TypeSchema::new($name, &[]);

            fn reflect_write(&self, writer: &mut BinaryWriter) {
                writer.$write(*self);
            }

            fn reflect_read(reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
                reader.$read()
            }
        }
    };
}

impl_reflect_scalar!(u8, "u8", write_u8, read_u8);
impl_reflect_scalar!(u32, "u32", write_u32, read_u32);
impl_reflect_scalar!(u64, "u64", write_u64, read_u64);
impl_reflect_scalar!(i32, "i32", write_i32, read_i32);
impl_reflect_scalar!(i64, "i64", write_i64, read_i64);
impl_reflect_scalar!(f32, "f32", write_f32, read_f32);
impl_reflect_scalar!(bool, "bool", write_bool, read_bool);

impl Reflect for EntityId {
    const SCHEMA: TypeSchema = TypeSchema::new("EntityId", &[]);

    fn reflect_write(&self, writer: &mut BinaryWriter) {
        writer.write_u64(self.raw());
    }

    fn reflect_read(reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
        reader.read_u64().map(EntityId::from_raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error_code::KernelErrorCode;
    use crate::error_scope::KernelErrorScope;

    fn round_trip<T: Reflect + PartialEq + std::fmt::Debug>(value: T) {
        let mut w = BinaryWriter::new();
        value.reflect_write(&mut w);
        let bytes = w.into_bytes();
        let mut r = BinaryReader::new(&bytes);
        assert_eq!(T::reflect_read(&mut r).unwrap(), value);
    }

    #[test]
    fn scalars_and_entity_id_round_trip() {
        round_trip(0xABu8);
        round_trip(0xABCD_1234u32);
        round_trip(0x0102_0304_0506_0708u64);
        round_trip(-12_345_i32);
        round_trip(-9_000_000_000_i64);
        round_trip(2.5f32);
        round_trip(true);
        round_trip(false);
        round_trip(EntityId::from_raw(42));
    }

    #[test]
    fn reflect_read_propagates_truncation() {
        assert!(u8::reflect_read(&mut BinaryReader::new(&[])).is_err());
        assert!(u32::reflect_read(&mut BinaryReader::new(&[])).is_err());
        assert!(u64::reflect_read(&mut BinaryReader::new(&[])).is_err());
        assert!(i32::reflect_read(&mut BinaryReader::new(&[])).is_err());
        assert!(i64::reflect_read(&mut BinaryReader::new(&[])).is_err());
        assert!(f32::reflect_read(&mut BinaryReader::new(&[])).is_err());
        assert!(bool::reflect_read(&mut BinaryReader::new(&[])).is_err());
        assert!(EntityId::reflect_read(&mut BinaryReader::new(&[])).is_err());
    }

    #[test]
    fn scalar_schemas_are_named_leaves() {
        assert_eq!(<f32 as Reflect>::SCHEMA.name(), "f32");
        assert!(<f32 as Reflect>::SCHEMA.fields().is_empty());
        assert_eq!(<u8 as Reflect>::SCHEMA.name(), "u8");
        assert_eq!(<i32 as Reflect>::SCHEMA.name(), "i32");
        assert_eq!(<i64 as Reflect>::SCHEMA.name(), "i64");
        assert_eq!(<EntityId as Reflect>::SCHEMA.name(), "EntityId");
    }

    #[derive(Debug, PartialEq)]
    enum Sample {
        Nothing,
        Number(i64),
    }

    impl Sample {
        fn write(&self, w: &mut BinaryWriter) {
            let tag = [0u8, 1][usize::from(matches!(self, Sample::Number(_)))];
            w.write_u8(tag);
            if let Sample::Number(n) = self {
                n.reflect_write(w);
            }
        }

        fn read(r: &mut BinaryReader<'_>) -> KernelResult<Self> {
            r.read_tagged(&[
                |_r| Ok(Sample::Nothing),
                |r| i64::reflect_read(r).map(Sample::Number),
            ])
        }
    }

    #[test]
    fn read_tagged_round_trips_each_variant() {
        for value in [Sample::Nothing, Sample::Number(-77)] {
            let mut w = BinaryWriter::new();
            value.write(&mut w);
            let bytes = w.into_bytes();
            assert_eq!(Sample::read(&mut BinaryReader::new(&bytes)).unwrap(), value);
        }
    }

    #[test]
    fn read_tagged_rejects_an_out_of_range_tag() {
        let mut w = BinaryWriter::new();
        w.write_u8(2);
        let err = Sample::read(&mut BinaryReader::new(&w.into_bytes())).unwrap_err();
        assert_eq!(err.scope(), KernelErrorScope::Binary);
        assert_eq!(err.code(), KernelErrorCode::InvalidDiscriminant);
    }

    #[test]
    fn read_tagged_propagates_truncation() {
        assert!(Sample::read(&mut BinaryReader::new(&[])).is_err());
        assert!(Sample::read(&mut BinaryReader::new(&[1])).is_err());
    }
}
