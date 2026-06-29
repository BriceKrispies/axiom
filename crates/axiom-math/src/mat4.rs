//! A 4×4 column-major float matrix.

use axiom_kernel::{BinaryReader, BinaryWriter, FieldSchema, KernelResult, Reflect, TypeSchema};

use crate::approx_eq::ApproxEq;
use crate::epsilon::Epsilon;
use crate::math_error::MathError;
use crate::math_result::MathResult;
use crate::quat::Quat;
use crate::scalar::Scalar;
use crate::vec3::Vec3;
use crate::vec4::Vec4;

/// A column-major 4×4 `f32` matrix.
///
/// Storage is the GPU-standard 16-element column-major layout: element
/// `data[col*4 + row]`. Multiplication uses the mathematical convention
/// `(A * B) * v == A * (B * v)`; in other words the rightmost factor is
/// applied first when transforming a point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mat4 {
    data: [f32; 16],
}

impl Mat4 {
    /// Zero matrix.
    pub const ZERO: Mat4 = Mat4 { data: [0.0; 16] };

    /// Identity matrix.
    pub const IDENTITY: Mat4 = Mat4 {
        data: [
            1.0, 0.0, 0.0, 0.0, //
            0.0, 1.0, 0.0, 0.0, //
            0.0, 0.0, 1.0, 0.0, //
            0.0, 0.0, 0.0, 1.0, //
        ],
    };

    /// Construct from a column-major `[f32; 16]` raw layout.
    pub const fn from_cols_array(data: [f32; 16]) -> Self {
        Mat4 { data }
    }

    /// The raw column-major array.
    pub const fn as_cols_array(&self) -> [f32; 16] {
        self.data
    }

    /// Translation matrix.
    pub const fn translation(t: Vec3) -> Mat4 {
        Mat4::from_cols_array([
            1.0, 0.0, 0.0, 0.0, //
            0.0, 1.0, 0.0, 0.0, //
            0.0, 0.0, 1.0, 0.0, //
            t.x, t.y, t.z, 1.0, //
        ])
    }

    /// Per-axis scale matrix.
    pub const fn scale(s: Vec3) -> Mat4 {
        Mat4::from_cols_array([
            s.x, 0.0, 0.0, 0.0, //
            0.0, s.y, 0.0, 0.0, //
            0.0, 0.0, s.z, 0.0, //
            0.0, 0.0, 0.0, 1.0, //
        ])
    }

    /// Rotation matrix derived from a (assumed-unit) quaternion.
    pub fn from_quaternion(q: Quat) -> Mat4 {
        let (x, y, z, w) = (q.x, q.y, q.z, q.w);
        let xx = x * x;
        let yy = y * y;
        let zz = z * z;
        let xy = x * y;
        let xz = x * z;
        let yz = y * z;
        let wx = w * x;
        let wy = w * y;
        let wz = w * z;
        Mat4::from_cols_array([
            1.0 - 2.0 * (yy + zz),
            2.0 * (xy + wz),
            2.0 * (xz - wy),
            0.0,
            //
            2.0 * (xy - wz),
            1.0 - 2.0 * (xx + zz),
            2.0 * (yz + wx),
            0.0,
            //
            2.0 * (xz + wy),
            2.0 * (yz - wx),
            1.0 - 2.0 * (xx + yy),
            0.0,
            //
            0.0,
            0.0,
            0.0,
            1.0,
        ])
    }

    /// Right-handed perspective projection with depth in `[-1, 1]`.
    ///
    /// Validates that `aspect > 0`, `near > 0`, `far > near`, and that
    /// `fovy_radians` is in `(0, π)`. Otherwise returns
    /// [`crate::math_error_code::MathErrorCode::InvalidMatrixOperation`].
    pub fn perspective(fovy_radians: f32, aspect: f32, near: f32, far: f32) -> MathResult<Mat4> {
        let all_finite =
            fovy_radians.is_finite() & aspect.is_finite() & near.is_finite() & far.is_finite();
        let fovy_in_range = (fovy_radians > 0.0) & (fovy_radians < std::f32::consts::PI);
        let bounds_ok = (aspect > 0.0) & (near > 0.0) & (far > near);
        (!all_finite)
            .then_some(Err(MathError::invalid_matrix_operation(
                "perspective parameters must be finite",
            )))
            .or_else(|| {
                (all_finite & !fovy_in_range).then_some(Err(MathError::invalid_matrix_operation(
                    "perspective fovy must be in (0, pi)",
                )))
            })
            .or_else(|| {
                (all_finite & fovy_in_range & !bounds_ok).then_some(Err(
                    MathError::invalid_matrix_operation(
                        "perspective requires aspect > 0, near > 0, far > near",
                    ),
                ))
            })
            .unwrap_or_else(|| Self::perspective_unchecked(fovy_radians, aspect, near, far))
    }

