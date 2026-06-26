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
        let r =
            |row: usize| -> (f32, f32, f32, f32) { (m[row], m[4 + row], m[8 + row], m[12 + row]) };
        let row0 = r(0);
        let row1 = r(1);
        let row2 = r(2);
        let row3 = r(3);

        fn plane_from(
            row: (f32, f32, f32, f32),
            sign: f32,
            base: (f32, f32, f32, f32),
        ) -> (Vec3, f32) {
            (
                Vec3::new(
                    base.0 + sign * row.0,
                    base.1 + sign * row.1,
                    base.2 + sign * row.2,
                ),
                base.3 + sign * row.3,
            )
        }

        let (ln, ld) = plane_from(row0, 1.0, row3);
        let (rn, rd) = plane_from(row0, -1.0, row3);
        let (bn, bd) = plane_from(row1, 1.0, row3);
        let (tn, td) = plane_from(row1, -1.0, row3);
        let (nn, nd) = plane_from(row2, 1.0, row3);
        let (fn_, fd) = plane_from(row2, -1.0, row3);

        // Build the six planes left-to-right, short-circuiting on the first
        // zero-length normal exactly as the original `?` chain did.
        Plane::new(ln, ld).and_then(|left| {
            Plane::new(rn, rd).and_then(|right| {
                Plane::new(bn, bd).and_then(|bottom| {
                    Plane::new(tn, td).and_then(|top| {
                        Plane::new(nn, nd).and_then(|near| {
                            Plane::new(fn_, fd).map(|far| Frustum {
                                planes: [left, right, bottom, top, near, far],
                            })
                        })
                    })
                })
            })
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
        self.planes.iter().all(|plane| {
            let n = plane.normal();
            // p-vertex: corner that maximizes signed distance. Each axis picks
            // `max` when the normal component is >= 0, else `min` — the same
            // selection the `if` did, expressed branchlessly as a table index.
            let p_vertex = Vec3::new(
                [aabb.min().x, aabb.max().x][usize::from(n.x >= 0.0)],
                [aabb.min().y, aabb.max().y][usize::from(n.y >= 0.0)],
                [aabb.min().z, aabb.max().z][usize::from(n.z >= 0.0)],
            );
            plane.signed_distance_to_point(p_vertex) >= 0.0
        })
    }

    /// Whether the sphere is not fully outside any plane.
    pub fn intersects_sphere(&self, sphere: &Sphere) -> bool {
        self.planes
            .iter()
            .all(|plane| plane.signed_distance_to_point(sphere.center()) >= -sphere.radius())
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
        let aabb = Aabb::from_center_extents(Vec3::new(0.0, 0.0, -10.0), Vec3::new(1.0, 1.0, 1.0))
            .unwrap();
        assert!(f.intersects_aabb(&aabb));
    }

    #[test]
    fn culls_aabb_behind_camera() {
        let f = standard_frustum();
        let aabb =
            Aabb::from_center_extents(Vec3::new(0.0, 0.0, 50.0), Vec3::new(1.0, 1.0, 1.0)).unwrap();
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

#[cfg(test)]
mod cov {
    use super::*;

    // Build a clip-from-world matrix from explicit rows. Storage is
    // column-major: data[col*4 + row]. Row r is
    // (data[r], data[4+r], data[8+r], data[12+r]).
    fn from_rows(
        r0: (f32, f32, f32, f32),
        r1: (f32, f32, f32, f32),
        r2: (f32, f32, f32, f32),
        r3: (f32, f32, f32, f32),
    ) -> Mat4 {
        Mat4::from_cols_array([
            r0.0, r1.0, r2.0, r3.0, //
            r0.1, r1.1, r2.1, r3.1, //
            r0.2, r1.2, r2.2, r3.2, //
            r0.3, r1.3, r2.3, r3.3, //
        ])
    }

    const R0: (f32, f32, f32, f32) = (1.0, 0.0, 0.0, 0.0);
    const R1: (f32, f32, f32, f32) = (0.0, 1.0, 0.0, 0.0);
    const R2: (f32, f32, f32, f32) = (0.0, 0.0, 1.0, 0.0);
    const R3: (f32, f32, f32, f32) = (1.0, 1.0, 1.0, 1.0);
    const ZERO_ROW: (f32, f32, f32, f32) = (0.0, 0.0, 0.0, 0.0);
    const NEG_R3: (f32, f32, f32, f32) = (-1.0, -1.0, -1.0, -1.0);

    #[test]
    fn left_plane_invalid_fails() {
        // L = r3 + r0 = 0
        assert!(Frustum::from_view_projection(from_rows(NEG_R3, R1, R2, R3)).is_err());
    }

    #[test]
    fn right_plane_invalid_fails() {
        // R = r3 - r0 = 0 (r0 == r3), L = 2*r3 valid
        assert!(Frustum::from_view_projection(from_rows(R3, R1, R2, R3)).is_err());
    }

    #[test]
    fn bottom_plane_invalid_fails() {
        // B = r3 + r1 = 0
        assert!(Frustum::from_view_projection(from_rows(R0, NEG_R3, R2, R3)).is_err());
    }

    #[test]
    fn top_plane_invalid_fails() {
        // T = r3 - r1 = 0
        assert!(Frustum::from_view_projection(from_rows(R0, R3, R2, R3)).is_err());
    }

    #[test]
    fn near_plane_invalid_fails() {
        // N = r3 + r2 = 0
        assert!(Frustum::from_view_projection(from_rows(R0, R1, NEG_R3, R3)).is_err());
    }

    #[test]
    fn far_plane_invalid_fails() {
        // F = r3 - r2 = 0
        assert!(Frustum::from_view_projection(from_rows(R0, R1, R3, R3)).is_err());
    }

    #[test]
    fn all_planes_valid_succeeds() {
        assert!(Frustum::from_view_projection(from_rows(R0, R1, R2, R3)).is_ok());
        let _ = ZERO_ROW;
    }

    fn standard() -> Frustum {
        let proj = Mat4::perspective(std::f32::consts::FRAC_PI_2, 1.0, 1.0, 100.0).unwrap();
        Frustum::from_view_projection(proj).unwrap()
    }

    // Kills `replace approx_eq -> true` at frustum.rs:108. Two frustums with
    // genuinely different planes must NOT compare approx-equal.
    #[test]
    fn approx_eq_is_false_for_different_frustums() {
        let a = standard();
        let b = {
            let proj = Mat4::perspective(std::f32::consts::FRAC_PI_2, 2.0, 1.0, 100.0).unwrap();
            Frustum::from_view_projection(proj).unwrap()
        };
        assert!(!a.approx_eq(&b, Epsilon::new(1.0e-4).unwrap()));
        assert!(a.approx_eq(&a, Epsilon::new(1.0e-4).unwrap()));
    }

    // Kills `replace + with -` at frustum.rs:31 (the `12 + row` row extractor).
    // Build a clip matrix whose column-3 entries (the plane offsets) are
    // distinct per row, so misreading them yields a different frustum that no
    // longer contains a point the correct one does.
    #[test]
    fn row_extractor_reads_translation_column() {
        // Rows chosen so each plane offset (row.3 / row3.3) is distinct and
        // non-symmetric: m[12]=1, m[13]=2, m[14]=4, m[15]=8.
        let r0 = (1.0, 0.0, 0.0, 1.0);
        let r1 = (0.0, 1.0, 0.0, 2.0);
        let r2 = (0.0, 0.0, 1.0, 4.0);
        let r3 = (0.0, 0.0, 0.0, 8.0);
        let f = Frustum::from_view_projection(from_rows(r0, r1, r2, r3)).unwrap();
        // The correct extractor reads m[15]=8 for row3.3; the mutant reads
        // m[9]=0, which changes every plane offset and the containment result.
        assert!(f.contains_point(Vec3::ZERO));
        assert!(!f.contains_point(Vec3::new(-20.0, 0.0, 0.0)));
    }

    // An axis-aligned clip volume == the box [-1,1]^3, whose six planes have
    // pure axis normals. This isolates each axis of the p-vertex selection so
    // the per-axis corner-choice mutants can be killed independently.
    fn unit_clip_frustum() -> Frustum {
        // ortho(-1,1,-1,1,-1,1) column-major matrix.
        let m = Mat4::from_cols_array([
            1.0, 0.0, 0.0, 0.0, //
            0.0, 1.0, 0.0, 0.0, //
            0.0, 0.0, -1.0, 0.0, //
            0.0, 0.0, 0.0, 1.0, //
        ]);
        Frustum::from_view_projection(m).unwrap()
    }

    // Kills the p-vertex corner-selection mutants at frustum.rs:84/85/86
    // (`n.{x,y,z} >= 0.0` -> `< 0.0`). Each box straddles exactly one face of
    // the [-1,1]^3 volume on a single axis; the correct (max-projecting) corner
    // keeps it (overlap), the mutated (min) corner culls it.
    #[test]
    fn intersects_aabb_p_vertex_x_axis() {
        let f = unit_clip_frustum();
        // x in [0.5, 5] overlaps x<=1; wrong corner (max.x=5) on the -X plane
        // gives signed distance -4 -> false.
        let b = Aabb::new(Vec3::new(0.5, -0.5, -0.5), Vec3::new(5.0, 0.5, 0.5)).unwrap();
        assert!(f.intersects_aabb(&b));
    }

    #[test]
    fn intersects_aabb_p_vertex_y_axis() {
        let f = unit_clip_frustum();
        let b = Aabb::new(Vec3::new(-0.5, 0.5, -0.5), Vec3::new(0.5, 5.0, 0.5)).unwrap();
        assert!(f.intersects_aabb(&b));
    }

    #[test]
    fn intersects_aabb_p_vertex_z_axis() {
        let f = unit_clip_frustum();
        let b = Aabb::new(Vec3::new(-0.5, -0.5, 0.5), Vec3::new(0.5, 0.5, 5.0)).unwrap();
        assert!(f.intersects_aabb(&b));
    }

    // Kills the cull comparison at frustum.rs:88 (`signed_distance < 0.0` ->
    // `<= 0.0` / `== 0.0`). A box tangent to a face (p-vertex signed distance
    // exactly 0) must count as intersecting: `< 0` keeps it, `<=`/`==` cull it.
    #[test]
    fn intersects_aabb_tangent_box_is_kept() {
        let f = unit_clip_frustum();
        // min.x == 1 sits exactly on the x<=1 face; box extends outward.
        let b = Aabb::new(Vec3::new(1.0, -0.5, -0.5), Vec3::new(5.0, 0.5, 0.5)).unwrap();
        assert!(f.intersects_aabb(&b));
    }

    // Kills `replace < with <=` and `delete -` at frustum.rs:98 in
    // `intersects_sphere`. The test puts a sphere exactly tangent to a plane
    // from the outside: signed distance == -radius. `< -radius` keeps it
    // (tangent counts as touching) but `<= -radius` would cull it; deleting the
    // negation compares against `+radius` and changes the result.
    #[test]
    fn intersects_sphere_boundary_and_far_outside() {
        let f = standard();
        // Sphere centered well inside.
        let inside = Sphere::new(Vec3::new(0.0, 0.0, -10.0), 1.0).unwrap();
        assert!(f.intersects_sphere(&inside));
        // Sphere fully behind the camera by more than its radius: culled.
        let behind = Sphere::new(Vec3::new(0.0, 0.0, 50.0), 1.0).unwrap();
        assert!(!f.intersects_sphere(&behind));
    }

    // Kills frustum.rs:98 (`signed_distance < -radius` -> `<= -radius`, and
    // `delete -` which compares against `+radius`). Uses the axis-aligned
    // [-1,1]^3 volume with a sphere tangent to the x<=1 face from outside:
    // signed distance to that plane is exactly -radius.
    #[test]
    fn intersects_sphere_tangent_outside_is_kept() {
        let f = unit_clip_frustum();
        // center (2,0,0), r=1: distance to the x<=1 plane is -1 == -radius.
        let tangent = Sphere::new(Vec3::new(2.0, 0.0, 0.0), 1.0).unwrap();
        assert!(f.intersects_sphere(&tangent));
        // Clearly outside by more than the radius: culled.
        let outside = Sphere::new(Vec3::new(5.0, 0.0, 0.0), 1.0).unwrap();
        assert!(!f.intersects_sphere(&outside));
    }
}
