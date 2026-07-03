//! Unit-rotation quaternion.

use axiom_kernel::{BinaryReader, BinaryWriter, FieldSchema, KernelResult, Reflect, TypeSchema};

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
        angle
            .is_finite()
            .then_some(())
            .ok_or_else(|| MathError::non_finite_scalar("quaternion angle must be finite"))
            .and_then(|()| axis.normalize())
            .map(|axis| {
                let half = angle * 0.5;
                let s = half.sin();
                let c = half.cos();
                Quat::new(axis.x * s, axis.y * s, axis.z * s, c)
            })
    }

    /// Build a unit rotation from intrinsic Euler angles (radians), applied in
    /// **X-then-Y-then-Z** order (`Rz · Ry · Rx`). Infallible by construction:
    /// the three rotations are built directly from half-angle sines/cosines
    /// about the canonical unit axes, so — unlike [`Quat::from_axis_angle`] —
    /// there is no axis-normalization step that can fail. Non-finite inputs
    /// propagate to a non-finite quaternion, exactly as [`Quat::new`] would.
    ///
    /// This is the primitive a skeletal-animation layer builds joint rotations
    /// on: authored per-joint Euler angles (clampable against anatomical limits)
    /// convert here to the composable rotation FK needs.
    pub fn from_euler_xyz(x: f32, y: f32, z: f32) -> Quat {
        let (hx, hy, hz) = (x * 0.5, y * 0.5, z * 0.5);
        let qx = Quat::new(hx.sin(), 0.0, 0.0, hx.cos());
        let qy = Quat::new(0.0, hy.sin(), 0.0, hy.cos());
        let qz = Quat::new(0.0, 0.0, hz.sin(), hz.cos());
        qz.multiply(qy).multiply(qx)
    }

    /// Build a unit rotation that orients local **-Z** along `forward` with
    /// local **+Y** toward `up`, using the same right-handed basis
    /// [`crate::Mat4::look_at`] uses for its view. This is the *camera-to-world*
    /// rotation: a node carrying it, viewed through `inverse(world)`, looks down
    /// `forward`. `up` need only be non-parallel to `forward`; it is
    /// orthonormalised, not used verbatim.
    ///
    /// Fails with
    /// [`crate::math_error_code::MathErrorCode::InvalidMatrixOperation`] when
    /// `forward` is zero-length or parallel to `up` (the basis would be
    /// degenerate) — matching `Mat4::look_at`.
    pub fn look_rotation(forward: Vec3, up: Vec3) -> MathResult<Quat> {
        forward
            .normalize()
            .map_err(|_| {
                MathError::invalid_matrix_operation("look_rotation forward is zero-length")
            })
            .and_then(|f| {
                f.cross(up)
                    .normalize()
                    .map_err(|_| {
                        MathError::invalid_matrix_operation(
                            "look_rotation forward and up are parallel",
                        )
                    })
                    .map(|s| Self::shepperd(f, s))
            })
    }

    /// Shepperd's quaternion extraction for the rotation whose columns are the
    /// world-space camera axes `+X -> s`, `+Y -> u`, `+Z -> -f`. The original
    /// picked one of four formulas via the largest diagonal term; here all four
    /// candidates are computed (each pure, no panic) and the matching one is
    /// selected branchlessly. The selection conditions are mutually exclusive,
    /// so exactly one candidate is chosen — value-identical to the branched
    /// form.
    fn shepperd(f: Vec3, s: Vec3) -> Quat {
        let u = s.cross(f);
        let m00 = s.x;
        let m11 = u.y;
        let m22 = -f.z;
        let trace = m00 + m11 + m22;

        let big_w = (trace + 1.0).sqrt(); // 2w
        let r_w = 0.5 / big_w;
        let cand_w = Quat::new(
            (u.z + f.y) * r_w,
            (-f.x - s.z) * r_w,
            (s.y - u.x) * r_w,
            0.5 * big_w,
        );

        let big_x = (1.0 + m00 - m11 - m22).sqrt(); // 2x
        let r_x = 0.5 / big_x;
        let cand_x = Quat::new(
            0.5 * big_x,
            (u.x + s.y) * r_x,
            (-f.x + s.z) * r_x,
            (u.z + f.y) * r_x,
        );

        let big_y = (1.0 + m11 - m00 - m22).sqrt(); // 2y
        let r_y = 0.5 / big_y;
        let cand_y = Quat::new(
            (u.x + s.y) * r_y,
            0.5 * big_y,
            (-f.y + u.z) * r_y,
            (-f.x - s.z) * r_y,
        );

        let big_z = (1.0 + m22 - m00 - m11).sqrt(); // 2z
        let r_z = 0.5 / big_z;
        let cand_z = Quat::new(
            (-f.x + s.z) * r_z,
            (-f.y + u.z) * r_z,
            0.5 * big_z,
            (s.y - u.x) * r_z,
        );

        // Mirror the original if / else-if / else-if / else exactly. The four
        // conditions are evaluated in order; `select(cond, fallback)` returns
        // `self` when `cond` else `fallback`, so nesting them reproduces the
        // first-match cascade with the final `else` defaulting to `cand_z`.
        let pick_w = trace > 0.0;
        let pick_x = !pick_w & (m00 >= m11) & (m00 >= m22);
        let pick_y = !pick_w & !pick_x & (m11 >= m22);
        let after_w = cand_w.select(pick_w, cand_z);
        let after_x = cand_x.select(pick_x, after_w);
        cand_y.select(pick_y, after_x)
    }

    /// Branchless choose: `self` when `cond`, else `other`.
    fn select(self, cond: bool, other: Quat) -> Quat {
        [other, self][usize::from(cond)]
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
        let valid = (len != 0.0) & len.is_finite();
        valid
            .then_some(len)
            .map(|len| Quat::new(self.x / len, self.y / len, self.z / len, self.w / len))
            .ok_or_else(|| {
                MathError::normalize_zero_length("cannot normalize zero-length quaternion")
            })
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
        let valid = (ls != 0.0) & ls.is_finite();
        valid
            .then_some(ls)
            .map(|ls| {
                let c = self.conjugate();
                Quat::new(c.x / ls, c.y / ls, c.z / ls, c.w / ls)
            })
            .ok_or_else(|| MathError::normalize_zero_length("cannot invert zero-length quaternion"))
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
        reader.read_f32().and_then(|x| {
            reader.read_f32().and_then(|y| {
                reader
                    .read_f32()
                    .and_then(|z| reader.read_f32().map(|w| Quat::new(x, y, z, w)))
            })
        })
    }
}