    fn perspective_unchecked(
        fovy_radians: f32,
        aspect: f32,
        near: f32,
        far: f32,
    ) -> MathResult<Mat4> {
        let f = 1.0 / (fovy_radians * 0.5).tan();
        let nf = 1.0 / (near - far);
        Ok(Mat4::from_cols_array([
            f / aspect,
            0.0,
            0.0,
            0.0,
            //
            0.0,
            f,
            0.0,
            0.0,
            //
            0.0,
            0.0,
            (far + near) * nf,
            -1.0,
            //
            0.0,
            0.0,
            2.0 * far * near * nf,
            0.0,
        ]))
    }

    /// Right-handed orthographic projection with depth in `[-1, 1]`.
    ///
    /// Validates `right > left`, `top > bottom`, `far > near`, all finite.
    pub fn orthographic(
        left: f32,
        right: f32,
        bottom: f32,
        top: f32,
        near: f32,
        far: f32,
    ) -> MathResult<Mat4> {
        let all_finite = [left, right, bottom, top, near, far]
            .into_iter()
            .all(|v| v.is_finite());
        let bounds_ok = (right > left) & (top > bottom) & (far > near);
        (!all_finite)
            .then_some(Err(MathError::invalid_matrix_operation(
                "orthographic parameters must be finite",
            )))
            .or_else(|| {
                (all_finite & !bounds_ok).then_some(Err(MathError::invalid_matrix_operation(
                    "orthographic requires right > left, top > bottom, far > near",
                )))
            })
            .unwrap_or_else(|| Self::orthographic_unchecked(left, right, bottom, top, near, far))
    }

    fn orthographic_unchecked(
        left: f32,
        right: f32,
        bottom: f32,
        top: f32,
        near: f32,
        far: f32,
    ) -> MathResult<Mat4> {
        let rl = right - left;
        let tb = top - bottom;
        let fn_ = far - near;
        Ok(Mat4::from_cols_array([
            2.0 / rl,
            0.0,
            0.0,
            0.0,
            //
            0.0,
            2.0 / tb,
            0.0,
            0.0,
            //
            0.0,
            0.0,
            -2.0 / fn_,
            0.0,
            //
            -(right + left) / rl,
            -(top + bottom) / tb,
            -(far + near) / fn_,
            1.0,
        ]))
    }

    /// Right-handed look-at view matrix.
    ///
    /// Fails when `eye == target` or when `target - eye` is parallel to `up`
    /// (the resulting basis would be degenerate).
    pub fn look_at(eye: Vec3, target: Vec3, up: Vec3) -> MathResult<Mat4> {
        target
            .subtract(eye)
            .normalize()
            .map_err(|_| MathError::invalid_matrix_operation("look_at eye and target coincide"))
            .and_then(|f| {
                f.cross(up)
                    .normalize()
                    .map_err(|_| {
                        MathError::invalid_matrix_operation("look_at forward and up are parallel")
                    })
                    .map(|s| {
                        let u = s.cross(f);
                        Mat4::from_cols_array([
                            s.x,
                            u.x,
                            -f.x,
                            0.0, //
                            s.y,
                            u.y,
                            -f.y,
                            0.0, //
                            s.z,
                            u.z,
                            -f.z,
                            0.0, //
                            -s.dot(eye),
                            -u.dot(eye),
                            f.dot(eye),
                            1.0,
                        ])
                    })
            })
    }

    /// Matrix product `self * other`.
    #[axiom_zones::strict]
    pub fn multiply(self, other: Mat4) -> Mat4 {
        let a = &self.data;
        let b = &other.data;
        // Explicit unroll of the column-major 4x4 product. Each element keeps
        // the exact accumulation order of the original `acc += a[k*4+row] *
        // b[col*4+k]` loop over k = 0,1,2,3: `(((0 + t0) + t1) + t2) + t3`.
        let elem = |col: usize, row: usize| -> f32 {
            0.0f32
                + a[row] * b[col * 4]
                + a[4 + row] * b[col * 4 + 1]
                + a[8 + row] * b[col * 4 + 2]
                + a[12 + row] * b[col * 4 + 3]
        };
        Mat4 {
            data: [
                elem(0, 0),
                elem(0, 1),
                elem(0, 2),
                elem(0, 3),
                elem(1, 0),
                elem(1, 1),
                elem(1, 2),
                elem(1, 3),
                elem(2, 0),
                elem(2, 1),
                elem(2, 2),
                elem(2, 3),
                elem(3, 0),
                elem(3, 1),
                elem(3, 2),
                elem(3, 3),
            ],
        }
    }

