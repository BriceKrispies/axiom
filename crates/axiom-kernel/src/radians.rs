//! `Radians` — a finite angle, in radians.

use crate::binary_reader::BinaryReader;
use crate::binary_writer::BinaryWriter;
use crate::error::KernelError;
use crate::error_code::KernelErrorCode;
use crate::error_scope::KernelErrorScope;
use crate::reflect::Reflect;
use crate::result::KernelResult;
use crate::type_schema::TypeSchema;

/// An angle in radians.
///
/// A kernel quantity primitive: a public API that takes `Radians` can no longer
/// be handed a length, a duration, or a degrees value by mistake — the unit
/// (radians, not degrees) is part of the type. The inner scalar is always finite
/// — [`Radians::new`] is the only constructor and it rejects NaN / infinity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Radians(f32);

impl Radians {
    /// Construct an angle, rejecting non-finite scalars (NaN / ±infinity).
    pub const fn new(value: f32) -> KernelResult<Self> {
        [
            Err(KernelError::new(
                KernelErrorScope::Scalar,
                KernelErrorCode::NonFiniteScalar,
                "Radians must be finite",
            )),
            Ok(Radians(value)),
        ][value.is_finite() as usize]
    }

    /// The underlying scalar value, in radians.
    pub const fn get(self) -> f32 {
        self.0
    }
}

impl Reflect for Radians {
    const SCHEMA: TypeSchema = TypeSchema::new("Radians", &[]);

    fn reflect_write(&self, writer: &mut BinaryWriter) {
        self.0.reflect_write(writer);
    }

    /// Read an angle, re-validating finiteness (a non-finite scalar in the byte
    /// stream is rejected exactly as [`Radians::new`] would).
    fn reflect_read(reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
        f32::reflect_read(reader).and_then(Radians::new)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_accepts_finite() {
        assert_eq!(Radians::new(1.25).unwrap().get(), 1.25);
    }

    #[test]
    fn new_rejects_nan() {
        let e = Radians::new(f32::NAN).unwrap_err();
        assert_eq!(e.scope(), KernelErrorScope::Scalar);
        assert_eq!(e.code(), KernelErrorCode::NonFiniteScalar);
    }

    #[test]
    fn new_rejects_infinity() {
        assert_eq!(
            Radians::new(f32::NEG_INFINITY).unwrap_err().code(),
            KernelErrorCode::NonFiniteScalar
        );
    }

    #[test]
    fn reflect_round_trips_rejects_truncation_and_nonfinite() {
        let r = Radians::new(1.25).unwrap();
        let mut w = BinaryWriter::new();
        r.reflect_write(&mut w);
        let bytes = w.into_bytes();
        assert_eq!(
            Radians::reflect_read(&mut BinaryReader::new(&bytes)).unwrap(),
            r
        );
        assert!(Radians::reflect_read(&mut BinaryReader::new(&[])).is_err());
        let mut bad = BinaryWriter::new();
        bad.write_f32(f32::NEG_INFINITY);
        assert!(Radians::reflect_read(&mut BinaryReader::new(&bad.into_bytes())).is_err());
        assert_eq!(<Radians as Reflect>::SCHEMA.name(), "Radians");
    }
}
