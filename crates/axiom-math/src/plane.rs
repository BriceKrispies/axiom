//! An oriented infinite plane.

use crate::approx_eq::ApproxEq;
use crate::epsilon::Epsilon;
use crate::math_error::MathError;
use crate::math_result::MathResult;
use crate::plane_side::PlaneSide;
use crate::vec3::Vec3;

/// A plane in normal-and-`d` form: `normal · p + d == 0`.
///
/// `normal` is stored as a unit vector. `signed_distance_to_point(p) > 0`
/// places `p` in the half-space the normal points into.
#[derive(Debug, Clone, Copy)]
pub struct Plane {
    normal: Vec3,
    d: f32,
}

impl Plane {
    /// Construct from a non-zero, finite normal and a finite plane offset.
    /// The normal is normalized on the way in.
    pub fn new(normal: Vec3, d: f32) -> MathResult<Plane> {
        if !d.is_finite() {
            return Err(MathError::non_finite_scalar(
                "plane offset d must be finite",
            ));
        }
        for component in [normal.x, normal.y, normal.z] {
            if !component.is_finite() {
                return Err(MathError::non_finite_scalar(
                    "plane normal components must be finite",
                ));
            }
        }
        let len = normal.length();
        if len == 0.0 {
            return Err(MathError::normalize_zero_length(
                "plane normal must be non-zero",
            ));
        }
        Ok(Plane {
            normal: Vec3::new(normal.x / len, normal.y / len, normal.z / len),
            d: d / len,
        })
    }

    /// Unit normal.
    pub const fn normal(&self) -> Vec3 {
        self.normal
    }

    /// Plane offset.
    pub const fn distance(&self) -> f32 {
        self.d
    }

    /// Signed distance: positive on the side `normal` points to.
    pub fn signed_distance_to_point(&self, p: Vec3) -> f32 {
        self.normal.dot(p) + self.d
    }

    /// Classify a point against the plane using the supplied tolerance.
    pub fn classify_point(&self, p: Vec3, epsilon: Epsilon) -> PlaneSide {
        let s = self.signed_distance_to_point(p);
        if s > epsilon.value() {
            PlaneSide::Front
        } else if s < -epsilon.value() {
            PlaneSide::Back
        } else {
            PlaneSide::On
        }
    }
}

impl ApproxEq for Plane {
    fn approx_eq(&self, other: &Self, epsilon: Epsilon) -> bool {
        self.normal.approx_eq(&other.normal, epsilon) && self.d.approx_eq(&other.d, epsilon)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math_error_code::MathErrorCode;

    fn eps() -> Epsilon {
        Epsilon::new(1.0e-5).unwrap()
    }

    #[test]
    fn new_normalizes_normal_and_scales_d() {
        let p = Plane::new(Vec3::new(0.0, 0.0, 2.0), -4.0).unwrap();
        assert!(p.normal().approx_eq(&Vec3::UNIT_Z, eps()));
        // 2.0 scale divides through both: d becomes -2.0.
        assert!(p.distance().approx_eq(&-2.0, eps()));
    }

    #[test]
    fn new_rejects_zero_normal() {
        let err = Plane::new(Vec3::ZERO, 0.0).unwrap_err();
        assert_eq!(err.code(), MathErrorCode::NormalizeZeroLength);
    }

    #[test]
    fn new_rejects_non_finite() {
        assert_eq!(
            Plane::new(Vec3::new(f32::NAN, 0.0, 1.0), 0.0)
                .unwrap_err()
                .code(),
            MathErrorCode::NonFiniteScalar
        );
        assert_eq!(
            Plane::new(Vec3::UNIT_Z, f32::INFINITY).unwrap_err().code(),
            MathErrorCode::NonFiniteScalar
        );
    }

    #[test]
    fn signed_distance_uses_normalized_form() {
        // Plane z = 2 (normal +Z, d = -2).
        let p = Plane::new(Vec3::UNIT_Z, -2.0).unwrap();
        assert!(p
            .signed_distance_to_point(Vec3::new(0.0, 0.0, 5.0))
            .approx_eq(&3.0, eps()));
        assert!(p
            .signed_distance_to_point(Vec3::new(0.0, 0.0, 0.0))
            .approx_eq(&-2.0, eps()));
    }

    #[test]
    fn classify_point_returns_front_back_on() {
        let p = Plane::new(Vec3::UNIT_Z, 0.0).unwrap();
        assert_eq!(
            p.classify_point(Vec3::new(0.0, 0.0, 1.0), eps()),
            PlaneSide::Front
        );
        assert_eq!(
            p.classify_point(Vec3::new(0.0, 0.0, -1.0), eps()),
            PlaneSide::Back
        );
        assert_eq!(p.classify_point(Vec3::ZERO, eps()), PlaneSide::On);
    }

    #[test]
    fn approx_eq_compares_components() {
        let a = Plane::new(Vec3::UNIT_Z, 1.0).unwrap();
        let b = Plane::new(Vec3::UNIT_Z, 1.0).unwrap();
        let c = Plane::new(Vec3::UNIT_X, 1.0).unwrap();
        assert!(a.approx_eq(&b, eps()));
        assert!(!a.approx_eq(&c, eps()));
    }

    // Kills classify_point boundary mutants at 66 (`s > eps` -> `>=`) and 68
    // (`s < -eps` -> `<=`). With the z = ±eps points sitting EXACTLY on the
    // tolerance boundary, the correct strict comparisons classify them `On`
    // while the mutated `>=` / `<=` would classify them `Front` / `Back`.
    #[test]
    fn classify_point_at_exact_epsilon_boundary_is_on() {
        let p = Plane::new(Vec3::UNIT_Z, 0.0).unwrap();
        let e = Epsilon::new(0.25).unwrap(); // 0.25 is exactly representable
        // signed distance == +0.25 == eps -> On (not Front).
        assert_eq!(p.classify_point(Vec3::new(0.0, 0.0, 0.25), e), PlaneSide::On);
        // signed distance == -0.25 == -eps -> On (not Back).
        assert_eq!(p.classify_point(Vec3::new(0.0, 0.0, -0.25), e), PlaneSide::On);
        // Just past the boundary still resolves Front / Back.
        assert_eq!(p.classify_point(Vec3::new(0.0, 0.0, 0.5), e), PlaneSide::Front);
        assert_eq!(p.classify_point(Vec3::new(0.0, 0.0, -0.5), e), PlaneSide::Back);
    }
}