    /// General 4×4 inverse via the cofactor/adjugate ÷ determinant method.
    ///
    /// Returns `None` when the matrix is singular — when `|det|` does not
    /// exceed [`Scalar::DEFAULT_EPSILON`], or when `det` is non-finite (the
    /// `> ε` test already rejects `NaN`; the explicit finiteness guard also
    /// rejects `±∞`). It is pure `+,-,*,/` arithmetic — no `mul_add`, no
    /// transcendentals — so the result is deterministic and the whole body is
    /// branchless. Storage stays column-major in, column-major out:
    /// `cof[i]` is the cofactor that, divided by `det`, yields inverse
    /// element `i` (the adjugate, i.e. the transpose of the cofactor matrix).
    #[axiom_zones::strict]
    pub fn inverse(&self) -> Option<Mat4> {
        let m = &self.data;
        let cof = [
            m[5] * m[10] * m[15] - m[5] * m[11] * m[14] - m[9] * m[6] * m[15]
                + m[9] * m[7] * m[14]
                + m[13] * m[6] * m[11]
                - m[13] * m[7] * m[10],
            -m[1] * m[10] * m[15] + m[1] * m[11] * m[14] + m[9] * m[2] * m[15]
                - m[9] * m[3] * m[14]
                - m[13] * m[2] * m[11]
                + m[13] * m[3] * m[10],
            m[1] * m[6] * m[15] - m[1] * m[7] * m[14] - m[5] * m[2] * m[15]
                + m[5] * m[3] * m[14]
                + m[13] * m[2] * m[7]
                - m[13] * m[3] * m[6],
            -m[1] * m[6] * m[11] + m[1] * m[7] * m[10] + m[5] * m[2] * m[11]
                - m[5] * m[3] * m[10]
                - m[9] * m[2] * m[7]
                + m[9] * m[3] * m[6],
            -m[4] * m[10] * m[15] + m[4] * m[11] * m[14] + m[8] * m[6] * m[15]
                - m[8] * m[7] * m[14]
                - m[12] * m[6] * m[11]
                + m[12] * m[7] * m[10],
            m[0] * m[10] * m[15] - m[0] * m[11] * m[14] - m[8] * m[2] * m[15]
                + m[8] * m[3] * m[14]
                + m[12] * m[2] * m[11]
                - m[12] * m[3] * m[10],
            -m[0] * m[6] * m[15] + m[0] * m[7] * m[14] + m[4] * m[2] * m[15]
                - m[4] * m[3] * m[14]
                - m[12] * m[2] * m[7]
                + m[12] * m[3] * m[6],
            m[0] * m[6] * m[11] - m[0] * m[7] * m[10] - m[4] * m[2] * m[11]
                + m[4] * m[3] * m[10]
                + m[8] * m[2] * m[7]
                - m[8] * m[3] * m[6],
            m[4] * m[9] * m[15] - m[4] * m[11] * m[13] - m[8] * m[5] * m[15]
                + m[8] * m[7] * m[13]
                + m[12] * m[5] * m[11]
                - m[12] * m[7] * m[9],
            -m[0] * m[9] * m[15] + m[0] * m[11] * m[13] + m[8] * m[1] * m[15]
                - m[8] * m[3] * m[13]
                - m[12] * m[1] * m[11]
                + m[12] * m[3] * m[9],
            m[0] * m[5] * m[15] - m[0] * m[7] * m[13] - m[4] * m[1] * m[15]
                + m[4] * m[3] * m[13]
                + m[12] * m[1] * m[7]
                - m[12] * m[3] * m[5],
            -m[0] * m[5] * m[11] + m[0] * m[7] * m[9] + m[4] * m[1] * m[11]
                - m[4] * m[3] * m[9]
                - m[8] * m[1] * m[7]
                + m[8] * m[3] * m[5],
            -m[4] * m[9] * m[14] + m[4] * m[10] * m[13] + m[8] * m[5] * m[14]
                - m[8] * m[6] * m[13]
                - m[12] * m[5] * m[10]
                + m[12] * m[6] * m[9],
            m[0] * m[9] * m[14] - m[0] * m[10] * m[13] - m[8] * m[1] * m[14]
                + m[8] * m[2] * m[13]
                + m[12] * m[1] * m[10]
                - m[12] * m[2] * m[9],
            -m[0] * m[5] * m[14] + m[0] * m[6] * m[13] + m[4] * m[1] * m[14]
                - m[4] * m[2] * m[13]
                - m[12] * m[1] * m[6]
                + m[12] * m[2] * m[5],
            m[0] * m[5] * m[10] - m[0] * m[6] * m[9] - m[4] * m[1] * m[10]
                + m[4] * m[2] * m[9]
                + m[8] * m[1] * m[6]
                - m[8] * m[2] * m[5],
        ];
        // Laplace expansion of the determinant along the first row, reusing the
        // cofactors already computed for the adjugate.
        let det = m[0] * cof[0] + m[1] * cof[4] + m[2] * cof[8] + m[3] * cof[12];
        (det.is_finite() & (det.abs() > Scalar::DEFAULT_EPSILON)).then(|| {
            let inv_det = 1.0 / det;
            Mat4 {
                data: cof.map(|c| c * inv_det),
            }
        })
    }

    /// Multiply a column vector by this matrix.
    pub fn transform_vec4(&self, v: Vec4) -> Vec4 {
        let c0 = Vec4::new(self.data[0], self.data[1], self.data[2], self.data[3]);
        let c1 = Vec4::new(self.data[4], self.data[5], self.data[6], self.data[7]);
        let c2 = Vec4::new(self.data[8], self.data[9], self.data[10], self.data[11]);
        let c3 = Vec4::new(self.data[12], self.data[13], self.data[14], self.data[15]);
        c0.mul_scalar(v.x)
            .add(c1.mul_scalar(v.y))
            .add(c2.mul_scalar(v.z))
            .add(c3.mul_scalar(v.w))
    }

