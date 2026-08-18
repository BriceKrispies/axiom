//! A triangle: three points, the plane they span, and the closest-point solve
//! every contact query in this layer ultimately asks a mesh.

use crate::approx_eq::ApproxEq;
use crate::epsilon::Epsilon;
use crate::math_error::MathError;
use crate::math_result::MathResult;
use crate::segment::Segment;
use crate::vec3::Vec3;

/// The smallest denominator a barycentric solve will divide by; see the note on
/// [`crate::Segment`]'s own guard. Every numerator paired with it is zero for
/// the degenerate triangle, so the guarded quotient stays finite.
const SAFE_DENOMINATOR: f32 = f32::MIN_POSITIVE;

/// A triangle `a`, `b`, `c` with counter-clockwise winding around
/// [`Triangle::normal`].
///
/// [`Triangle::new`] rejects non-finite vertices but **accepts** a degenerate
/// (zero-area) triangle: mesh data contains slivers and collapsed faces, and a
/// constructor that rejected them would push a branch into every loader. The
/// degeneracy surfaces where it actually matters — [`Triangle::normal`] fails
/// for it, and every cast reports a miss against it.
#[derive(Debug, Clone, Copy)]
pub struct Triangle {
    a: Vec3,
    b: Vec3,
    c: Vec3,
}

impl Triangle {
    /// Construct from three finite vertices, which may be collinear.
    pub fn new(a: Vec3, b: Vec3, c: Vec3) -> MathResult<Triangle> {
        let all_finite = [a, b, c]
            .into_iter()
            .flat_map(|v| [v.x, v.y, v.z])
            .all(|component| component.is_finite());
        all_finite
            .then_some(Triangle { a, b, c })
            .ok_or_else(|| MathError::non_finite_scalar("Triangle vertices must be finite"))
    }

    /// First vertex.
    pub const fn a(&self) -> Vec3 {
        self.a
    }

    /// Second vertex.
    pub const fn b(&self) -> Vec3 {
        self.b
    }

    /// Third vertex.
    pub const fn c(&self) -> Vec3 {
        self.c
    }

    /// `b - a`, the first edge vector.
    pub const fn edge_ab(&self) -> Vec3 {
        self.b.subtract(self.a)
    }

    /// `c - a`, the second edge vector.
    pub const fn edge_ac(&self) -> Vec3 {
        self.c.subtract(self.a)
    }

    /// The three bounding edges, in winding order: `a->b`, `b->c`, `c->a`.
    pub fn edges(&self) -> [Segment; 3] {
        [
            Segment::from_points(self.a, self.b),
            Segment::from_points(self.b, self.c),
            Segment::from_points(self.c, self.a),
        ]
    }

    /// The unit normal `(b - a) x (c - a)`, or
    /// [`crate::MathErrorCode::NormalizeZeroLength`] for a degenerate triangle,
    /// which spans no plane and therefore has no normal.
    pub fn normal(&self) -> MathResult<Vec3> {
        self.edge_ab().cross(self.edge_ac()).normalize()
    }

    /// Twice the triangle's area.
    pub fn double_area(&self) -> f32 {
        self.edge_ab().cross(self.edge_ac()).length()
    }

    /// The barycentric coordinates `(v, w)` of `p` **projected onto the
    /// triangle's plane**, where the third coordinate is `u = 1 - v - w` and the
    /// point is `a + v * (b - a) + w * (c - a)`.
    ///
    /// `p` lies over the triangle exactly when `v >= 0`, `w >= 0` and
    /// `v + w <= 1` — the epsilon-free containment test the face arm of every
    /// swept query uses, wrapped up as [`Self::contains_projection`]. A
    /// degenerate triangle spans no plane, so its coordinates are meaningless;
    /// they come out of the guarded denominator finite rather than infinite, and
    /// it is [`Self::contains_projection`] that rules the face out.
    pub fn barycentric(&self, p: Vec3) -> (f32, f32) {
        let ab = self.edge_ab();
        let ac = self.edge_ac();
        let ap = p.subtract(self.a);
        let d00 = ab.length_squared();
        let d01 = ab.dot(ac);
        let d11 = ac.length_squared();
        let d20 = ap.dot(ab);
        let d21 = ap.dot(ac);
        let denom = (d00 * d11 - d01 * d01).max(SAFE_DENOMINATOR);
        (
            (d11 * d20 - d01 * d21) / denom,
            (d00 * d21 - d01 * d20) / denom,
        )
    }

