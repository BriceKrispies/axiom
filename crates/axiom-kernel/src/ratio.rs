//! `Ratio` — a finite, dimensionless ratio.

use crate::binary_reader::BinaryReader;
use crate::binary_writer::BinaryWriter;
use crate::error::KernelError;
use crate::error_code::KernelErrorCode;
use crate::error_scope::KernelErrorScope;
use crate::reflect::Reflect;
use crate::result::KernelResult;
use crate::type_schema::TypeSchema;

/// A dimensionless ratio.
///
/// A kernel quantity primitive for the genuinely *unitless* scalars that are
/// nonetheless not arbitrary floats — an aspect ratio (width / height), a DPI
/// scale factor, a normalized fraction. Typing them as `Ratio` says "this is a
/// ratio, not some unknown number," which is the point: it stops a bare `f32`
/// from standing in for a quantity whose meaning the caller would have to guess.
/// The inner scalar is always finite — [`Ratio::new`] is the only constructor
/// and it rejects NaN / infinity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ratio(f32);

impl Ratio {
    /// Construct a ratio, rejecting non-finite scalars (NaN / ±infinity).
    pub const fn new(value: f32) -> KernelResult<Self> {
        [
            Err(KernelError::new(
                KernelErrorScope::Scalar,
                KernelErrorCode::NonFiniteScalar,
                "Ratio must be finite",
            )),
            Ok(Ratio(value)),
        ][value.is_finite() as usize]
    }

    /// Construct a ratio from a *computed* scalar, mapping any non-finite result
    /// (NaN / ±infinity) to `0.0` so the constructor is **total** — it never
    /// fails. This is the sanctioned path for values produced by arithmetic
    /// (interpolation, normalized progress, fades) where a fallible [`Ratio::new`]
    /// would leave an unreachable error arm: the inputs are already finite, so the
    /// sanitizing branch exists only as a defined fallback. Finite values
    /// (including HDR magnitudes above `1.0`) pass through unchanged; only the
    /// genuinely non-finite are clamped to a finite zero.
    pub const fn finite_or_zero(value: f32) -> Self {
        Ratio([0.0, value][value.is_finite() as usize])
    }

    /// The underlying dimensionless value.
    pub const fn get(self) -> f32 {
        self.0
    }
}

impl Reflect for Ratio {
    const SCHEMA: TypeSchema = TypeSchema::new("Ratio", &[]);

    fn reflect_write(&self, writer: &mut BinaryWriter) {
        self.0.reflect_write(writer);
    }

    /// Read a ratio, re-validating finiteness (a non-finite scalar in the byte
    /// stream is rejected exactly as [`Ratio::new`] would).
    fn reflect_read(reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
        f32::reflect_read(reader).and_then(Ratio::new)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_accepts_finite() {
        assert_eq!(Ratio::new(1.7777).unwrap().get(), 1.7777);
    }

    #[test]
    fn finite_or_zero_passes_finite_and_sanitizes_nonfinite() {
        // Finite values (including HDR > 1.0 and negatives) pass through.
        assert_eq!(Ratio::finite_or_zero(0.5).get(), 0.5);
        assert_eq!(Ratio::finite_or_zero(2.5).get(), 2.5);
        assert_eq!(Ratio::finite_or_zero(-0.25).get(), -0.25);
        // Non-finite scalars collapse to a finite zero (both the NaN and the
        // infinity arms select the fallback).
        assert_eq!(Ratio::finite_or_zero(f32::NAN).get(), 0.0);
        assert_eq!(Ratio::finite_or_zero(f32::INFINITY).get(), 0.0);
        assert_eq!(Ratio::finite_or_zero(f32::NEG_INFINITY).get(), 0.0);
    }

    #[test]
    fn new_rejects_nan() {
        let e = Ratio::new(f32::NAN).unwrap_err();
        assert_eq!(e.scope(), KernelErrorScope::Scalar);
        assert_eq!(e.code(), KernelErrorCode::NonFiniteScalar);
    }

    #[test]
    fn new_rejects_infinity() {
        assert_eq!(
            Ratio::new(f32::INFINITY).unwrap_err().code(),
            KernelErrorCode::NonFiniteScalar
        );
    }

    #[test]
    fn reflect_round_trips_rejects_truncation_and_nonfinite() {
        let r = Ratio::new(1.7777).unwrap();
        let mut w = BinaryWriter::new();
        r.reflect_write(&mut w);
        let bytes = w.into_bytes();
        assert_eq!(
            Ratio::reflect_read(&mut BinaryReader::new(&bytes)).unwrap(),
            r
        );
        // A truncated stream errs rather than panicking.
        assert!(Ratio::reflect_read(&mut BinaryReader::new(&[])).is_err());
        // A non-finite scalar in the stream is rejected on read.
        let mut bad = BinaryWriter::new();
        bad.write_f32(f32::INFINITY);
        assert!(Ratio::reflect_read(&mut BinaryReader::new(&bad.into_bytes())).is_err());
        assert_eq!(<Ratio as Reflect>::SCHEMA.name(), "Ratio");
    }
}
