//! Three-component float vector.

use axiom_kernel::{BinaryReader, BinaryWriter, KernelResult};

use crate::approx_eq::ApproxEq;
use crate::epsilon::Epsilon;
use crate::math_error::MathError;
use crate::math_result::MathResult;

/// A deterministic three-component `f32` vector.
///
/// This is the workhorse vector of the engine: positions, directions,
/// translations, and the row of [`crate::Vec4`] that drops the homogeneous
/// component. Every fallible operation returns a
/// [`crate::math_result::MathResult`]; nothing in this type panics.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    /// `(0, 0, 0)`.
    pub const ZERO: Vec3 = Vec3 { x: 0.0, y: 0.0, z: 0.0 };
    /// `(1, 1, 1)`.
    pub const ONE: Vec3 = Vec3 { x: 1.0, y: 1.0, z: 1.0 };
    /// `(1, 0, 0)`.
    pub const UNIT_X: Vec3 = Vec3 { x: 1.0, y: 0.0, z: 0.0 };
    /// `(0, 1, 0)`.
    pub const UNIT_Y: Vec3 = Vec3 { x: 0.0, y: 1.0, z: 0.0 };
    /// `(0, 0, 1)`.
    pub const UNIT_Z: Vec3 = Vec3 { x: 0.0, y: 0.0, z: 1.0 };

    /// Component constructor.
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Vec3 { x, y, z }
    }

    /// Component-wise sum.
    pub const fn add(self, other: Vec3) -> Vec3 {
        Vec3::new(self.x + other.x, self.y + other.y, self.z + other.z)
    }

    /// Component-wise difference.
    pub const fn subtract(self, other: Vec3) -> Vec3 {
        Vec3::new(self.x - other.x, self.y - other.y, self.z - other.z)
    }

    /// Scale by a scalar.
    pub const fn mul_scalar(self, k: f32) -> Vec3 {
        Vec3::new(self.x * k, self.y * k, self.z * k)
    }

    /// Divide by a scalar, returning [`crate::math_error_code::MathErrorCode::DivideByZero`]
    /// if `k` is `0.0` and [`crate::math_error_code::MathErrorCode::NonFiniteScalar`]
    /// if `k` is not finite.
    pub fn div_scalar(self, k: f32) -> MathResult<Vec3> {
        if !k.is_finite() {
            return Err(MathError::non_finite_scalar(
                "vec3 scalar divisor must be finite",
            ));
        }
        if k == 0.0 {
            return Err(MathError::divide_by_zero(
                "vec3 scalar divisor was zero",
            ));
        }
        Ok(Vec3::new(self.x / k, self.y / k, self.z / k))
    }

    /// Dot product.
    pub const fn dot(self, other: Vec3) -> f32 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    /// Cross product. Right-handed: `unit_x × unit_y = unit_z`.
    pub const fn cross(self, other: Vec3) -> Vec3 {
        Vec3::new(
            self.y * other.z - self.z * other.y,
            self.z * other.x - self.x * other.z,
            self.x * other.y - self.y * other.x,
        )
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
    pub fn normalize(self) -> MathResult<Vec3> {
        let len = self.length();
        if len == 0.0 || !len.is_finite() {
            return Err(MathError::normalize_zero_length(
                "cannot normalize zero-length Vec3",
            ));
        }
        Ok(Vec3::new(self.x / len, self.y / len, self.z / len))
    }

    /// Euclidean distance between `self` and `other`.
    pub fn distance(self, other: Vec3) -> f32 {
        self.subtract(other).length()
    }

    /// Append the three `f32` components in declaration order.
    pub fn write_to(self, writer: &mut BinaryWriter) {
        writer.write_f32(self.x);
        writer.write_f32(self.y);
        writer.write_f32(self.z);
    }

    /// Read three `f32` components in declaration order.
    pub fn read_from(reader: &mut BinaryReader<'_>) -> KernelResult<Vec3> {
        let x = reader.read_f32()?;
        let y = reader.read_f32()?;
        let z = reader.read_f32()?;
        Ok(Vec3::new(x, y, z))
    }
}