impl ApproxEq for Quat {
    fn approx_eq(&self, other: &Self, epsilon: Epsilon) -> bool {
        self.x.approx_eq(&other.x, epsilon)
            & self.y.approx_eq(&other.y, epsilon)
            & self.z.approx_eq(&other.z, epsilon)
            & self.w.approx_eq(&other.w, epsilon)
    }
}

impl Reflect for Quat {
    const SCHEMA: TypeSchema = TypeSchema::new(
        "Quat",
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
        f32::reflect_read(reader).and_then(|x| {
            f32::reflect_read(reader).and_then(|y| {
                f32::reflect_read(reader)
                    .and_then(|z| f32::reflect_read(reader).map(|w| Quat::new(x, y, z, w)))
            })
        })
    }
}

#[cfg(test)]
mod reflect_tests {
    use super::*;

    #[test]
    fn reflect_round_trips_describes_and_rejects_truncation() {
        let q = Quat::new(0.0, 0.5, 0.0, 0.866);
        let mut w = BinaryWriter::new();
        q.reflect_write(&mut w);
        let bytes = w.into_bytes();
        assert_eq!(
            Quat::reflect_read(&mut BinaryReader::new(&bytes)).unwrap(),
            q
        );
        for len in 0..bytes.len() {
            assert!(Quat::reflect_read(&mut BinaryReader::new(&bytes[..len])).is_err());
        }
        assert_eq!(<Quat as Reflect>::SCHEMA.name(), "Quat");
        assert_eq!(<Quat as Reflect>::SCHEMA.fields().len(), 4);
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

    fn assert_look_maps_axes(forward: Vec3, up: Vec3, right: Vec3, real_up: Vec3) {
        let q = Quat::look_rotation(forward, up).unwrap();
        assert!((q.length() - 1.0).abs() <= eps().value());
        assert!(q
            .rotate(Vec3::new(0.0, 0.0, -1.0))
            .approx_eq(&forward, eps()));
        assert!(q.rotate(Vec3::UNIT_X).approx_eq(&right, eps()));
        assert!(q.rotate(Vec3::UNIT_Y).approx_eq(&real_up, eps()));
    }

    #[test]
    fn look_rotation_forward_is_identity_when_facing_negative_z() {
        let q = Quat::look_rotation(Vec3::new(0.0, 0.0, -1.0), Vec3::UNIT_Y).unwrap();
        assert!(q.approx_eq(&Quat::IDENTITY, eps()));
        assert_look_maps_axes(
            Vec3::new(0.0, 0.0, -1.0),
            Vec3::UNIT_Y,
            Vec3::UNIT_X,
            Vec3::UNIT_Y,
        );
    }

    #[test]
    fn look_rotation_covers_x_dominant_branch() {
        assert_look_maps_axes(
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::new(0.0, -1.0, 0.0),
            Vec3::UNIT_X,
            Vec3::new(0.0, -1.0, 0.0),
        );
    }

    #[test]
    fn look_rotation_covers_y_dominant_branch() {
        assert_look_maps_axes(
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::UNIT_Y,
            Vec3::new(-1.0, 0.0, 0.0),
            Vec3::UNIT_Y,
        );
    }

    #[test]
    fn look_rotation_covers_z_dominant_branch() {
        assert_look_maps_axes(
            Vec3::new(0.0, 0.0, -1.0),
            Vec3::new(0.0, -1.0, 0.0),
            Vec3::new(-1.0, 0.0, 0.0),
            Vec3::new(0.0, -1.0, 0.0),
        );
    }

    #[test]
    fn look_rotation_rejects_degenerate_inputs() {
        assert_eq!(
            Quat::look_rotation(Vec3::ZERO, Vec3::UNIT_Y)
                .unwrap_err()
                .code(),
            MathErrorCode::InvalidMatrixOperation
        );
        assert_eq!(
            Quat::look_rotation(Vec3::UNIT_Y, Vec3::UNIT_Y)
                .unwrap_err()
                .code(),
            MathErrorCode::InvalidMatrixOperation
        );
    }

    #[test]
    fn multiplication_order_is_deterministic() {
        let rot_z = Quat::from_axis_angle(Vec3::UNIT_Z, std::f32::consts::FRAC_PI_2).unwrap();
        let rot_y = Quat::from_axis_angle(Vec3::UNIT_Y, std::f32::consts::FRAC_PI_2).unwrap();
        let composed = rot_y.multiply(rot_z);
        let direct = rot_y.rotate(rot_z.rotate(Vec3::UNIT_X));
        let through_q = composed.rotate(Vec3::UNIT_X);
        assert!(through_q.approx_eq(&direct, eps()));

        let reverse = rot_z.multiply(rot_y);
        assert!(!reverse.rotate(Vec3::UNIT_X).approx_eq(&through_q, eps()));
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

#[cfg(test)]
mod cov {
    use super::*;
    use axiom_kernel::BinaryReader;

    #[test]
    fn normalize_non_finite_length_fails() {
        let q = Quat::new(f32::MAX, f32::MAX, f32::MAX, f32::MAX);
        assert!(q.normalize().is_err());
    }

    #[test]
    fn inverse_non_finite_length_squared_fails() {
        let q = Quat::new(f32::MAX, f32::MAX, f32::MAX, f32::MAX);
        assert!(q.inverse().is_err());
    }

    #[test]
    fn read_from_truncated_each_component() {
        assert!(Quat::read_from(&mut BinaryReader::new(&[])).is_err());
        assert!(Quat::read_from(&mut BinaryReader::new(&[0u8; 4])).is_err());
        assert!(Quat::read_from(&mut BinaryReader::new(&[0u8; 8])).is_err());
        assert!(Quat::read_from(&mut BinaryReader::new(&[0u8; 12])).is_err());
    }

    #[test]
    fn approx_eq_each_component_differs() {
        let base = Quat::new(0.0, 0.0, 0.0, 1.0);
        let eps = Epsilon::DEFAULT;
        assert!(!base.approx_eq(&Quat::new(1.0, 0.0, 0.0, 1.0), eps));
        assert!(!base.approx_eq(&Quat::new(0.0, 1.0, 0.0, 1.0), eps));
        assert!(!base.approx_eq(&Quat::new(0.0, 0.0, 1.0, 1.0), eps));
        assert!(!base.approx_eq(&Quat::new(0.0, 0.0, 0.0, 2.0), eps));
        assert!(base.approx_eq(&base, eps));
    }

    #[test]
    fn normalize_divides_each_component_by_length() {
        let n = Quat::new(2.0, 2.0, 2.0, 2.0).normalize().unwrap();
        assert_eq!(n.x, 0.5);
        assert_eq!(n.y, 0.5);
        assert_eq!(n.z, 0.5);
        assert_eq!(n.w, 0.5);
    }

    #[test]
    fn inverse_divides_conjugate_by_length_squared() {
        let inv = Quat::new(1.0, 2.0, 3.0, 4.0).inverse().unwrap();
        let ls = 30.0f32;
        assert!((inv.x - (-1.0 / ls)).abs() < 1.0e-7);
        assert!((inv.y - (-2.0 / ls)).abs() < 1.0e-7);
        assert!((inv.z - (-3.0 / ls)).abs() < 1.0e-7);
        assert!((inv.w - (4.0 / ls)).abs() < 1.0e-7);
    }

    #[test]
    fn multiply_hamilton_product_is_exact() {
        let a = Quat::new(1.0, 2.0, 3.0, 4.0);
        let b = Quat::new(5.0, 6.0, 7.0, 8.0);
        let r = a.multiply(b);
        assert_eq!(r.x, 24.0);
        assert_eq!(r.y, 48.0);
        assert_eq!(r.z, 48.0);
        assert_eq!(r.w, -6.0);
    }

    fn cov_eps() -> Epsilon {
        Epsilon::new(1.0e-5).unwrap()
    }

    #[test]
    fn from_euler_zero_is_identity() {
        assert!(Quat::from_euler_xyz(0.0, 0.0, 0.0).approx_eq(&Quat::IDENTITY, cov_eps()));
    }

    #[test]
    fn from_euler_single_axis_matches_axis_angle() {
        let angle = 0.7f32;
        assert!(Quat::from_euler_xyz(angle, 0.0, 0.0)
            .approx_eq(&Quat::from_axis_angle(Vec3::UNIT_X, angle).unwrap(), cov_eps()));
        assert!(Quat::from_euler_xyz(0.0, angle, 0.0)
            .approx_eq(&Quat::from_axis_angle(Vec3::UNIT_Y, angle).unwrap(), cov_eps()));
        assert!(Quat::from_euler_xyz(0.0, 0.0, angle)
            .approx_eq(&Quat::from_axis_angle(Vec3::UNIT_Z, angle).unwrap(), cov_eps()));
    }

    #[test]
    fn from_euler_composes_x_then_y_then_z() {
        // Applying X first then Y then Z equals Rz * Ry * Rx as a rotation.
        let (x, y, z) = (0.3f32, -0.5f32, 0.4f32);
        let composed = Quat::from_axis_angle(Vec3::UNIT_Z, z)
            .unwrap()
            .multiply(Quat::from_axis_angle(Vec3::UNIT_Y, y).unwrap())
            .multiply(Quat::from_axis_angle(Vec3::UNIT_X, x).unwrap());
        let v = Vec3::new(0.2, 0.9, -0.4);
        assert!(Quat::from_euler_xyz(x, y, z)
            .rotate(v)
            .approx_eq(&composed.rotate(v), cov_eps()));
    }
}