    /// Transform a 3D point (homogeneous `w = 1`), performing a perspective
    /// divide if `w'` is non-zero.
    pub fn transform_point(&self, p: Vec3) -> Vec3 {
        let v = self.transform_vec4(Vec4::new(p.x, p.y, p.z, 1.0));
        let needs_divide = (v.w != 0.0) & (v.w != 1.0);
        // Select the divisor branchlessly: 1.0 leaves the components unchanged
        // (the affine branch), v.w performs the perspective divide. Dividing by
        // 1.0 is always finite, so this is safe for the non-divide case.
        let divisor = [1.0, v.w][usize::from(needs_divide)];
        Vec3::new(v.x / divisor, v.y / divisor, v.z / divisor)
    }

    /// Transform a 3D direction (homogeneous `w = 0`). Translation has no
    /// effect, as expected for a direction.
    pub fn transform_vector(&self, d: Vec3) -> Vec3 {
        let v = self.transform_vec4(Vec4::new(d.x, d.y, d.z, 0.0));
        Vec3::new(v.x, v.y, v.z)
    }

    /// Append the 16 column-major `f32` elements in order.
    pub fn write_to(self, writer: &mut BinaryWriter) {
        self.data
            .into_iter()
            .for_each(|elem| writer.write_f32(elem));
    }

    /// Read 16 column-major `f32` elements in order.
    pub fn read_from(reader: &mut BinaryReader<'_>) -> KernelResult<Mat4> {
        let mut data = [0.0f32; 16];
        data.iter_mut()
            .try_for_each(|slot| reader.read_f32().map(|v| *slot = v))
            .map(|()| Mat4 { data })
    }
}

impl ApproxEq for Mat4 {
    fn approx_eq(&self, other: &Self, epsilon: Epsilon) -> bool {
        self.data
            .iter()
            .zip(other.data.iter())
            .all(|(a, b)| a.approx_eq(b, epsilon))
    }
}

impl Reflect for Mat4 {
    const SCHEMA: TypeSchema = TypeSchema::new("Mat4", &[FieldSchema::new("data", "[f32; 16]")]);

    fn reflect_write(&self, writer: &mut BinaryWriter) {
        self.data
            .into_iter()
            .for_each(|elem| elem.reflect_write(writer));
    }

    fn reflect_read(reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
        let mut data = [0.0f32; 16];
        data.iter_mut()
            .try_for_each(|slot| f32::reflect_read(reader).map(|v| *slot = v))
            .map(|()| Mat4 { data })
    }
}

#[cfg(test)]
mod reflect_tests {
    use super::*;