    /// Whether `p` projects onto the triangle's face (as opposed to outside its
    /// boundary). The projection's distance from the plane is not consulted.
    ///
    /// A degenerate triangle has no face and so contains nothing, which the
    /// area term states directly — the barycentric coordinates alone cannot say
    /// so, because a collapsed triangle cancels its own numerators to zero.
    pub fn contains_projection(&self, p: Vec3) -> bool {
        let (v, w) = self.barycentric(p);
        (v >= 0.0) & (w >= 0.0) & (v + w <= 1.0) & (self.double_area() > 0.0)
    }

    /// The closest point on the triangle (face, edges and vertices) to `p`.
    pub fn closest_point_to(&self, p: Vec3) -> Vec3 {
        let (v, w) = self.closest_barycentric(p);
        self.a
            .add(self.edge_ab().mul_scalar(v))
            .add(self.edge_ac().mul_scalar(w))
    }

    /// The barycentric coordinates of [`Self::closest_point_to`], in Ericson's
    /// Voronoi-region form.
    ///
    /// The seven regions — three vertices, three edges, the face — are *not*
    /// selected by a branch. Every region's coordinates are computed
    /// unconditionally from the same six edge dot products, each region's
    /// membership test is evaluated as a plain boolean, and the answers are then
    /// stacked in reverse priority order so that the highest-priority region
    /// that claims `p` is the one left standing. Each region's own divisor is a
    /// quantity that region's membership test already forces non-negative, so
    /// the guarded division is exact where it is selected and merely finite
    /// where it is not.
    fn closest_barycentric(&self, p: Vec3) -> (f32, f32) {
        let ab = self.edge_ab();
        let ac = self.edge_ac();
        let ap = p.subtract(self.a);
        let bp = p.subtract(self.b);
        let cp = p.subtract(self.c);
        let (d1, d2) = (ab.dot(ap), ac.dot(ap));
        let (d3, d4) = (ab.dot(bp), ac.dot(bp));
        let (d5, d6) = (ab.dot(cp), ac.dot(cp));
        let vc = d1 * d4 - d3 * d2;
        let vb = d5 * d2 - d1 * d6;
        let va = d3 * d6 - d5 * d4;
        let in_a = (d1 <= 0.0) & (d2 <= 0.0);
        let in_b = (d3 >= 0.0) & (d4 <= d3);
        let in_c = (d6 >= 0.0) & (d5 <= d6);
        let on_ab = (vc <= 0.0) & (d1 >= 0.0) & (d3 <= 0.0);
        let on_ac = (vb <= 0.0) & (d2 >= 0.0) & (d6 <= 0.0);
        let bc_run = d4 - d3;
        let bc_rise = d5 - d6;
        let on_bc = (va <= 0.0) & (bc_run >= 0.0) & (bc_rise >= 0.0);
        let v_ab = (d1 / (d1 - d3).max(SAFE_DENOMINATOR)).clamp(0.0, 1.0);
        let w_ac = (d2 / (d2 - d6).max(SAFE_DENOMINATOR)).clamp(0.0, 1.0);
        let w_bc = (bc_run / (bc_run + bc_rise).max(SAFE_DENOMINATOR)).clamp(0.0, 1.0);
        let scale = 1.0 / (va + vb + vc).max(SAFE_DENOMINATOR);
        let face = (vb * scale, vc * scale);
        let by_bc = [face, (1.0 - w_bc, w_bc)][usize::from(on_bc)];
        let by_ac = [by_bc, (0.0, w_ac)][usize::from(on_ac)];
        let by_c = [by_ac, (0.0, 1.0)][usize::from(in_c)];
        let by_ab = [by_c, (v_ab, 0.0)][usize::from(on_ab)];
        let by_b = [by_ab, (1.0, 0.0)][usize::from(in_b)];
        [by_b, (0.0, 0.0)][usize::from(in_a)]
    }
}

