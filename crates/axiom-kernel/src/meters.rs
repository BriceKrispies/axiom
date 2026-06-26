//! `Meters` — a finite length, in metres.

use crate::binary_reader::BinaryReader;
use crate::binary_writer::BinaryWriter;
use crate::error::KernelError;
use crate::error_code::KernelErrorCode;
use crate::error_scope::KernelErrorScope;
use crate::reflect::Reflect;
use crate::result::KernelResult;
use crate::type_schema::TypeSchema;

/// A length in metres.
///
/// A kernel quantity primitive: the typed boundary where a raw `f32` becomes a
/// dimensioned length, so layers above stop passing naked floats whose unit a
/// caller has to guess. The inner scalar is always finite — [`Meters::new`] is
/// the only constructor and it rejects NaN / infinity.
///
/// (Metres are Axiom's world-space length unit. If a future decision makes the
/// world unit configurable, this is the one type to rename.)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Meters(f32);

impl Meters {
    /// Construct a length, rejecting non-finite scalars (NaN / ±infinity).
    pub const fn new(value: f32) -> KernelResult<Self> {
        [
            Err(KernelError::new(
                KernelErrorScope::Scalar,
                KernelErrorCode::NonFiniteScalar,
                "Meters must be finite",
            )),
            Ok(Meters(value)),
        ][value.is_finite() as usize]
    }

    /// The underlying scalar value, in metres.
    pub const fn get(self) -> f32 {
        self.0
    }
}

impl Reflect for Meters {
    const SCHEMA: TypeSchema = TypeSchema::new("Meters", &[]);

    fn reflect_write(&self, writer: &mut BinaryWriter) {
        self.0.reflect_write(writer);
    }

    /// Read a length, re-validating finiteness (a non-finite scalar in the byte
    /// stream is rejected exactly as [`Meters::new`] would).
    fn reflect_read(reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
        f32::reflect_read(reader).and_then(Meters::new)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_accepts_finite() {
        assert_eq!(Meters::new(2.5).unwrap().get(), 2.5);
    }

    #[test]
    fn new_rejects_nan() {
        let e = Meters::new(f32::NAN).unwrap_err();
        assert_eq!(e.scope(), KernelErrorScope::Scalar);
        assert_eq!(e.code(), KernelErrorCode::NonFiniteScalar);
    }

    #[test]
    fn new_rejects_infinity() {
        assert_eq!(
            Meters::new(f32::INFINITY).unwrap_err().code(),
            KernelErrorCode::NonFiniteScalar
        );
    }

    #[test]
    fn reflect_round_trips_rejects_truncation_and_nonfinite() {
        let m = Meters::new(2.5).unwrap();
        let mut w = BinaryWriter::new();
        m.reflect_write(&mut w);
        let bytes = w.into_bytes();
        assert_eq!(
            Meters::reflect_read(&mut BinaryReader::new(&bytes)).unwrap(),
            m
        );
        assert!(Meters::reflect_read(&mut BinaryReader::new(&[])).is_err());
        let mut bad = BinaryWriter::new();
        bad.write_f32(f32::INFINITY);
        assert!(Meters::reflect_read(&mut BinaryReader::new(&bad.into_bytes())).is_err());
        assert_eq!(<Meters as Reflect>::SCHEMA.name(), "Meters");
    }
}