    #[test]
    fn reflect_round_trips_describes_and_rejects_truncation() {
        let m = Mat4::IDENTITY;
        let mut w = BinaryWriter::new();
        m.reflect_write(&mut w);
        let bytes = w.into_bytes();
        assert_eq!(
            Mat4::reflect_read(&mut BinaryReader::new(&bytes)).unwrap(),
            m
        );
        for len in 0..bytes.len() {
            assert!(Mat4::reflect_read(&mut BinaryReader::new(&bytes[..len])).is_err());
        }
        assert_eq!(<Mat4 as Reflect>::SCHEMA.name(), "Mat4");
        assert_eq!(<Mat4 as Reflect>::SCHEMA.fields().len(), 1);
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
    fn identity_leaves_points_and_vectors_unchanged() {
        let p = Vec3::new(1.0, 2.0, 3.0);
        let d = Vec3::new(-0.5, 0.25, 1.0);
        assert!(Mat4::IDENTITY.transform_point(p).approx_eq(&p, eps()));
        assert!(Mat4::IDENTITY.transform_vector(d).approx_eq(&d, eps()));
    }

    #[test]
    fn translation_moves_points_but_not_vectors() {
        let m = Mat4::translation(Vec3::new(10.0, 20.0, 30.0));
        let p = Vec3::new(1.0, 2.0, 3.0);
        let d = Vec3::new(1.0, 2.0, 3.0);
        assert!(m
            .transform_point(p)
            .approx_eq(&Vec3::new(11.0, 22.0, 33.0), eps()));
        assert!(m.transform_vector(d).approx_eq(&d, eps()));
    }

    #[test]
    fn scale_scales_points_component_wise() {
        let m = Mat4::scale(Vec3::new(2.0, 3.0, 4.0));
        let p = Vec3::new(1.0, 1.0, 1.0);
        assert!(m
            .transform_point(p)
            .approx_eq(&Vec3::new(2.0, 3.0, 4.0), eps()));
    }

    #[test]
    fn rotation_matches_quaternion_rotation() {
        let q = Quat::from_axis_angle(Vec3::UNIT_Z, std::f32::consts::FRAC_PI_2).unwrap();
        let m = Mat4::from_quaternion(q);
        let v = Vec3::new(0.5, 0.7, -0.3);
        assert!(m.transform_vector(v).approx_eq(&q.rotate(v), eps()));
    }

    #[test]
    fn multiply_order_is_deterministic_and_translation_then_scale_is_not_scale_then_translation() {
        let t = Mat4::translation(Vec3::new(1.0, 0.0, 0.0));
        let s = Mat4::scale(Vec3::new(2.0, 2.0, 2.0));
        // Apply scale first, then translation: point at origin should land at (1,0,0).
        let ts = t.multiply(s);
        assert!(ts
            .transform_point(Vec3::ZERO)
            .approx_eq(&Vec3::new(1.0, 0.0, 0.0), eps()));
        // Apply translation first, then scale: (0,0,0) -> (1,0,0) -> (2,0,0).
        let st = s.multiply(t);
        assert!(st
            .transform_point(Vec3::ZERO)
            .approx_eq(&Vec3::new(2.0, 0.0, 0.0), eps()));
    }

    #[test]
    fn identity_is_neutral_under_multiply() {
        let m = Mat4::translation(Vec3::new(1.0, 2.0, 3.0));
        assert!(m.multiply(Mat4::IDENTITY).approx_eq(&m, eps()));
        assert!(Mat4::IDENTITY.multiply(m).approx_eq(&m, eps()));
    }

    #[test]
    fn perspective_has_stable_expected_values() {
        let m = Mat4::perspective(std::f32::consts::FRAC_PI_2, 2.0, 1.0, 100.0).unwrap();
        // f = 1/tan(45deg) = 1.0; f/aspect = 0.5
        let cols = m.as_cols_array();
        assert!(cols[0].approx_eq(&0.5, eps()));
        assert!(cols[5].approx_eq(&1.0, eps()));
        assert!(cols[11].approx_eq(&-1.0, eps()));
        assert!(cols[15].approx_eq(&0.0, eps()));
    }

    #[test]
    fn perspective_rejects_bad_inputs() {
        assert_eq!(
            Mat4::perspective(0.0, 1.0, 1.0, 10.0).unwrap_err().code(),
            MathErrorCode::InvalidMatrixOperation
        );
        assert_eq!(
            Mat4::perspective(1.0, 0.0, 1.0, 10.0).unwrap_err().code(),
            MathErrorCode::InvalidMatrixOperation
        );
        assert_eq!(
            Mat4::perspective(1.0, 1.0, 10.0, 1.0).unwrap_err().code(),
            MathErrorCode::InvalidMatrixOperation
        );
        assert_eq!(
            Mat4::perspective(1.0, 1.0, 1.0, f32::NAN)
                .unwrap_err()
                .code(),
            MathErrorCode::InvalidMatrixOperation
        );
    }

    #[test]
    fn orthographic_has_stable_expected_values() {
        let m = Mat4::orthographic(-1.0, 1.0, -1.0, 1.0, 0.0, 1.0).unwrap();
        // Should map (0,0,0) -> (0,0,-1) in NDC.
        let p = m.transform_point(Vec3::new(0.0, 0.0, 0.0));
        assert!(p.approx_eq(&Vec3::new(0.0, 0.0, -1.0), eps()));
        // And (1,1,0) -> (1, 1, -1).
        let p2 = m.transform_point(Vec3::new(1.0, 1.0, 0.0));
        assert!(p2.approx_eq(&Vec3::new(1.0, 1.0, -1.0), eps()));
    }

    #[test]
    fn orthographic_rejects_bad_inputs() {
        assert_eq!(
            Mat4::orthographic(1.0, 0.0, -1.0, 1.0, 0.0, 1.0)
                .unwrap_err()
                .code(),
            MathErrorCode::InvalidMatrixOperation
        );
        assert_eq!(
            Mat4::orthographic(0.0, 1.0, 0.0, 0.0, 0.0, 1.0)
                .unwrap_err()
                .code(),
            MathErrorCode::InvalidMatrixOperation
        );
        assert_eq!(
            Mat4::orthographic(0.0, 1.0, 0.0, 1.0, 1.0, 0.0)
                .unwrap_err()
                .code(),
            MathErrorCode::InvalidMatrixOperation
        );
    }

    #[test]
    fn look_at_produces_stable_basis() {
        // Camera at (0,0,5), looking at origin, up = +Y. Forward axis in view
        // space is +Z (because RH look_at negates the world forward).
        let m = Mat4::look_at(
            Vec3::new(0.0, 0.0, 5.0),
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        )
        .unwrap();
        // The world origin sits 5 units in front of the camera; transformed to
        // view space it lands at (0, 0, -5).
        let view_origin = m.transform_point(Vec3::ZERO);
        assert!(view_origin.approx_eq(&Vec3::new(0.0, 0.0, -5.0), eps()));
        // Right axis (world +X) stays +X in view space.
        let view_right = m.transform_vector(Vec3::UNIT_X);
        assert!(view_right.approx_eq(&Vec3::UNIT_X, eps()));
    }

    #[test]
    fn look_at_rejects_degenerate_inputs() {
        // eye == target.
        assert_eq!(
            Mat4::look_at(Vec3::ZERO, Vec3::ZERO, Vec3::UNIT_Y)
                .unwrap_err()
                .code(),
            MathErrorCode::InvalidMatrixOperation
        );
        // forward parallel to up.
        assert_eq!(
            Mat4::look_at(Vec3::ZERO, Vec3::new(0.0, 1.0, 0.0), Vec3::UNIT_Y)
                .unwrap_err()
                .code(),
            MathErrorCode::InvalidMatrixOperation
        );
    }

    #[test]
    fn binary_round_trip_preserves_layout() {
        let api = KernelApi::new();
        let m = Mat4::translation(Vec3::new(1.0, 2.0, 3.0))
            .multiply(Mat4::scale(Vec3::new(2.0, 0.5, 4.0)));
        let mut writer = api.binary_writer();
        m.write_to(&mut writer);
        let bytes = writer.into_bytes();
        let mut reader = api.binary_reader(&bytes);
        let back = Mat4::read_from(&mut reader).unwrap();
        assert!(back.approx_eq(&m, eps()));
    }

    #[test]
    fn from_and_as_cols_array_round_trip() {
        let raw = [
            1.0, 2.0, 3.0, 4.0, //
            5.0, 6.0, 7.0, 8.0, //
            9.0, 10.0, 11.0, 12.0, //
            13.0, 14.0, 15.0, 16.0, //
        ];
        assert_eq!(Mat4::from_cols_array(raw).as_cols_array(), raw);
    }

    #[test]
    fn zero_matrix_is_all_zero() {
        let cols = Mat4::ZERO.as_cols_array();
        assert!(cols.iter().all(|c| *c == 0.0));
    }

    #[test]
    fn transform_vec4_handles_homogeneous_components() {
        let m = Mat4::translation(Vec3::new(1.0, 2.0, 3.0));
        let v = m.transform_vec4(Vec4::new(0.0, 0.0, 0.0, 1.0));
        assert!(v.approx_eq(&Vec4::new(1.0, 2.0, 3.0, 1.0), eps()));
        // w=0 — translation has no effect.
        let d = m.transform_vec4(Vec4::new(1.0, 0.0, 0.0, 0.0));
        assert!(d.approx_eq(&Vec4::new(1.0, 0.0, 0.0, 0.0), eps()));
    }
}

#[cfg(test)]
mod cov {
    use super::*;
    use axiom_kernel::BinaryReader;

