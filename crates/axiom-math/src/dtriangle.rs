//! Double-precision triangle: ray intersection and closest point.

use crate::approx_eq::ApproxEq;
use crate::dvec3::DVec3;
use crate::epsilon::Epsilon;

/// Where a ray met a triangle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DTriangleHit {
    /// Distance along the ray, in units of the ray's direction vector. Negative
    /// when the triangle is behind the origin — the intersection is reported
    /// rather than culled, because a caller measuring penetration depth needs
    /// the exit hit as much as the entry one.
    pub distance: f64,
    /// Whether the ray struck the side the winding faces.
    pub front_face: bool,
}

/// A triangle in `f64`, wound counter-clockwise as seen from its front face.
///
/// The double-precision sibling of [`crate::Triangle`]; see [`crate::Scalar`]
/// for when the extra precision is load-bearing and when it is not. This is the
/// leaf primitive of a triangle-soup collision world: every ray, sweep and
/// overlap against static geometry bottoms out in the two queries below.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DTriangle {
    pub a: DVec3,
    pub b: DVec3,
    pub c: DVec3,
}

/// Below this, the ray is treated as parallel to the triangle's plane and the
/// intersection is rejected rather than divided by a near-zero determinant.
const PARALLEL_DETERMINANT: f64 = 1.0e-12;

/// Barycentric slack on the edge tests.
///
/// Deliberately asymmetric and deliberately not zero. Adjacent triangles in a
/// soup share an edge, and a ray passing exactly through it must hit *at least*
/// one of them — an exact test lets it slip between two triangles that visually
/// touch, which is the classic "bullet through the wall seam" defect. Erring
/// outward makes the seam double-hit instead, which a closest-hit traversal
/// resolves harmlessly.
const EDGE_SLACK: f64 = 1.0e-6;
const EDGE_LIMIT: f64 = 1.000_001;

impl DTriangle {
    /// Vertex constructor.
    pub const fn new(a: DVec3, b: DVec3, c: DVec3) -> Self {
        DTriangle { a, b, c }
    }

    /// The unnormalised geometric normal, `(b - a) × (c - a)`.
    ///
    /// Left unnormalised because its *length* is twice the triangle's area, and
    /// both callers of this want one or the other. Normalising here would throw
    /// the area away and force a degenerate triangle to decide what a zero
    /// normal means.
    pub fn normal_scaled(self) -> DVec3 {
        self.b.subtract(self.a).cross(self.c.subtract(self.a))
    }

    /// Möller–Trumbore ray/triangle intersection.
    ///
    /// Backfaces are **not** culled: [`DTriangleHit::front_face`] reports which
    /// side was struck and the caller decides. A penetration query needs the
    /// exit hit, and a one-sided test cannot give it one.
    ///
    /// `direction` need not be normalised; [`DTriangleHit::distance`] is in
    /// units of whatever length it has, so a caller can pass a full segment and
    /// read the hit parameter directly in `[0, 1]`.
    pub fn ray_hit(self, origin: DVec3, direction: DVec3) -> Option<DTriangleHit> {
        let edge1 = self.b.subtract(self.a);
        let edge2 = self.c.subtract(self.a);

        let pvec = direction.cross(edge2);
        let determinant = edge1.dot(pvec);
        let parallel = determinant.abs() < PARALLEL_DETERMINANT;

        // Evaluated even when parallel; the result is discarded by the select
        // at the end. A division by a near-zero determinant yields an infinity
        // or a NaN, never a trap.
        let inverse = 1.0 / determinant;
        let tvec = origin.subtract(self.a);
        let u = tvec.dot(pvec) * inverse;
        // A NaN `u` fails both comparisons, so it lands in `outside_u` — which
        // is the answer we want for a degenerate determinant anyway.
        let outside_u = !((u >= -EDGE_SLACK) & (u <= EDGE_LIMIT));

        let qvec = tvec.cross(edge1);
        let v = direction.dot(qvec) * inverse;
        let outside_v = (v < -EDGE_SLACK) | (u + v > EDGE_LIMIT);

        let missed = parallel | outside_u | outside_v;
        let distance = edge2.dot(qvec) * inverse;

        (!missed).then_some(DTriangleHit {
            distance,
            front_face: determinant > 0.0,
        })
    }