impl ApproxEq for Vec3 {
    fn approx_eq(&self, other: &Self, epsilon: Epsilon) -> bool {
        self.x.approx_eq(&other.x, epsilon)
            && self.y.approx_eq(&other.y, epsilon)
            && self.z.approx_eq(&other.z, epsilon)
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
    fn basis_vectors_are_axis_aligned() {
        assert!(Vec3::UNIT_X.approx_eq(&Vec3::new(1.0, 0.0, 0.0), eps()));
        assert!(Vec3::UNIT_Y.approx_eq(&Vec3::new(0.0, 1.0, 0.0), eps()));
        assert!(Vec3::UNIT_Z.approx_eq(&Vec3::new(0.0, 0.0, 1.0), eps()));
        assert!(Vec3::ZERO.approx_eq(&Vec3::new(0.0, 0.0, 0.0), eps()));
        assert!(Vec3::ONE.approx_eq(&Vec3::new(1.0, 1.0, 1.0), eps()));
    }

    #[test]
    fn add_and_subtract_are_component_wise() {
        let a = Vec3::new(1.0, 2.0, 3.0);
        let b = Vec3::new(4.0, 5.0, 6.0);
        assert!(a.add(b).approx_eq(&Vec3::new(5.0, 7.0, 9.0), eps()));
        assert!(b.subtract(a).approx_eq(&Vec3::new(3.0, 3.0, 3.0), eps()));
    }

    #[test]
    fn scalar_multiply_and_divide_scale_components() {
        assert!(Vec3::new(2.0, -4.0, 6.0)
            .mul_scalar(0.5)
            .approx_eq(&Vec3::new(1.0, -2.0, 3.0), eps()));
        assert!(Vec3::new(2.0, -4.0, 6.0)
            .div_scalar(2.0)
            .unwrap()
            .approx_eq(&Vec3::new(1.0, -2.0, 3.0), eps()));
    }

    #[test]
    fn div_by_zero_is_rejected() {
        assert_eq!(
            Vec3::new(1.0, 0.0, 0.0).div_scalar(0.0).unwrap_err().code(),
            MathErrorCode::DivideByZero
        );
    }

    #[test]
    fn div_by_non_finite_is_rejected() {
        assert_eq!(
            Vec3::new(1.0, 0.0, 0.0)
                .div_scalar(f32::NAN)
                .unwrap_err()
                .code(),
            MathErrorCode::NonFiniteScalar
        );
    }

    #[test]
    fn dot_matches_orthogonality() {
        assert_eq!(Vec3::UNIT_X.dot(Vec3::UNIT_Y), 0.0);
        assert_eq!(Vec3::UNIT_X.dot(Vec3::UNIT_X), 1.0);
        assert_eq!(Vec3::new(1.0, 2.0, 3.0).dot(Vec3::new(4.0, -5.0, 6.0)), 12.0);
    }

    #[test]
    fn cross_is_right_handed() {
        assert!(Vec3::UNIT_X
            .cross(Vec3::UNIT_Y)
            .approx_eq(&Vec3::UNIT_Z, eps()));
        assert!(Vec3::UNIT_Y
            .cross(Vec3::UNIT_Z)
            .approx_eq(&Vec3::UNIT_X, eps()));
        assert!(Vec3::UNIT_Z
            .cross(Vec3::UNIT_X)
            .approx_eq(&Vec3::UNIT_Y, eps()));
    }

    #[test]
    fn cross_is_antisymmetric() {
        let a = Vec3::new(1.0, 2.0, 3.0);
        let b = Vec3::new(-2.0, 5.0, 0.5);
        assert!(a
            .cross(b)
            .add(b.cross(a))
            .approx_eq(&Vec3::ZERO, eps()));
    }

    #[test]
    fn length_and_length_squared_match() {
        let v = Vec3::new(2.0, 3.0, 6.0);
        assert_eq!(v.length_squared(), 49.0);
        assert_eq!(v.length(), 7.0);
    }

    #[test]
    fn normalize_produces_unit_vector() {
        let n = Vec3::new(0.0, 0.0, 5.0).normalize().unwrap();
        assert!(n.approx_eq(&Vec3::UNIT_Z, eps()));
        assert!((n.length() - 1.0).abs() <= eps().value());
    }

    #[test]
    fn normalize_zero_fails() {
        assert_eq!(
            Vec3::ZERO.normalize().unwrap_err().code(),
            MathErrorCode::NormalizeZeroLength
        );
    }

    #[test]
    fn distance_is_symmetric() {
        let a = Vec3::new(1.0, 2.0, 2.0);
        let b = Vec3::new(4.0, 6.0, 14.0);
        assert_eq!(a.distance(b), 13.0);
        assert_eq!(b.distance(a), 13.0);
    }

    #[test]
    fn approx_eq_rejects_nan_components() {
        let nan = Vec3::new(f32::NAN, 0.0, 0.0);
        assert!(!nan.approx_eq(&Vec3::ZERO, eps()));
    }

    #[test]
    fn binary_round_trip_preserves_components() {
        let api = KernelApi::new();
        let v = Vec3::new(1.5, -2.25, 3.125);

        let mut writer = api.binary_writer();
        v.write_to(&mut writer);
        let bytes = writer.into_bytes();

        let mut reader = api.binary_reader(&bytes);
        let back = Vec3::read_from(&mut reader).unwrap();
        assert!(back.approx_eq(&v, eps()));
    }
}

#[cfg(test)]
mod cov {
    use super::*;
    use axiom_kernel::BinaryReader;

    #[test]
    fn normalize_non_finite_length_fails() {
        assert!(Vec3::new(f32::MAX, f32::MAX, f32::MAX).normalize().is_err());
    }

    #[test]
    fn read_from_truncated_each_component() {
        assert!(Vec3::read_from(&mut BinaryReader::new(&[])).is_err());
        assert!(Vec3::read_from(&mut BinaryReader::new(&[0u8; 4])).is_err());
        assert!(Vec3::read_from(&mut BinaryReader::new(&[0u8; 8])).is_err());
    }
}