    #[test]
    fn perspective_rejects_non_finite_each() {
        assert!(Mat4::perspective(f32::NAN, 1.0, 1.0, 2.0).is_err());
        assert!(Mat4::perspective(1.0, f32::NAN, 1.0, 2.0).is_err());
        assert!(Mat4::perspective(1.0, 1.0, f32::NAN, 2.0).is_err());
        assert!(Mat4::perspective(1.0, 1.0, 1.0, f32::NAN).is_err());
    }

    #[test]
    fn perspective_rejects_fovy_bounds() {
        assert!(Mat4::perspective(0.0, 1.0, 1.0, 2.0).is_err());
        assert!(Mat4::perspective(std::f32::consts::PI, 1.0, 1.0, 2.0).is_err());
    }

    #[test]
    fn perspective_rejects_aspect_near_far() {
        assert!(Mat4::perspective(1.0, 0.0, 1.0, 2.0).is_err());
        assert!(Mat4::perspective(1.0, 1.0, 0.0, 2.0).is_err());
        assert!(Mat4::perspective(1.0, 1.0, 2.0, 1.0).is_err());
    }

    #[test]
    fn perspective_accepts_valid() {
        assert!(Mat4::perspective(1.0, 1.5, 0.1, 100.0).is_ok());
    }

    #[test]
    fn orthographic_rejects_non_finite() {
        assert!(Mat4::orthographic(f32::NAN, 1.0, 0.0, 1.0, 0.0, 1.0).is_err());
    }

    #[test]
    fn orthographic_accepts_valid() {
        assert!(Mat4::orthographic(-1.0, 1.0, -1.0, 1.0, 0.1, 10.0).is_ok());
    }

    #[test]
    fn transform_point_perspective_divide_and_affine() {
        // Perspective matrix yields w' != 0 and != 1 -> divide branch.
        let p = Mat4::perspective(1.0, 1.0, 0.1, 100.0).unwrap();
        let _ = p.transform_point(Vec3::new(1.0, 1.0, -5.0));
        // Identity yields w' == 1 -> affine branch.
        let i = Mat4::IDENTITY.transform_point(Vec3::new(2.0, 3.0, 4.0));
        assert!(i.approx_eq(&Vec3::new(2.0, 3.0, 4.0), Epsilon::DEFAULT));
    }

    #[test]
    fn transform_point_zero_w_takes_affine_branch() {
        // Fourth row all zeros -> w' == 0, so the perspective divide is skipped.
        let m = Mat4::from_cols_array([
            1.0, 0.0, 0.0, 0.0, //
            0.0, 1.0, 0.0, 0.0, //
            0.0, 0.0, 1.0, 0.0, //
            0.0, 0.0, 0.0, 0.0, //
        ]);
        let r = m.transform_point(Vec3::new(2.0, 3.0, 4.0));
        assert!(r.approx_eq(&Vec3::new(2.0, 3.0, 4.0), Epsilon::DEFAULT));
    }

