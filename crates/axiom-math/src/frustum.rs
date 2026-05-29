//! A view-projection frustum represented as six oriented planes.

use crate::aabb::Aabb;
use crate::approx_eq::ApproxEq;
use crate::epsilon::Epsilon;
use crate::mat4::Mat4;
use crate::math_result::MathResult;
use crate::plane::Plane;
use crate::sphere::Sphere;
use crate::vec3::Vec3;

/// The six oriented planes of a view-projection volume, in order:
/// `[Left, Right, Bottom, Top, Near, Far]`.
///
/// Built via the standard Gribb–Hartmann row-combination method from a clip-
/// from-world matrix (`projection * view`). Each plane's normal points into
/// the frustum interior, so `contains_point` / `intersects_*` reduce to
/// signed-distance sign checks against every plane.
#[derive(Debug, Clone, Copy)]
pub struct Frustum {
    planes: [Plane; 6],
}

impl Frustum {
    /// Build a frustum from a clip-from-world (`projection * view`) matrix.
    /// Fails if any of the six derived planes has a zero-length normal.
    pub fn from_view_projection(clip_from_world: Mat4) -> MathResult<Frustum> {
        let m = clip_from_world.as_cols_array();
        // Row r of the row-major form is `(m[0*4+r], m[1*4+r], m[2*4+r], m[3*4+r])`.
        let r = |row: usize| -> (f32, f32, f32, f32) {
            (m[row], m[4 + row], m[8 + row], m[12 + row])
        };
        let row0 = r(0);
        let row1 = r(1);
        let row2 = r(2);
        let row3 = r(3);

        fn plane_from(row: (f32, f32, f32, f32), sign: f32, base: (f32, f32, f32, f32)) -> (Vec3, f32) {
            (
                Vec3::new(base.0 + sign * row.0, base.1 + sign * row.1, base.2 + sign * row.2),
                base.3 + sign * row.3,
            )
        }

        let (ln, ld) = plane_from(row0, 1.0, row3);
        let (rn, rd) = plane_from(row0, -1.0, row3);
        let (bn, bd) = plane_from(row1, 1.0, row3);
        let (tn, td) = plane_from(row1, -1.0, row3);
        let (nn, nd) = plane_from(row2, 1.0, row3);
        let (fn_, fd) = plane_from(row2, -1.0, row3);

        Ok(Frustum {
            planes: [
                Plane::new(ln, ld)?,
                Plane::new(rn, rd)?,
                Plane::new(bn, bd)?,
                Plane::new(tn, td)?,
                Plane::new(nn, nd)?,
                Plane::new(fn_, fd)?,
            ],
        })
    }

    /// The six planes, in `[Left, Right, Bottom, Top, Near, Far]` order.
    pub const fn planes(&self) -> &[Plane; 6] {
        &self.planes
    }

    /// Whether `p` is on the interior side of every plane.
    pub fn contains_point(&self, p: Vec3) -> bool {
        self.planes
            .iter()
            .all(|plane| plane.signed_distance_to_point(p) >= 0.0)
    }

    /// Whether `aabb` is not fully outside any plane. Conservative: may return
    /// `true` for boxes that touch the frustum only through their bounding
    /// volume but not their interior — this is the standard culling test.
    pub fn intersects_aabb(&self, aabb: &Aabb) -> bool {
        for plane in &self.planes {
            let n = plane.normal();
            // p-vertex: corner that maximizes signed distance.
            let p_vertex = Vec3::new(
                if n.x >= 0.0 { aabb.max().x } else { aabb.min().x },
                if n.y >= 0.0 { aabb.max().y } else { aabb.min().y },
                if n.z >= 0.0 { aabb.max().z } else { aabb.min().z },
            );
            if plane.signed_distance_to_point(p_vertex) < 0.0 {
                return false;
            }
        }
        true
    }

    /// Whether the sphere is not fully outside any plane.
    pub fn intersects_sphere(&self, sphere: &Sphere) -> bool {
        for plane in &self.planes {
            if plane.signed_distance_to_point(sphere.center()) < -sphere.radius() {
                return false;
            }
        }
        true
    }
}

impl ApproxEq for Frustum {
    fn approx_eq(&self, other: &Self, epsilon: Epsilon) -> bool {
        self.planes
            .iter()
            .zip(other.planes.iter())
            .all(|(a, b)| a.approx_eq(b, epsilon))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eps() -> Epsilon {
        Epsilon::new(1.0e-4).unwrap()
    }

    fn standard_frustum() -> Frustum {
        let proj = Mat4::perspective(std::f32::consts::FRAC_PI_2, 1.0, 1.0, 100.0).unwrap();
        Frustum::from_view_projection(proj).unwrap()
    }

    #[test]
    fn from_view_projection_extracts_six_planes() {
        let f = standard_frustum();
        assert_eq!(f.planes().len(), 6);
    }

    #[test]
    fn contains_point_in_front_of_camera() {
        let f = standard_frustum();
        // View space looks down -Z; (0, 0, -5) is well inside near/far.
        assert!(f.contains_point(Vec3::new(0.0, 0.0, -5.0)));
    }

    #[test]
    fn rejects_point_behind_camera() {
        let f = standard_frustum();
        assert!(!f.contains_point(Vec3::new(0.0, 0.0, 5.0)));
    }

    #[test]
    fn rejects_point_past_far_plane() {
        let f = standard_frustum();
        assert!(!f.contains_point(Vec3::new(0.0, 0.0, -200.0)));
    }

    #[test]
    fn intersects_aabb_in_front_of_camera() {
        let f = standard_frustum();
        let aabb = Aabb::from_center_extents(
            Vec3::new(0.0, 0.0, -10.0),
            Vec3::new(1.0, 1.0, 1.0),
        )
        .unwrap();
        assert!(f.intersects_aabb(&aabb));
    }

    #[test]
    fn culls_aabb_behind_camera() {
        let f = standard_frustum();
        let aabb = Aabb::from_center_extents(
            Vec3::new(0.0, 0.0, 50.0),
            Vec3::new(1.0, 1.0, 1.0),
        )
        .unwrap();
        assert!(!f.intersects_aabb(&aabb));
    }

    #[test]
    fn intersects_sphere_in_front_of_camera() {
        let f = standard_frustum();
        let s = Sphere::new(Vec3::new(0.0, 0.0, -10.0), 1.0).unwrap();
        assert!(f.intersects_sphere(&s));
    }

    #[test]
    fn culls_sphere_behind_camera() {
        let f = standard_frustum();
        let s = Sphere::new(Vec3::new(0.0, 0.0, 50.0), 1.0).unwrap();
        assert!(!f.intersects_sphere(&s));
    }

    #[test]
    fn approx_eq_compares_planes_pointwise() {
        let a = standard_frustum();
        let b = standard_frustum();
        assert!(a.approx_eq(&b, eps()));
    }
}
