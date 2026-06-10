//! Two-component float vector.

use axiom_kernel::{BinaryReader, BinaryWriter, FieldSchema, KernelResult, Reflect, TypeSchema};

use crate::approx_eq::ApproxEq;
use crate::epsilon::Epsilon;
use crate::math_error::MathError;
use crate::math_result::MathResult;

/// A deterministic two-component `f32` vector.
///
/// Component-wise IEEE-754 arithmetic. Every fallible operation
/// ([`Vec2::div_scalar`], [`Vec2::normalize`]) routes through
/// [`crate::math_result::MathResult`]; nothing in this type panics.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    /// `(0, 0)`.
    pub const ZERO: Vec2 = Vec2 { x: 0.0, y: 0.0 };
    /// `(1, 1)`.
    pub const ONE: Vec2 = Vec2 { x: 1.0, y: 1.0 };

    /// Component constructor.
    pub const fn new(x: f32, y: f32) -> Self {
        Vec2 { x, y }
    }

    /// Component-wise sum.
    pub const fn add(self, other: Vec2) -> Vec2 {
        Vec2::new(self.x + other.x, self.y + other.y)
    }

    /// Component-wise difference.
    pub const fn subtract(self, other: Vec2) -> Vec2 {
        Vec2::new(self.x - other.x, self.y - other.y)
    }

    /// Scale by a scalar.
    pub const fn mul_scalar(self, k: f32) -> Vec2 {
        Vec2::new(self.x * k, self.y * k)
    }

    /// Divide by a scalar, returning [`crate::math_error_code::MathErrorCode::DivideByZero`]
    /// if `k` is `0.0` and [`crate::math_error_code::MathErrorCode::NonFiniteScalar`]
    /// if `k` is not finite.
    pub fn div_scalar(self, k: f32) -> MathResult<Vec2> {
        if !k.is_finite() {
            return Err(MathError::non_finite_scalar(
                "vec2 scalar divisor must be finite",
            ));
        }
        if k == 0.0 {
            return Err(MathError::divide_by_zero("vec2 scalar divisor was zero"));
        }
        Ok(Vec2::new(self.x / k, self.y / k))
    }

    /// Dot product.
    pub const fn dot(self, other: Vec2) -> f32 {
        self.x * other.x + self.y * other.y
    }

    /// Squared length.
    pub const fn length_squared(self) -> f32 {
        self.dot(self)
    }

    /// Euclidean length.
    pub fn length(self) -> f32 {
        self.length_squared().sqrt()
    }

    /// Unit-length copy. Fails with
    /// [`crate::math_error_code::MathErrorCode::NormalizeZeroLength`] for the
    /// zero vector.
    pub fn normalize(self) -> MathResult<Vec2> {
        let len = self.length();
        if len == 0.0 || !len.is_finite() {
            return Err(MathError::normalize_zero_length(
                "cannot normalize zero-length Vec2",
            ));
        }
        Ok(Vec2::new(self.x / len, self.y / len))
    }

    /// Euclidean distance between `self` and `other`.
    pub fn distance(self, other: Vec2) -> f32 {
        self.subtract(other).length()
    }

    /// Append the two `f32` components in declaration order.
    pub fn write_to(self, writer: &mut BinaryWriter) {
        writer.write_f32(self.x);
        writer.write_f32(self.y);
    }

    /// Read two `f32` components in declaration order.
    pub fn read_from(reader: &mut BinaryReader<'_>) -> KernelResult<Vec2> {
        let x = reader.read_f32()?;
        let y = reader.read_f32()?;
        Ok(Vec2::new(x, y))
    }
}

impl ApproxEq for Vec2 {
    fn approx_eq(&self, other: &Self, epsilon: Epsilon) -> bool {
        self.x.approx_eq(&other.x, epsilon) && self.y.approx_eq(&other.y, epsilon)
    }
}

impl Reflect for Vec2 {
    const SCHEMA: TypeSchema = TypeSchema::new(
        "Vec2",
        &[FieldSchema::new("x", "f32"), FieldSchema::new("y", "f32")],
    );

    fn reflect_write(&self, writer: &mut BinaryWriter) {
        self.x.reflect_write(writer);
        self.y.reflect_write(writer);
    }

    fn reflect_read(reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
        Ok(Vec2::new(
            f32::reflect_read(reader)?,
            f32::reflect_read(reader)?,
        ))
    }
}

#[cfg(test)]
mod reflect_tests {
    use super::*;