    #[test]
    fn read_from_truncated_errors() {
        let mut r = BinaryReader::new(&[0u8; 4]);
        assert!(Mat4::read_from(&mut r).is_err());
    }

    #[test]
    fn approx_eq_detects_difference() {
        let a = Mat4::IDENTITY;
        let mut data = a.as_cols_array();
        data[5] = 99.0;
        let b = Mat4::from_cols_array(data);
        assert!(!a.approx_eq(&b, Epsilon::DEFAULT));
        assert!(a.approx_eq(&Mat4::IDENTITY, Epsilon::DEFAULT));
    }

    fn eps5() -> Epsilon {
        Epsilon::new(1.0e-5).unwrap()
    }

    // Kills from_quaternion mutants at 71 (`x*x`), 88 (`yz + wx`), 92
    // (`yz - wx`). Uses q = (0.5,0.5,0.5,0.5) (a genuine unit quaternion) where
    // each squared/cross term is a distinct 0.25, so the mutated forms produce
    // different, checkable matrix elements.
    #[test]
    fn from_quaternion_matrix_elements_are_exact() {
        let q = Quat::new(0.5, 0.5, 0.5, 0.5);
        let m = Mat4::from_quaternion(q).as_cols_array();
        // index5 = 1 - 2*(xx + zz); with xx=x*x=0.25 this is 0.0.
        // Mutant `x + x = 1.0` makes it 1 - 2*1.25 = -1.5.
        assert!(m[5].approx_eq(&0.0, eps5()));
        // index6 = 2*(yz + wx) = 2*(0.25+0.25) = 1.0.
        assert!(m[6].approx_eq(&1.0, eps5()));
        // index9 = 2*(yz - wx) = 0.0; mutant `+` gives 1.0.
        assert!(m[9].approx_eq(&0.0, eps5()));
        // index10 = 1 - 2*(xx + yy) = 0.0 (also pins the `x*x` term).
        assert!(m[10].approx_eq(&0.0, eps5()));
    }

    // Kills perspective mutants at 133 (`1.0 / tan`) and 153 (`2.0 * far`).
    // FRAC_PI_2 makes tan == 1 which hides the 133 divide, so use FRAC_PI_3.
    #[test]
    fn perspective_focal_and_depth_terms_are_exact() {
        let fovy = std::f32::consts::FRAC_PI_3; // 60 deg; tan(30 deg) = 1/sqrt(3)
                                                // near = 2 (NOT 1) so that `far * near` differs from `far / near`,
                                                // killing the 153:23 (`*` -> `/`) mutant; and `far - near` differs from
                                                // `far + near` for the depth terms.
        let near = 2.0f32;
        let far = 100.0f32;
        let m = Mat4::perspective(fovy, 1.0, near, far).unwrap();
        let cols = m.as_cols_array();
        // f = 1/tan(30deg) = sqrt(3) ~= 1.7320508; f/aspect with aspect 1.
        // Mutant 133 (`/` -> `*`) gives f = tan(30deg) ~= 0.57735.
        assert!(cols[0].approx_eq(&3.0f32.sqrt(), eps5()));
        let nf = 1.0f32 / (near - far); // = -1/98
                                        // col[10] = (far+near)*nf = 102 * nf.
        assert!(cols[10].approx_eq(&((far + near) * nf), eps5()));
        // col[14] = 2*far*near*nf. With near=2: 2*100*2*nf = 400*nf.
        // Mutant 153 (`far / near`) gives 2*(100/2)*nf = 100*nf, distinct.
        assert!(cols[14].approx_eq(&(2.0 * far * near * nf), eps5()));
    }

    // Kills every orthographic mutant: 183 (`far - near`), 197 (`-2.0/fn_`
    // delete `-`, `/`->`%`/`*`), 200/201 (translation column delete `-`,
    // `/`->`%`/`*`), 202 (`far + near` -> `-`, `/`->`*`). Asymmetric bounds make
    // each element distinct from the mutated alternatives.
    #[test]
    fn orthographic_matrix_elements_are_exact() {
        let m = Mat4::orthographic(2.0, 6.0, 1.0, 5.0, 3.0, 9.0).unwrap();
        let c = m.as_cols_array();
        // rl=4, tb=4, fn_=6.
        assert!(c[0].approx_eq(&0.5, eps5())); // 2/rl
        assert!(c[5].approx_eq(&0.5, eps5())); // 2/tb
        assert!(c[10].approx_eq(&(-2.0 / 6.0), eps5())); // -2/fn_
        assert!(c[12].approx_eq(&-2.0, eps5())); // -(right+left)/rl = -8/4
        assert!(c[13].approx_eq(&-1.5, eps5())); // -(top+bottom)/tb = -6/4
        assert!(c[14].approx_eq(&-2.0, eps5())); // -(far+near)/fn_ = -12/6
    }

