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
        let f = forward.normalize().map_err(|_| {
            MathError::invalid_matrix_operation("look_rotation forward is zero-length")
        })?;
        let s = f.cross(up).normalize().map_err(|_| {
            MathError::invalid_matrix_operation("look_rotation forward and up are parallel")
        })?;
        let u = s.cross(f);
        // Rotation whose columns are the world-space camera axes: +X -> s,
        // +Y -> u, +Z -> -f (so -Z -> f). Extract its quaternion via Shepperd's
        // method, branching on the largest diagonal term so the divisor stays
        // well away from zero. The diagonal entries are m00 = s.x, m11 = u.y,
        // m22 = -f.z; off-diagonals follow from the same column layout.
        let m00 = s.x;
        let m11 = u.y;
        let m22 = -f.z;
        let trace = m00 + m11 + m22;
        let q = if trace > 0.0 {
            let big = (trace + 1.0).sqrt(); // 2w
            let r = 0.5 / big; // 1 / 4w
            Quat::new((u.z + f.y) * r, (-f.x - s.z) * r, (s.y - u.x) * r, 0.5 * big)
        } else if m00 >= m11 && m00 >= m22 {
            let big = (1.0 + m00 - m11 - m22).sqrt(); // 2x
            let r = 0.5 / big;
            Quat::new(0.5 * big, (u.x + s.y) * r, (-f.x + s.z) * r, (u.z + f.y) * r)
        } else if m11 >= m22 {
            let big = (1.0 + m11 - m00 - m22).sqrt(); // 2y
            let r = 0.5 / big;
            Quat::new((u.x + s.y) * r, 0.5 * big, (-f.y + u.z) * r, (-f.x - s.z) * r)
        } else {
            let big = (1.0 + m22 - m00 - m11).sqrt(); // 2z
            let r = 0.5 / big;
            Quat::new((-f.x + s.z) * r, (-f.y + u.z) * r, 0.5 * big, (s.y - u.x) * r)
        };
        Ok(q)
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
        Ok(Quat::new(
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
        let q = Quat::new(0.0, 0.5, 0.0, 0.866);
        let mut w = BinaryWriter::new();
        q.reflect_write(&mut w);
        let bytes = w.into_bytes();
        assert_eq!(Quat::reflect_read(&mut BinaryReader::new(&bytes)).unwrap(), q);
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

    // look_rotation: a node carrying it maps its local axes (+X, +Y, -Z) onto
    // the world basis (right, up, forward). Each case is chosen to land in a
    // distinct Shepperd branch (positive trace, then each dominant diagonal).
    fn assert_look_maps_axes(forward: Vec3, up: Vec3, right: Vec3, real_up: Vec3) {
        let q = Quat::look_rotation(forward, up).unwrap();
        // Unit rotation.
        assert!((q.length() - 1.0).abs() <= eps().value());
        // Local -Z faces `forward`; +X -> right; +Y -> the orthonormalised up.
        assert!(q.rotate(Vec3::new(0.0, 0.0, -1.0)).approx_eq(&forward, eps()));
        assert!(q.rotate(Vec3::UNIT_X).approx_eq(&right, eps()));
        assert!(q.rotate(Vec3::UNIT_Y).approx_eq(&real_up, eps()));
    }

    #[test]
    fn look_rotation_forward_is_identity_when_facing_negative_z() {
        // trace > 0 branch: basis already aligned, rotation is identity.
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
        // forward +Z, up -Y -> R = diag(1,-1,-1), m00 dominant.
        assert_look_maps_axes(
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::new(0.0, -1.0, 0.0),
            Vec3::UNIT_X,
            Vec3::new(0.0, -1.0, 0.0),
        );
    }

    #[test]
    fn look_rotation_covers_y_dominant_branch() {
        // forward +Z, up +Y -> R = diag(-1,1,-1), m11 dominant.
        assert_look_maps_axes(
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::UNIT_Y,
            Vec3::new(-1.0, 0.0, 0.0),
            Vec3::UNIT_Y,
        );
    }

    #[test]
    fn look_rotation_covers_z_dominant_branch() {
        // forward -Z, up -Y -> R = diag(-1,-1,1), m22 dominant.
        assert_look_maps_axes(
            Vec3::new(0.0, 0.0, -1.0),
            Vec3::new(0.0, -1.0, 0.0),
            Vec3::new(-1.0, 0.0, 0.0),
            Vec3::new(0.0, -1.0, 0.0),
        );
    }

    #[test]
    fn look_rotation_rejects_degenerate_inputs() {
        // Zero-length forward.
        assert_eq!(
            Quat::look_rotation(Vec3::ZERO, Vec3::UNIT_Y).unwrap_err().code(),
            MathErrorCode::InvalidMatrixOperation
        );
        // Forward parallel to up.
        assert_eq!(
            Quat::look_rotation(Vec3::UNIT_Y, Vec3::UNIT_Y).unwrap_err().code(),
            MathErrorCode::InvalidMatrixOperation
        );
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

    // Kills normalize divide mutants at 78/79/80 (`/ len` -> `% len` / `* len`).
    // q = (2,2,2,2) has length 4, so each component normalizes to exactly 0.5.
    // The mutated forms give 2*4=8 (mul) or 2%4=2 (rem), both checkable.
    #[test]
    fn normalize_divides_each_component_by_length() {
        let n = Quat::new(2.0, 2.0, 2.0, 2.0).normalize().unwrap();
        assert_eq!(n.x, 0.5);
        assert_eq!(n.y, 0.5);
        assert_eq!(n.z, 0.5);
        assert_eq!(n.w, 0.5);
    }

    // Kills inverse divide mutants at 101 (`/ ls` -> `% ls` / `* ls`) for all
    // four components. q = (1,2,3,4) has length_squared 30; inverse is
    // conjugate/30 = (-1/30, -2/30, -3/30, 4/30).
    #[test]
    fn inverse_divides_conjugate_by_length_squared() {
        let inv = Quat::new(1.0, 2.0, 3.0, 4.0).inverse().unwrap();
        let ls = 30.0f32;
        assert!((inv.x - (-1.0 / ls)).abs() < 1.0e-7);
        assert!((inv.y - (-2.0 / ls)).abs() < 1.0e-7);
        assert!((inv.z - (-3.0 / ls)).abs() < 1.0e-7);
        assert!((inv.w - (4.0 / ls)).abs() < 1.0e-7);
    }

    // Kills every Hamilton-product mutant at 110..=113. Operands have distinct
    // nonzero integer components so each term is value-significant; the exact
    // product is asserted component-wise.
    #[test]
    fn multiply_hamilton_product_is_exact() {
        let a = Quat::new(1.0, 2.0, 3.0, 4.0);
        let b = Quat::new(5.0, 6.0, 7.0, 8.0);
        let r = a.multiply(b);
        // x = aw*bx + ax*bw + ay*bz - az*by = 20 + 8 + 14 - 18 = 24
        assert_eq!(r.x, 24.0);
        // y = aw*by - ax*bz + ay*bw + az*bx = 24 - 7 + 16 + 15 = 48
        assert_eq!(r.y, 48.0);
        // z = aw*bz + ax*by - ay*bx + az*bw = 28 + 6 - 10 + 24 = 48
        assert_eq!(r.z, 48.0);
        // w = aw*bw - ax*bx - ay*by - az*bz = 32 - 5 - 12 - 21 = -6
        assert_eq!(r.w, -6.0);
    }
}
