//! A 3×3 column-major float matrix — the 2D affine transform primitive.

use axiom_kernel::Radians;

use crate::approx_eq::ApproxEq;
use crate::epsilon::Epsilon;
use crate::vec2::Vec2;

/// A column-major 3×3 `f32` matrix.
///
/// The dimensionless 2D-transform peer of [`crate::Mat4`]: it carries a 2D
/// affine transform (translation + linear part) as the `3×3` homogeneous matrix
/// `[ a c tx ; b d ty ; 0 0 1 ]`, stored column-major as `data[col*3 + row]`.
/// Multiplication uses the same convention as `Mat4`: `(A * B) * v == A * (B *
/// v)`, so the rightmost factor is applied first when transforming a point.
///
/// It is the value the 2D draw surface bakes onto every command from its
/// transform stack; the surface composes these (`multiply`) and the backend
/// applies them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mat3 {
    data: [f32; 9],
}

impl Mat3 {
    /// Zero matrix.
    pub const ZERO: Mat3 = Mat3 { data: [0.0; 9] };

    /// Identity matrix.
    pub const IDENTITY: Mat3 = Mat3 {
        data: [
            1.0, 0.0, 0.0, //
            0.0, 1.0, 0.0, //
            0.0, 0.0, 1.0, //
        ],
    };

    /// Construct from a column-major `[f32; 9]` raw layout.
    pub const fn from_cols_array(data: [f32; 9]) -> Self {
        Mat3 { data }
    }

    /// The raw column-major array.
    pub const fn as_cols_array(&self) -> [f32; 9] {
        self.data
    }

    /// 2D translation matrix (translation in the third column).
    pub const fn translation(t: Vec2) -> Mat3 {
        Mat3::from_cols_array([
            1.0, 0.0, 0.0, //
            0.0, 1.0, 0.0, //
            t.x, t.y, 1.0, //
        ])
    }

    /// Per-axis 2D scale matrix.
    pub const fn scale(s: Vec2) -> Mat3 {
        Mat3::from_cols_array([
            s.x, 0.0, 0.0, //
            0.0, s.y, 0.0, //
            0.0, 0.0, 1.0, //
        ])
    }

    /// 2D rotation matrix for a counter-clockwise angle.
    pub fn rotation(angle: Radians) -> Mat3 {
        let a = angle.get();
        let (sin, cos) = a.sin_cos();
        Mat3::from_cols_array([
            cos, sin, 0.0, //
            -sin, cos, 0.0, //
            0.0, 0.0, 1.0, //
        ])
    }

    /// Matrix product `self * other`.
    #[axiom_zones::strict]
    pub fn multiply(self, other: Mat3) -> Mat3 {
        let a = &self.data;
        let b = &other.data;
        // Keeps the exact accumulation order `((0 + t0) + t1) + t2` for determinism.
        let elem = |col: usize, row: usize| -> f32 {
            0.0f32 + a[row] * b[col * 3] + a[3 + row] * b[col * 3 + 1] + a[6 + row] * b[col * 3 + 2]
        };
        Mat3 {
            data: [
                elem(0, 0),
                elem(0, 1),
                elem(0, 2),
                elem(1, 0),
                elem(1, 1),
                elem(1, 2),
                elem(2, 0),
                elem(2, 1),
                elem(2, 2),
            ],
        }
    }

    /// Transform a 2D point (homogeneous `w = 1`): applies the linear part and
    /// the translation column. No perspective divide — a 2D affine matrix's
    /// bottom row is `[0 0 1]`.
    pub fn transform_point(&self, p: Vec2) -> Vec2 {
        Vec2::new(
            self.data[0] * p.x + self.data[3] * p.y + self.data[6],
            self.data[1] * p.x + self.data[4] * p.y + self.data[7],
        )
    }

    /// Transform a 2D direction (homogeneous `w = 0`): the linear part only, so
    /// translation has no effect.
    pub fn transform_vector(&self, d: Vec2) -> Vec2 {
        Vec2::new(
            self.data[0] * d.x + self.data[3] * d.y,
            self.data[1] * d.x + self.data[4] * d.y,
        )
    }
}