    #[test]
    fn reflect_round_trips_describes_and_rejects_truncation() {
        let v = Vec2::new(1.5, -2.0);
        let mut w = BinaryWriter::new();
        v.reflect_write(&mut w);
        let bytes = w.into_bytes();
        assert_eq!(
            Vec2::reflect_read(&mut BinaryReader::new(&bytes)).unwrap(),
            v
        );
        for len in 0..bytes.len() {
            assert!(Vec2::reflect_read(&mut BinaryReader::new(&bytes[..len])).is_err());
        }
        assert_eq!(<Vec2 as Reflect>::SCHEMA.name(), "Vec2");
        assert_eq!(<Vec2 as Reflect>::SCHEMA.fields().len(), 2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math_error_code::MathErrorCode;
    use axiom_kernel::KernelApi;

    fn eps() -> Epsilon {
        Epsilon::DEFAULT
    }

    #[test]
    fn constants_match_documentation() {
        assert!(Vec2::ZERO.approx_eq(&Vec2::new(0.0, 0.0), eps()));
        assert!(Vec2::ONE.approx_eq(&Vec2::new(1.0, 1.0), eps()));
    }

    #[test]
    fn add_is_component_wise() {
        let r = Vec2::new(1.0, 2.0).add(Vec2::new(3.0, 4.0));
        assert!(r.approx_eq(&Vec2::new(4.0, 6.0), eps()));
    }

    #[test]
    fn subtract_is_component_wise() {
        let r = Vec2::new(5.0, 7.0).subtract(Vec2::new(2.0, 3.0));
        assert!(r.approx_eq(&Vec2::new(3.0, 4.0), eps()));
    }

    #[test]
    fn mul_scalar_scales_each_component() {
        let r = Vec2::new(1.0, -2.0).mul_scalar(0.5);
        assert!(r.approx_eq(&Vec2::new(0.5, -1.0), eps()));
    }

    #[test]
    fn div_scalar_scales_each_component() {
        let r = Vec2::new(2.0, -4.0).div_scalar(2.0).unwrap();
        assert!(r.approx_eq(&Vec2::new(1.0, -2.0), eps()));
    }

    #[test]
    fn div_by_zero_is_rejected() {
        let err = Vec2::new(1.0, 1.0).div_scalar(0.0).unwrap_err();
        assert_eq!(err.code(), MathErrorCode::DivideByZero);
    }

    #[test]
    fn div_by_non_finite_is_rejected() {
        let err = Vec2::new(1.0, 1.0).div_scalar(f32::NAN).unwrap_err();
        assert_eq!(err.code(), MathErrorCode::NonFiniteScalar);
    }

    #[test]
    fn dot_matches_geometry() {
        assert_eq!(Vec2::new(1.0, 0.0).dot(Vec2::new(0.0, 1.0)), 0.0);
        assert_eq!(Vec2::new(2.0, 3.0).dot(Vec2::new(4.0, 5.0)), 23.0);
    }

    #[test]
    fn length_and_length_squared_agree() {
        let v = Vec2::new(3.0, 4.0);
        assert_eq!(v.length_squared(), 25.0);
        assert_eq!(v.length(), 5.0);
    }

    #[test]
    fn normalize_produces_unit_length() {
        let n = Vec2::new(0.0, 7.0).normalize().unwrap();
        assert!(n.approx_eq(&Vec2::new(0.0, 1.0), eps()));
        assert!((n.length() - 1.0).abs() <= eps().value());
    }

    #[test]
    fn normalize_zero_fails() {
        let err = Vec2::ZERO.normalize().unwrap_err();
        assert_eq!(err.code(), MathErrorCode::NormalizeZeroLength);
    }

    #[test]
    fn distance_is_symmetric_and_positive() {
        let a = Vec2::new(1.0, 2.0);
        let b = Vec2::new(4.0, 6.0);
        assert_eq!(a.distance(b), 5.0);
        assert_eq!(b.distance(a), 5.0);
    }

    #[test]
    fn approx_eq_rejects_nan_components() {
        let nan = Vec2::new(f32::NAN, 0.0);
        assert!(!nan.approx_eq(&Vec2::ZERO, eps()));
    }

    #[test]
    fn binary_round_trip_preserves_components() {
        let api = KernelApi::new();
        let v = Vec2::new(1.5, -2.25);

        let mut writer = api.binary_writer();
        v.write_to(&mut writer);
        let bytes = writer.into_bytes();

        let mut reader = api.binary_reader(&bytes);
        let back = Vec2::read_from(&mut reader).unwrap();
        assert!(back.approx_eq(&v, eps()));
    }
}

#[cfg(test)]
mod cov {
    use super::*;
    use axiom_kernel::BinaryReader;

    #[test]
    fn normalize_non_finite_length_fails() {
        assert!(Vec2::new(f32::MAX, f32::MAX).normalize().is_err());
    }

    #[test]
    fn read_from_truncated_each_component() {
        assert!(Vec2::read_from(&mut BinaryReader::new(&[])).is_err());
        assert!(Vec2::read_from(&mut BinaryReader::new(&[0u8; 4])).is_err());
    }

    // Kills normalize divide mutants at 89 (`self.x / len`, `self.y / len` ->
    // `% len` / `* len`). v = (3,4) has length 5, normalizing to exactly
    // (0.6, 0.8). Both components are nonzero so each divide is observable.
    #[test]
    fn normalize_divides_each_component_by_length() {
        let n = Vec2::new(3.0, 4.0).normalize().unwrap();
        assert!((n.x - 0.6).abs() < 1.0e-7);
        assert!((n.y - 0.8).abs() < 1.0e-7);
    }
}
