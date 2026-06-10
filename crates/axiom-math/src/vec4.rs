//! Four-component float vector.

use axiom_kernel::{BinaryReader, BinaryWriter, FieldSchema, KernelResult, Reflect, TypeSchema};

use crate::approx_eq::ApproxEq;
use crate::epsilon::Epsilon;
use crate::math_error::MathError;
use crate::math_result::MathResult;

/// A deterministic four-component `f32` vector.
///
/// Used as a homogeneous position/direction in projection math and as the
/// per-column storage of [`crate::Mat4`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vec4 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

impl Vec4 {
    /// `(0, 0, 0, 0)`.
    pub const ZERO: Vec4 = Vec4 {
        x: 0.0,
        y: 0.0,
        z: 0.0,
        w: 0.0,
    };
    /// `(1, 1, 1, 1)`.
    pub const ONE: Vec4 = Vec4 {
        x: 1.0,
        y: 1.0,
        z: 1.0,
        w: 1.0,
    };

    /// Component constructor.
    pub const fn new(x: f32, y: f32, z: f32, w: f32) -> Self {
        Vec4 { x, y, z, w }
    }

    /// Component-wise sum.
    pub const fn add(self, other: Vec4) -> Vec4 {
        Vec4::new(
            self.x + other.x,
            self.y + other.y,
            self.z + other.z,
            self.w + other.w,
        )
    }

    /// Component-wise difference.
    pub const fn subtract(self, other: Vec4) -> Vec4 {
        Vec4::new(
            self.x - other.x,
            self.y - other.y,
            self.z - other.z,
            self.w - other.w,
        )
    }

    /// Scale by a scalar.
    pub const fn mul_scalar(self, k: f32) -> Vec4 {
        Vec4::new(self.x * k, self.y * k, self.z * k, self.w * k)
    }

    /// Divide by a scalar with the same checked semantics as the other
    /// vectors.
    pub fn div_scalar(self, k: f32) -> MathResult<Vec4> {
        if !k.is_finite() {
            return Err(MathError::non_finite_scalar(
                "vec4 scalar divisor must be finite",
            ));
        }
        if k == 0.0 {
            return Err(MathError::divide_by_zero("vec4 scalar divisor was zero"));
        }
        Ok(Vec4::new(self.x / k, self.y / k, self.z / k, self.w / k))
    }

    /// Dot product.
    pub const fn dot(self, other: Vec4) -> f32 {
        self.x * other.x + self.y * other.y + self.z * other.z + self.w * other.w
    }

    /// Append the four `f32` components in declaration order.
    pub fn write_to(self, writer: &mut BinaryWriter) {
        writer.write_f32(self.x);
        writer.write_f32(self.y);
        writer.write_f32(self.z);
        writer.write_f32(self.w);
    }

    /// Read four `f32` components in declaration order.
    pub fn read_from(reader: &mut BinaryReader<'_>) -> KernelResult<Vec4> {
        let x = reader.read_f32()?;
        let y = reader.read_f32()?;
        let z = reader.read_f32()?;
        let w = reader.read_f32()?;
        Ok(Vec4::new(x, y, z, w))
    }
}

impl ApproxEq for Vec4 {
    fn approx_eq(&self, other: &Self, epsilon: Epsilon) -> bool {
        self.x.approx_eq(&other.x, epsilon)
            && self.y.approx_eq(&other.y, epsilon)
            && self.z.approx_eq(&other.z, epsilon)
            && self.w.approx_eq(&other.w, epsilon)
    }
}

impl Reflect for Vec4 {
    const SCHEMA: TypeSchema = TypeSchema::new(
        "Vec4",
        &[
            FieldSchema::new("x", "f32"),
            FieldSchema::new("y", "f32"),
            FieldSchema::new("z", "f32"),
            FieldSchema::new("w", "f32"),
        ],
    );

    fn reflect_write(&self, writer: &mut BinaryWriter) {
        self.x.reflect_write(writer);
        self.y.reflect_write(writer);
        self.z.reflect_write(writer);
        self.w.reflect_write(writer);
    }