impl ApproxEq for Mat3 {
    fn approx_eq(&self, other: &Self, epsilon: Epsilon) -> bool {
        self.data
            .iter()
            .zip(other.data.iter())
            .all(|(a, b)| a.approx_eq(b, epsilon))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eps() -> Epsilon {
        Epsilon::new(1.0e-5).unwrap()
    }

    #[test]
    fn identity_leaves_points_and_vectors_unchanged() {
        let p = Vec2::new(3.0, -4.0);
        assert!(Mat3::IDENTITY.transform_point(p).approx_eq(&p, eps()));
        assert!(Mat3::IDENTITY.transform_vector(p).approx_eq(&p, eps()));
    }

    #[test]
    fn translation_moves_points_but_not_vectors() {
        let m = Mat3::translation(Vec2::new(10.0, 20.0));
        assert!(m
            .transform_point(Vec2::new(1.0, 2.0))
            .approx_eq(&Vec2::new(11.0, 22.0), eps()));
        assert!(m
            .transform_vector(Vec2::new(1.0, 2.0))
            .approx_eq(&Vec2::new(1.0, 2.0), eps()));
    }

    #[test]
    fn scale_scales_each_component() {
        let m = Mat3::scale(Vec2::new(2.0, 3.0));
        assert!(m
            .transform_point(Vec2::new(4.0, 5.0))
            .approx_eq(&Vec2::new(8.0, 15.0), eps()));
        assert!(m
            .transform_vector(Vec2::new(4.0, 5.0))
            .approx_eq(&Vec2::new(8.0, 15.0), eps()));
    }

    #[test]
    fn rotation_quarter_turn_maps_x_to_y() {
        let m = Mat3::rotation(Radians::new(std::f32::consts::FRAC_PI_2).unwrap());
        assert!(m
            .transform_vector(Vec2::new(1.0, 0.0))
            .approx_eq(&Vec2::new(0.0, 1.0), eps()));
        assert!(m
            .transform_vector(Vec2::new(0.0, 1.0))
            .approx_eq(&Vec2::new(-1.0, 0.0), eps()));
    }

    #[test]
    fn multiply_applies_right_factor_first() {
        let t = Mat3::translation(Vec2::new(1.0, 0.0));
        let s = Mat3::scale(Vec2::new(2.0, 2.0));
        let ts = t.multiply(s);
        assert!(ts
            .transform_point(Vec2::ZERO)
            .approx_eq(&Vec2::new(1.0, 0.0), eps()));
        let st = s.multiply(t);
        assert!(st
            .transform_point(Vec2::ZERO)
            .approx_eq(&Vec2::new(2.0, 0.0), eps()));
    }

    #[test]
    fn identity_is_neutral_under_multiply() {
        let m = Mat3::translation(Vec2::new(3.0, 4.0)).multiply(Mat3::scale(Vec2::new(2.0, 5.0)));
        assert!(m.multiply(Mat3::IDENTITY).approx_eq(&m, eps()));
        assert!(Mat3::IDENTITY.multiply(m).approx_eq(&m, eps()));
    }

    #[test]
    fn from_and_as_cols_array_round_trip() {
        let raw = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0];
        assert_eq!(Mat3::from_cols_array(raw).as_cols_array(), raw);
    }

    #[test]
    fn zero_matrix_is_all_zero() {
        assert!(Mat3::ZERO.as_cols_array().iter().all(|c| *c == 0.0));
    }

    #[test]
    fn approx_eq_detects_difference() {
        let mut data = Mat3::IDENTITY.as_cols_array();
        data[4] = 99.0;
        assert!(!Mat3::IDENTITY.approx_eq(&Mat3::from_cols_array(data), eps()));
        assert!(Mat3::IDENTITY.approx_eq(&Mat3::IDENTITY, eps()));
    }

    #[test]
    fn multiply_elements_are_exact() {
        let a = Mat3::from_cols_array([1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0]);
        let b = Mat3::from_cols_array([9.0, 8.0, 7.0, 6.0, 5.0, 4.0, 3.0, 2.0, 1.0]);
        let p = a.multiply(b).as_cols_array();
        assert!(p[0].approx_eq(&90.0, eps()));
        assert!(p[1].approx_eq(&114.0, eps()));
        assert!(p[2].approx_eq(&138.0, eps()));
        assert!(p[3].approx_eq(&54.0, eps()));
        assert!(p[6].approx_eq(&18.0, eps()));
    }

    #[test]
    fn transform_point_uses_distinct_coefficients() {
        let m = Mat3::from_cols_array([
            2.0, 3.0, 0.0, //
            4.0, 5.0, 0.0, //
            6.0, 7.0, 1.0, //
        ]);
        let r = m.transform_point(Vec2::new(10.0, 100.0));
        assert!(r.approx_eq(&Vec2::new(426.0, 537.0), eps()));
        let v = m.transform_vector(Vec2::new(10.0, 100.0));
        assert!(v.approx_eq(&Vec2::new(420.0, 530.0), eps()));
    }
}