impl ApproxEq for Triangle {
    fn approx_eq(&self, other: &Self, epsilon: Epsilon) -> bool {
        self.a.approx_eq(&other.a, epsilon)
            & self.b.approx_eq(&other.b, epsilon)
            & self.c.approx_eq(&other.c, epsilon)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math_error_code::MathErrorCode;

    fn eps() -> Epsilon {
        Epsilon::DEFAULT
    }

    /// The right triangle `(0,0,0) (4,0,0) (0,0,4)` lying in the y = 0 plane,
    /// normal +Y.
    fn floor_triangle() -> Triangle {
        Triangle::new(
            Vec3::ZERO,
            Vec3::new(4.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 4.0),
        )
        .unwrap()
    }

    #[test]
    fn new_rejects_non_finite_vertices() {
        assert_eq!(
            Triangle::new(Vec3::ZERO, Vec3::new(f32::NAN, 0.0, 0.0), Vec3::UNIT_Z)
                .unwrap_err()
                .code(),
            MathErrorCode::NonFiniteScalar
        );
    }

    #[test]
    fn accessors_report_vertices_edges_and_area() {
        let tri = floor_triangle();
        assert!(tri.a().approx_eq(&Vec3::ZERO, eps()));
        assert!(tri.b().approx_eq(&Vec3::new(4.0, 0.0, 0.0), eps()));
        assert!(tri.c().approx_eq(&Vec3::new(0.0, 0.0, 4.0), eps()));
        assert!(tri.edge_ab().approx_eq(&Vec3::new(4.0, 0.0, 0.0), eps()));
        assert!(tri.edge_ac().approx_eq(&Vec3::new(0.0, 0.0, 4.0), eps()));
        assert_eq!(tri.double_area(), 16.0);
    }

    #[test]
    fn edges_wind_a_to_b_to_c_and_back() {
        let tri = floor_triangle();
        let edges = tri.edges();
        assert!(edges[0].start().approx_eq(&tri.a(), eps()));
        assert!(edges[0].end().approx_eq(&tri.b(), eps()));
        assert!(edges[1].start().approx_eq(&tri.b(), eps()));
        assert!(edges[1].end().approx_eq(&tri.c(), eps()));
        assert!(edges[2].start().approx_eq(&tri.c(), eps()));
        assert!(edges[2].end().approx_eq(&tri.a(), eps()));
    }

    #[test]
    fn normal_is_the_unit_cross_product() {
        // a->b is +X and a->c is +Z, so the winding normal is X x Z = -Y.
        let tri = floor_triangle();
        assert!(tri
            .normal()
            .unwrap()
            .approx_eq(&Vec3::new(0.0, -1.0, 0.0), eps()));
    }

    #[test]
    fn degenerate_triangle_has_no_normal() {
        let collinear = Triangle::new(
            Vec3::ZERO,
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(2.0, 0.0, 0.0),
        )
        .unwrap();
        assert_eq!(collinear.double_area(), 0.0);
        assert_eq!(
            collinear.normal().unwrap_err().code(),
            MathErrorCode::NormalizeZeroLength
        );
    }

    #[test]
    fn barycentric_names_the_vertices_and_the_centroid() {
        let tri = floor_triangle();
        assert_eq!(tri.barycentric(tri.a()), (0.0, 0.0));
        assert_eq!(tri.barycentric(tri.b()), (1.0, 0.0));
        assert_eq!(tri.barycentric(tri.c()), (0.0, 1.0));
        let (v, w) = tri.barycentric(Vec3::new(1.0, 9.0, 1.0));
        assert!(v.approx_eq(&0.25, eps()));
        assert!(w.approx_eq(&0.25, eps()));
    }

    #[test]
    fn contains_projection_answers_over_and_beyond_the_face() {
        let tri = floor_triangle();
        assert!(tri.contains_projection(Vec3::new(1.0, 5.0, 1.0)));
        assert!(tri.contains_projection(Vec3::new(2.0, 0.0, 2.0)));
        assert!(!tri.contains_projection(Vec3::new(3.0, 0.0, 3.0)));
        assert!(!tri.contains_projection(Vec3::new(-1.0, 0.0, 1.0)));
        assert!(!tri.contains_projection(Vec3::new(1.0, 0.0, -1.0)));
    }

    #[test]
    fn degenerate_triangle_contains_no_projection() {
        let collinear = Triangle::new(
            Vec3::ZERO,
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(2.0, 0.0, 0.0),
        )
        .unwrap();
        assert!(!collinear.contains_projection(Vec3::new(0.5, 0.0, 0.0)));
    }
}

#[cfg(test)]
mod closest_point_tests {
    use super::*;

    fn eps() -> Epsilon {
        Epsilon::DEFAULT
    }