    /// The point on the triangle closest to `p` (Ericson, *Real-Time Collision
    /// Detection* §5.1.5).
    ///
    /// The plane projection falls in one of seven Voronoi regions — the three
    /// vertices, the three edges, or the face interior — and each has its own
    /// closed form. All seven are evaluated and one is selected, in the order
    /// the region tests are written: a point in the vertex-A region satisfies
    /// the A test and is not reached by any later one, so the selects are
    /// applied in reverse and the earliest match survives.
    pub fn closest_point(self, p: DVec3) -> DVec3 {
        let ab = self.b.subtract(self.a);
        let ac = self.c.subtract(self.a);
        let ap = p.subtract(self.a);
        let d1 = ab.dot(ap);
        let d2 = ac.dot(ap);
        let in_vertex_a = (d1 <= 0.0) & (d2 <= 0.0);

        let bp = p.subtract(self.b);
        let d3 = ab.dot(bp);
        let d4 = ac.dot(bp);
        let in_vertex_b = (d3 >= 0.0) & (d4 <= d3);

        let vc = d1 * d4 - d3 * d2;
        let in_edge_ab = (vc <= 0.0) & (d1 >= 0.0) & (d3 <= 0.0);
        let on_ab = self.a.add(ab.mul_scalar(d1 / (d1 - d3)));

        let cp = p.subtract(self.c);
        let d5 = ab.dot(cp);
        let d6 = ac.dot(cp);
        let in_vertex_c = (d6 >= 0.0) & (d5 <= d6);

        let vb = d5 * d2 - d1 * d6;
        let in_edge_ac = (vb <= 0.0) & (d2 >= 0.0) & (d6 <= 0.0);
        let on_ac = self.a.add(ac.mul_scalar(d2 / (d2 - d6)));

        let va = d3 * d6 - d5 * d4;
        let in_edge_bc = (va <= 0.0) & ((d4 - d3) >= 0.0) & ((d5 - d6) >= 0.0);
        let bc_parameter = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        let on_bc = self
            .b
            .add(self.c.subtract(self.b).mul_scalar(bc_parameter));

        let denominator = 1.0 / (va + vb + vc);
        let interior = self
            .a
            .add(ab.mul_scalar(vb * denominator))
            .add(ac.mul_scalar(vc * denominator));

        // Applied in reverse test order, so the earliest satisfied region wins.
        let picked = [interior, on_bc][usize::from(in_edge_bc)];
        let picked = [picked, on_ac][usize::from(in_edge_ac)];
        let picked = [picked, self.c][usize::from(in_vertex_c)];
        let picked = [picked, on_ab][usize::from(in_edge_ab)];
        let picked = [picked, self.b][usize::from(in_vertex_b)];
        [picked, self.a][usize::from(in_vertex_a)]
    }
}