    fn reflect_read(reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
        Ok(Vec4::new(
            f32::reflect_read(reader)?,
            f32::reflect_read(reader)?,
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
        let v = Vec4::new(1.0, -2.0, 3.5, 4.0);
        let mut w = BinaryWriter::new();
        v.reflect_write(&mut w);
        let bytes = w.into_bytes();
        assert_eq!(
            Vec4::reflect_read(&mut BinaryReader::new(&bytes)).unwrap(),
            v
        );
        for len in 0..bytes.len() {
            assert!(Vec4::reflect_read(&mut BinaryReader::new(&bytes[..len])).is_err());
        }
        assert_eq!(<Vec4 as Reflect>::SCHEMA.name(), "Vec4");
        assert_eq!(<Vec4 as Reflect>::SCHEMA.fields().len(), 4);
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
        assert!(Vec4::ZERO.approx_eq(&Vec4::new(0.0, 0.0, 0.0, 0.0), eps()));
        assert!(Vec4::ONE.approx_eq(&Vec4::new(1.0, 1.0, 1.0, 1.0), eps()));
    }

    #[test]
    fn add_and_subtract_are_component_wise() {
        let a = Vec4::new(1.0, 2.0, 3.0, 4.0);
        let b = Vec4::new(5.0, 6.0, 7.0, 8.0);
        assert!(a.add(b).approx_eq(&Vec4::new(6.0, 8.0, 10.0, 12.0), eps()));
        assert!(b
            .subtract(a)
            .approx_eq(&Vec4::new(4.0, 4.0, 4.0, 4.0), eps()));
    }

    #[test]
    fn mul_and_div_scalar_work() {
        let v = Vec4::new(2.0, -4.0, 6.0, 8.0);
        assert!(v
            .mul_scalar(0.5)
            .approx_eq(&Vec4::new(1.0, -2.0, 3.0, 4.0), eps()));
        assert!(v
            .div_scalar(2.0)
            .unwrap()
            .approx_eq(&Vec4::new(1.0, -2.0, 3.0, 4.0), eps()));
    }

    #[test]
    fn div_by_zero_is_rejected() {
        assert_eq!(
            Vec4::ONE.div_scalar(0.0).unwrap_err().code(),
            MathErrorCode::DivideByZero
        );
    }

    #[test]
    fn div_by_non_finite_is_rejected() {
        assert_eq!(
            Vec4::ONE.div_scalar(f32::INFINITY).unwrap_err().code(),
            MathErrorCode::NonFiniteScalar
        );
    }

    #[test]
    fn dot_matches_expected() {
        assert_eq!(
            Vec4::new(1.0, 2.0, 3.0, 4.0).dot(Vec4::new(5.0, 6.0, 7.0, 8.0)),
            1.0 * 5.0 + 2.0 * 6.0 + 3.0 * 7.0 + 4.0 * 8.0
        );
    }

    #[test]
    fn approx_eq_rejects_nan_components() {
        let nan = Vec4::new(0.0, 0.0, 0.0, f32::NAN);
        assert!(!nan.approx_eq(&Vec4::ZERO, eps()));
    }

    #[test]
    fn binary_round_trip_preserves_components() {
        let api = KernelApi::new();
        let v = Vec4::new(1.5, -2.25, 3.125, -0.5);

        let mut writer = api.binary_writer();
        v.write_to(&mut writer);
        let bytes = writer.into_bytes();

        let mut reader = api.binary_reader(&bytes);
        let back = Vec4::read_from(&mut reader).unwrap();
        assert!(back.approx_eq(&v, eps()));
    }
}

#[cfg(test)]
mod cov {
    use super::*;
    use axiom_kernel::BinaryReader;

    #[test]
    fn read_from_truncated_each_component() {
        assert!(Vec4::read_from(&mut BinaryReader::new(&[])).is_err());
        assert!(Vec4::read_from(&mut BinaryReader::new(&[0u8; 4])).is_err());
        assert!(Vec4::read_from(&mut BinaryReader::new(&[0u8; 8])).is_err());
        assert!(Vec4::read_from(&mut BinaryReader::new(&[0u8; 12])).is_err());
    }

    #[test]
    fn approx_eq_each_component_differs() {
        let base = Vec4::new(0.0, 0.0, 0.0, 0.0);
        let eps = Epsilon::DEFAULT;
        assert!(!base.approx_eq(&Vec4::new(1.0, 0.0, 0.0, 0.0), eps));
        assert!(!base.approx_eq(&Vec4::new(0.0, 1.0, 0.0, 0.0), eps));
        assert!(!base.approx_eq(&Vec4::new(0.0, 0.0, 1.0, 0.0), eps));
        assert!(!base.approx_eq(&Vec4::new(0.0, 0.0, 0.0, 1.0), eps));
        assert!(base.approx_eq(&base, eps));
    }
}
