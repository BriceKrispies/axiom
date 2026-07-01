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

    /// Construct a length from a *computed* scalar, mapping any non-finite result
    /// (NaN / ±infinity) to `0.0` so the constructor is **total** — it never
    /// fails. This is the sanctioned path for values produced by arithmetic
    /// (interpolation between two endpoints, a deterministic in-range pick, a
    /// scale) where a fallible [`Meters::new`] would leave an unreachable error
    /// arm: the inputs are already finite, so the sanitizing branch exists only as
    /// a defined fallback. Mirrors [`crate::Ratio::finite_or_zero`].
    pub const fn finite_or_zero(value: f32) -> Self {
        Meters([0.0, value][value.is_finite() as usize])
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
    fn finite_or_zero_passes_finite_and_sanitizes_nonfinite() {
        assert_eq!(Meters::finite_or_zero(2.5).get(), 2.5);
        assert_eq!(Meters::finite_or_zero(-0.25).get(), -0.25);
        assert_eq!(Meters::finite_or_zero(0.0).get(), 0.0);
        assert_eq!(Meters::finite_or_zero(f32::NAN).get(), 0.0);
        assert_eq!(Meters::finite_or_zero(f32::INFINITY).get(), 0.0);
        assert_eq!(Meters::finite_or_zero(f32::NEG_INFINITY).get(), 0.0);
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