    fn floor_triangle() -> Triangle {
        Triangle::new(
            Vec3::ZERO,
            Vec3::new(4.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 4.0),
        )
        .unwrap()
    }

    #[test]
    fn point_over_the_face_projects_onto_it() {
        let tri = floor_triangle();
        assert!(tri
            .closest_point_to(Vec3::new(1.0, 3.0, 1.0))
            .approx_eq(&Vec3::new(1.0, 0.0, 1.0), eps()));
    }

    #[test]
    fn point_past_each_vertex_returns_that_vertex() {
        let tri = floor_triangle();
        assert!(tri
            .closest_point_to(Vec3::new(-1.0, 1.0, -1.0))
            .approx_eq(&tri.a(), eps()));
        assert!(tri
            .closest_point_to(Vec3::new(9.0, 1.0, -1.0))
            .approx_eq(&tri.b(), eps()));
        assert!(tri
            .closest_point_to(Vec3::new(-1.0, 1.0, 9.0))
            .approx_eq(&tri.c(), eps()));
    }

    #[test]
    fn point_beside_each_edge_returns_a_point_on_that_edge() {
        let tri = floor_triangle();
        // Beside a->b (the z = 0 edge).
        assert!(tri
            .closest_point_to(Vec3::new(2.0, 1.0, -3.0))
            .approx_eq(&Vec3::new(2.0, 0.0, 0.0), eps()));
        // Beside a->c (the x = 0 edge).
        assert!(tri
            .closest_point_to(Vec3::new(-3.0, 1.0, 2.0))
            .approx_eq(&Vec3::new(0.0, 0.0, 2.0), eps()));
        // Beside the hypotenuse b->c.
        assert!(tri
            .closest_point_to(Vec3::new(4.0, 1.0, 4.0))
            .approx_eq(&Vec3::new(2.0, 0.0, 2.0), eps()));
    }

    #[test]
    fn point_exactly_on_a_vertex_and_on_an_edge_is_returned_unchanged() {
        let tri = floor_triangle();
        assert!(tri.closest_point_to(tri.a()).approx_eq(&tri.a(), eps()));
        assert!(tri.closest_point_to(tri.b()).approx_eq(&tri.b(), eps()));
        assert!(tri.closest_point_to(tri.c()).approx_eq(&tri.c(), eps()));
        assert!(tri
            .closest_point_to(Vec3::new(2.0, 0.0, 0.0))
            .approx_eq(&Vec3::new(2.0, 0.0, 0.0), eps()));
        assert!(tri
            .closest_point_to(Vec3::new(2.0, 0.0, 2.0))
            .approx_eq(&Vec3::new(2.0, 0.0, 2.0), eps()));
    }

    #[test]
    fn degenerate_triangle_answers_along_its_collapsed_span() {
        let collinear = Triangle::new(
            Vec3::ZERO,
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(4.0, 0.0, 0.0),
        )
        .unwrap();
        assert!(collinear
            .closest_point_to(Vec3::new(1.0, 5.0, 0.0))
            .approx_eq(&Vec3::new(1.0, 0.0, 0.0), eps()));
        assert!(collinear
            .closest_point_to(Vec3::new(-3.0, 1.0, 0.0))
            .approx_eq(&Vec3::ZERO, eps()));
    }

    #[test]
    fn fully_collapsed_triangle_answers_its_single_point() {
        let point = Triangle::new(
            Vec3::new(1.0, 1.0, 1.0),
            Vec3::new(1.0, 1.0, 1.0),
            Vec3::new(1.0, 1.0, 1.0),
        )
        .unwrap();
        assert!(point
            .closest_point_to(Vec3::new(5.0, 5.0, 5.0))
            .approx_eq(&Vec3::new(1.0, 1.0, 1.0), eps()));
    }

    #[test]
    fn approx_eq_compares_every_vertex() {
        let tri = floor_triangle();
        assert!(tri.approx_eq(&tri, eps()));
        let moved_a = Triangle::new(Vec3::UNIT_Y, tri.b(), tri.c()).unwrap();
        let moved_b = Triangle::new(tri.a(), Vec3::UNIT_Y, tri.c()).unwrap();
        let moved_c = Triangle::new(tri.a(), tri.b(), Vec3::UNIT_Y).unwrap();
        assert!(!tri.approx_eq(&moved_a, eps()));
        assert!(!tri.approx_eq(&moved_b, eps()));
        assert!(!tri.approx_eq(&moved_c, eps()));
    }
}