impl ApproxEq for DTriangle {
    fn approx_eq(&self, other: &Self, epsilon: Epsilon) -> bool {
        self.a.approx_eq(&other.a, epsilon)
            & self.b.approx_eq(&other.b, epsilon)
            & self.c.approx_eq(&other.c, epsilon)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The unit triangle in the z = 0 plane, wound counter-clockwise seen from
    /// `+z`, so its front face points that way.
    fn flat() -> DTriangle {
        DTriangle::new(
            DVec3::ZERO,
            DVec3::new(1.0, 0.0, 0.0),
            DVec3::new(0.0, 1.0, 0.0),
        )
    }

    #[test]
    fn the_scaled_normal_points_along_the_winding_and_carries_twice_the_area() {
        let n = flat().normal_scaled();
        assert_eq!(n, DVec3::new(0.0, 0.0, 1.0));
        assert_eq!(n.length() * 0.5, 0.5);
    }

    #[test]
    fn a_ray_through_the_interior_hits_at_the_plane_distance() {
        // The winding puts the normal at +z, so a ray travelling -z meets the
        // front face. A ray travelling +z meets the back, which is the next
        // test rather than a different answer to this one.
        let hit = flat()
            .ray_hit(DVec3::new(0.25, 0.25, 2.0), DVec3::UNIT_Z.mul_scalar(-1.0))
            .unwrap();
        assert_eq!(hit.distance, 2.0);
        assert!(hit.front_face);
    }

    #[test]
    fn a_ray_from_behind_reports_a_back_face_rather_than_being_culled() {
        let hit = flat()
            .ray_hit(DVec3::new(0.25, 0.25, -2.0), DVec3::UNIT_Z)
            .unwrap();
        assert_eq!(hit.distance, 2.0);
        assert!(!hit.front_face);
    }

    #[test]
    fn a_triangle_behind_the_origin_reports_a_negative_distance() {
        let hit = flat()
            .ray_hit(DVec3::new(0.25, 0.25, 2.0), DVec3::UNIT_Z)
            .unwrap();
        assert_eq!(hit.distance, -2.0);
        assert!(!hit.front_face);
    }

    #[test]
    fn a_ray_parallel_to_the_plane_misses() {
        assert_eq!(
            flat().ray_hit(DVec3::new(0.25, 0.25, 1.0), DVec3::UNIT_X),
            None
        );
    }

    #[test]
    fn a_ray_outside_each_edge_misses() {
        let t = flat();
        // Beyond the u edge, beyond the v edge, and beyond the hypotenuse.
        assert_eq!(t.ray_hit(DVec3::new(-0.5, 0.25, -1.0), DVec3::UNIT_Z), None);
        assert_eq!(t.ray_hit(DVec3::new(0.25, -0.5, -1.0), DVec3::UNIT_Z), None);
        assert_eq!(t.ray_hit(DVec3::new(0.9, 0.9, -1.0), DVec3::UNIT_Z), None);
    }

    #[test]
    fn a_degenerate_triangle_is_parallel_to_every_ray() {
        let sliver = DTriangle::new(DVec3::ZERO, DVec3::UNIT_X, DVec3::UNIT_X);
        assert_eq!(sliver.ray_hit(DVec3::new(0.5, 0.0, -1.0), DVec3::UNIT_Z), None);
    }

    /// An unnormalised direction makes the reported distance the segment
    /// parameter, which is what a swept query wants.
    #[test]
    fn the_distance_is_in_units_of_the_direction_vector() {
        let hit = flat()
            .ray_hit(DVec3::new(0.25, 0.25, 2.0), DVec3::UNIT_Z.mul_scalar(-4.0))
            .unwrap();
        assert_eq!(hit.distance, 0.5);
    }

    /// A ray through a shared edge must hit at least one of the two triangles,
    /// which is what the outward slack buys.
    #[test]
    fn a_ray_exactly_through_a_shared_edge_still_hits() {
        let hit = flat().ray_hit(DVec3::new(0.5, 0.5, -1.0), DVec3::UNIT_Z);
        assert!(hit.is_some(), "a ray on the hypotenuse slipped through");
    }

    #[test]
    fn a_point_over_the_face_projects_into_the_interior() {
        assert_eq!(
            flat().closest_point(DVec3::new(0.25, 0.25, 5.0)),
            DVec3::new(0.25, 0.25, 0.0)
        );
    }

    #[test]
    fn each_vertex_region_returns_its_vertex() {
        let t = flat();
        assert_eq!(t.closest_point(DVec3::new(-1.0, -1.0, 0.0)), t.a);
        assert_eq!(t.closest_point(DVec3::new(3.0, -1.0, 0.0)), t.b);
        assert_eq!(t.closest_point(DVec3::new(-1.0, 3.0, 0.0)), t.c);
    }

    #[test]
    fn each_edge_region_returns_a_point_on_that_edge() {
        let t = flat();
        // Below edge AB (the x axis).
        assert_eq!(t.closest_point(DVec3::new(0.5, -1.0, 0.0)), DVec3::new(0.5, 0.0, 0.0));
        // Left of edge AC (the y axis).
        assert_eq!(t.closest_point(DVec3::new(-1.0, 0.5, 0.0)), DVec3::new(0.0, 0.5, 0.0));
        // Outside the hypotenuse BC.
        let on_bc = t.closest_point(DVec3::new(1.0, 1.0, 0.0));
        assert!((on_bc.x - 0.5).abs() < 1.0e-12);
        assert!((on_bc.y - 0.5).abs() < 1.0e-12);
        assert_eq!(on_bc.z, 0.0);
    }

    #[test]
    fn a_point_on_the_triangle_is_its_own_closest_point() {
        let t = flat();
        assert_eq!(t.closest_point(t.a), t.a);
        let inside = DVec3::new(0.25, 0.25, 0.0);
        assert_eq!(t.closest_point(inside), inside);
    }

    /// The closest point must never be farther than any other point of the
    /// triangle — the defining property, checked against a sampling of it.
    #[test]
    fn the_returned_point_is_no_farther_than_any_sampled_point() {
        let t = flat();
        let probes = [
            DVec3::new(2.0, -1.0, 1.0),
            DVec3::new(-3.0, 0.4, -2.0),
            DVec3::new(0.3, 0.3, 4.0),
            DVec3::new(1.5, 1.5, 0.0),
        ];
        probes.into_iter().for_each(|p| {
            let best = t.closest_point(p).distance(p);
            (0..=10).for_each(|i| {
                (0..=10).for_each(|j| {
                    let (u, v) = (f64::from(i) / 10.0, f64::from(j) / 10.0);
                    let inside = u + v <= 1.0;
                    let sample = t
                        .a
                        .add(t.b.subtract(t.a).mul_scalar(u))
                        .add(t.c.subtract(t.a).mul_scalar(v));
                    let d = sample.distance(p);
                    assert!(
                        !inside | (best <= d + 1.0e-12),
                        "closest {best} beat by sample {d}"
                    );
                });
            });
        });
    }

    #[test]
    fn approx_eq_compares_every_vertex() {
        let eps = Epsilon::DEFAULT_DOUBLE;
        assert!(flat().approx_eq(&flat(), eps));
        let moved = DTriangle::new(DVec3::new(0.1, 0.0, 0.0), flat().b, flat().c);
        assert!(!flat().approx_eq(&moved, eps));
        assert!(!flat().approx_eq(&DTriangle::new(flat().a, DVec3::ZERO, flat().c), eps));
        assert!(!flat().approx_eq(&DTriangle::new(flat().a, flat().b, DVec3::ZERO), eps));
    }
}