    // Kills the look_at `delete -` mutants at 220..=224. f, s, u and the eye
    // dots are recomputed independently and the matrix's third row / fourth
    // column are pinned to the negated values production must store.
    #[test]
    fn look_at_negation_terms_are_exact() {
        let eye = Vec3::new(3.0, 4.0, 5.0);
        let target = Vec3::new(0.0, 1.0, -2.0);
        let up = Vec3::UNIT_Y;
        let m = Mat4::look_at(eye, target, up).unwrap().as_cols_array();

        let f = target.subtract(eye).normalize().unwrap();
        let s = f.cross(up).normalize().unwrap();
        let u = s.cross(f);

        // Third row holds -f (indices 2, 6, 10).
        assert!(m[2].approx_eq(&(-f.x), eps5()));
        assert!(m[6].approx_eq(&(-f.y), eps5()));
        assert!(m[10].approx_eq(&(-f.z), eps5()));
        // Fourth column holds -s.dot(eye), -u.dot(eye), f.dot(eye).
        assert!(m[12].approx_eq(&(-s.dot(eye)), eps5()));
        assert!(m[13].approx_eq(&(-u.dot(eye)), eps5()));
        assert!(m[14].approx_eq(&(f.dot(eye)), eps5()));
        // Guard the values are actually non-trivial (so deletes change them).
        assert!(f.z.abs() > 1.0e-3);
        assert!(s.dot(eye).abs() > 1.0e-3);
        assert!(u.dot(eye).abs() > 1.0e-3);
    }

    // Kills transform_point mutants at 261 (`v.w != 1.0` -> `==`) and 262
    // (the three perspective divides `/` -> `%` / `*`). A matrix that forces
    // w' == 2 (not 0, not 1) must take the divide branch and divide by 2.
    #[test]
    fn transform_point_perspective_divide_by_two() {
        let m = Mat4::from_cols_array([
            1.0, 0.0, 0.0, 0.0, //
            0.0, 1.0, 0.0, 0.0, //
            0.0, 0.0, 1.0, 0.0, //
            0.0, 0.0, 0.0, 2.0, // w' = 2 for any input point
        ]);
        let r = m.transform_point(Vec3::new(3.0, 7.0, 9.0));
        assert!(r.approx_eq(&Vec3::new(1.5, 3.5, 4.5), eps5()));
    }

    #[test]
    fn inverse_of_identity_is_identity() {
        let inv = Mat4::IDENTITY.inverse().unwrap();
        assert!(inv.approx_eq(&Mat4::IDENTITY, eps5()));
    }

    #[test]
    fn inverse_of_translation_is_negated_translation() {
        let t = Vec3::new(10.0, -20.0, 30.0);
        let m = Mat4::translation(t);
        let inv = m.inverse().unwrap();
        // The inverse of a pure translation is the translation by -t.
        assert!(inv.approx_eq(&Mat4::translation(t.mul_scalar(-1.0)), eps5()));
        // And it round-trips a point back to itself.
        let p = Vec3::new(1.0, 2.0, 3.0);
        assert!(inv.transform_point(m.transform_point(p)).approx_eq(&p, eps5()));
    }

    #[test]
    fn inverse_of_general_matrix_times_self_is_identity() {
        // A general invertible affine matrix: scale, then rotate, then
        // translate (det = 2*3*4 = 24, comfortably non-singular).
        let q = Quat::from_axis_angle(Vec3::new(1.0, 2.0, 3.0), 0.9).unwrap();
        let m = Mat4::translation(Vec3::new(5.0, -3.0, 2.0))
            .multiply(Mat4::from_quaternion(q))
            .multiply(Mat4::scale(Vec3::new(2.0, 3.0, 4.0)));
        let inv = m.inverse().unwrap();
        // M * M⁻¹ ≈ I and M⁻¹ * M ≈ I.
        assert!(m.multiply(inv).approx_eq(&Mat4::IDENTITY, eps5()));
        assert!(inv.multiply(m).approx_eq(&Mat4::IDENTITY, eps5()));
    }

    #[test]
    fn inverse_of_singular_matrix_is_none() {
        // Zero matrix: det = 0.
        assert!(Mat4::ZERO.inverse().is_none());
        // Rank-deficient: two identical columns force det = 0.
        let rank_deficient = Mat4::from_cols_array([
            1.0, 2.0, 3.0, 4.0, //
            1.0, 2.0, 3.0, 4.0, // duplicate of column 0
            9.0, 8.0, 7.0, 6.0, //
            0.0, 1.0, 0.0, 1.0, //
        ]);
        assert!(rank_deficient.inverse().is_none());
    }

    #[test]
    fn inverse_of_non_finite_matrix_is_none() {
        // A NaN entry propagates into det, which is then non-finite; the
        // finiteness guard (and the `> ε` test) rejects it.
        let m = Mat4::from_cols_array([
            f32::NAN, 0.0, 0.0, 0.0, //
            0.0, 1.0, 0.0, 0.0, //
            0.0, 0.0, 1.0, 0.0, //
            0.0, 0.0, 0.0, 1.0, //
        ]);
        assert!(m.inverse().is_none());
    }
}
