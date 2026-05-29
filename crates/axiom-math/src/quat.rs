//! Unit-rotation quaternion.

use axiom_kernel::{BinaryReader, BinaryWriter, KernelResult};

use crate::approx_eq::ApproxEq;
use crate::epsilon::Epsilon;
use crate::math_error::MathError;
use crate::math_result::MathResult;
use crate::vec3::Vec3;

/// A rotation quaternion `(x, y, z, w)` (vector part first, scalar `w` last).
///
/// `Quat` is **not** enforced to be unit-length at the type level — that lets
/// callers compose rotations cheaply — but every method that depends on
/// unit-ness ([`Quat::rotate`], [`Quat::inverse`]) is documented to expect a
/// previously [`Quat::normalize`]d input. Quaternion multiplication uses the
/// standard right-handed convention: `(self * other)` rotates first by
/// `other`, then by `self`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quat {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

impl Quat {
    /// The identity rotation `(0, 0, 0, 1)`.
    pub const IDENTITY: Quat = Quat {
        x: 0.0,
        y: 0.0,
        z: 0.0,
        w: 1.0,
    };

    /// Raw component constructor.
    pub const fn new(x: f32, y: f32, z: f32, w: f32) -> Self {
        Quat { x, y, z, w }
    }

    /// Build a unit quaternion from a *unit* `axis` and an `angle` in radians.
    /// Fails with [`crate::math_error_code::MathErrorCode::NormalizeZeroLength`]
    /// when the axis is zero-length, and with
    /// [`crate::math_error_code::MathErrorCode::NonFiniteScalar`] when any
    /// component is not finite.
    pub fn from_axis_angle(axis: Vec3, angle: f32) -> MathResult<Quat> {
        if !angle.is_finite() {
            return Err(MathError::non_finite_scalar(
                "quaternion angle must be finite",
            ));
        }
        let axis = axis.normalize()?;
        let half = angle * 0.5;
        let s = half.sin();
        let c = half.cos();
        Ok(Quat::new(axis.x * s, axis.y * s, axis.z * s, c))
    }

    /// Squared length (`x² + y² + z² + w²`).
    pub const fn length_squared(self) -> f32 {
        self.x * self.x + self.y * self.y + self.z * self.z + self.w * self.w
    }

    /// Euclidean length.
    pub fn length(self) -> f32 {
        self.length_squared().sqrt()
    }

    /// Unit-length copy. Fails for the zero quaternion.
    pub fn normalize(self) -> MathResult<Quat> {
        let len = self.length();
        if len == 0.0 || !len.is_finite() {
            return Err(MathError::normalize_zero_length(
                "cannot normalize zero-length quaternion",
            ));
        }
        Ok(Quat::new(
            self.x / len,
            self.y / len,
            self.z / len,
            self.w / len,
        ))
    }

    /// Conjugate `(-x, -y, -z, w)`. For a unit quaternion this is its inverse.
    pub const fn conjugate(self) -> Quat {
        Quat::new(-self.x, -self.y, -self.z, self.w)
    }

    /// Inverse rotation. Fails for the zero quaternion. Uses
    /// `conjugate / length_squared` so it is correct for non-unit inputs as
    /// well as unit ones.
    pub fn inverse(self) -> MathResult<Quat> {
        let ls = self.length_squared();
        if ls == 0.0 || !ls.is_finite() {
            return Err(MathError::normalize_zero_length(
                "cannot invert zero-length quaternion",
            ));
        }
        let c = self.conjugate();
        Ok(Quat::new(c.x / ls, c.y / ls, c.z / ls, c.w / ls))
    }

    /// Hamilton product: `self * other` rotates first by `other`, then by
    /// `self`.
    pub const fn multiply(self, other: Quat) -> Quat {
        let (ax, ay, az, aw) = (self.x, self.y, self.z, self.w);
        let (bx, by, bz, bw) = (other.x, other.y, other.z, other.w);
        Quat::new(
            aw * bx + ax * bw + ay * bz - az * by,
            aw * by - ax * bz + ay * bw + az * bx,
            aw * bz + ax * by - ay * bx + az * bw,
            aw * bw - ax * bx - ay * by - az * bz,
        )
    }

    /// Rotate a vector by this (assumed unit) quaternion.
    pub fn rotate(self, v: Vec3) -> Vec3 {
        // v + 2 * q.xyz × (q.xyz × v + q.w * v) — the standard 18-multiply
        // optimisation of q * v * q^-1 for a unit quaternion.
        let qv = Vec3::new(self.x, self.y, self.z);
        let t = qv.cross(v).mul_scalar(2.0);
        v.add(t.mul_scalar(self.w)).add(qv.cross(t))
    }

    /// Append the four `f32` components in declaration order.
    pub fn write_to(self, writer: &mut BinaryWriter) {
        writer.write_f32(self.x);
        writer.write_f32(self.y);
        writer.write_f32(self.z);
        writer.write_f32(self.w);
    }

    /// Read four `f32` components in declaration order.
    pub fn read_from(reader: &mut BinaryReader<'_>) -> KernelResult<Quat> {
        let x = reader.read_f32()?;
        let y = reader.read_f32()?;
        let z = reader.read_f32()?;
        let w = reader.read_f32()?;
        Ok(Quat::new(x, y, z, w))
    }
}

impl ApproxEq for Quat {
    fn approx_eq(&self, other: &Self, epsilon: Epsilon) -> bool {
        self.x.approx_eq(&other.x, epsilon)
            && self.y.approx_eq(&other.y, epsilon)
            && self.z.approx_eq(&other.z, epsilon)
            && self.w.approx_eq(&other.w, epsilon)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math_error_code::MathErrorCode;
    use axiom_kernel::KernelApi;

    fn eps() -> Epsilon {
        Epsilon::new(1.0e-5).unwrap()
    }

    #[test]
    fn identity_is_neutral_on_vectors() {
        let v = Vec3::new(0.3, -1.2, 2.4);
        assert!(Quat::IDENTITY.rotate(v).approx_eq(&v, eps()));
    }

    #[test]
    fn identity_constant_round_trips_through_new() {
        assert!(Quat::IDENTITY.approx_eq(&Quat::new(0.0, 0.0, 0.0, 1.0), eps()));
    }

    #[test]
    fn quarter_turn_around_z_rotates_x_to_y() {
        let q = Quat::from_axis_angle(Vec3::UNIT_Z, std::f32::consts::FRAC_PI_2).unwrap();
        assert!(q.rotate(Vec3::UNIT_X).approx_eq(&Vec3::UNIT_Y, eps()));
    }

    #[test]
    fn quarter_turn_around_y_rotates_z_to_x() {
        let q = Quat::from_axis_angle(Vec3::UNIT_Y, std::f32::consts::FRAC_PI_2).unwrap();
        assert!(q.rotate(Vec3::UNIT_Z).approx_eq(&Vec3::UNIT_X, eps()));
    }

    #[test]
    fn from_axis_angle_zero_axis_fails() {
        let err = Quat::from_axis_angle(Vec3::ZERO, 1.0).unwrap_err();
        assert_eq!(err.code(), MathErrorCode::NormalizeZeroLength);
    }

    #[test]
    fn from_axis_angle_nan_angle_fails() {
        let err = Quat::from_axis_angle(Vec3::UNIT_X, f32::NAN).unwrap_err();
        assert_eq!(err.code(), MathErrorCode::NonFiniteScalar);
    }

    #[test]
    fn multiplication_order_is_deterministic() {
        // Rotate +X by (rot_z then rot_y): rot_z sends X to Y, rot_y sends Y to Y.
        let rot_z = Quat::from_axis_angle(Vec3::UNIT_Z, std::f32::consts::FRAC_PI_2).unwrap();
        let rot_y = Quat::from_axis_angle(Vec3::UNIT_Y, std::f32::consts::FRAC_PI_2).unwrap();
        let composed = rot_y.multiply(rot_z); // apply rot_z first, then rot_y
        let direct = rot_y.rotate(rot_z.rotate(Vec3::UNIT_X));
        let through_q = composed.rotate(Vec3::UNIT_X);
        assert!(through_q.approx_eq(&direct, eps()));

        // And the reverse composition is genuinely different.
        let reverse = rot_z.multiply(rot_y);
        assert!(!reverse
            .rotate(Vec3::UNIT_X)
            .approx_eq(&through_q, eps()));
    }

    #[test]
    fn inverse_reverses_rotation() {
        let q = Quat::from_axis_angle(Vec3::new(1.0, 2.0, 3.0), 1.234).unwrap();
        let v = Vec3::new(0.5, -0.25, 1.5);
        let back = q.inverse().unwrap().rotate(q.rotate(v));
        assert!(back.approx_eq(&v, eps()));
    }

    #[test]
    fn conjugate_inverts_a_unit_quaternion() {
        let q = Quat::from_axis_angle(Vec3::UNIT_X, 0.7).unwrap();
        let v = Vec3::new(0.1, 0.2, 0.3);
        let back = q.conjugate().rotate(q.rotate(v));
        assert!(back.approx_eq(&v, eps()));
    }

    #[test]
    fn normalize_zero_fails() {
        let err = Quat::new(0.0, 0.0, 0.0, 0.0).normalize().unwrap_err();
        assert_eq!(err.code(), MathErrorCode::NormalizeZeroLength);
    }

    #[test]
    fn inverse_zero_fails() {
        let err = Quat::new(0.0, 0.0, 0.0, 0.0).inverse().unwrap_err();
        assert_eq!(err.code(), MathErrorCode::NormalizeZeroLength);
    }

    #[test]
    fn length_and_length_squared_match() {
        let q = Quat::new(0.0, 0.0, 0.0, 1.0);
        assert_eq!(q.length_squared(), 1.0);
        assert_eq!(q.length(), 1.0);
    }

    #[test]
    fn normalize_produces_unit_length() {
        let q = Quat::new(0.0, 0.0, 0.0, 2.0).normalize().unwrap();
        assert!((q.length() - 1.0).abs() <= eps().value());
    }

    #[test]
    fn binary_round_trip_preserves_components() {
        let api = KernelApi::new();
        let q = Quat::from_axis_angle(Vec3::UNIT_Y, 0.5).unwrap();
        let mut writer = api.binary_writer();
        q.write_to(&mut writer);
        let bytes = writer.into_bytes();
        let mut reader = api.binary_reader(&bytes);
        let back = Quat::read_from(&mut reader).unwrap();
        assert!(back.approx_eq(&q, eps()));
    }
}
