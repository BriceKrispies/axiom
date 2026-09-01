//! Double-precision segment: closest features against another segment and
//! against a triangle.

use crate::approx_eq::ApproxEq;
use crate::dclosest_pair::DClosestPair;
use crate::dtriangle::DTriangle;
use crate::dvec3::DVec3;
use crate::epsilon::Epsilon;

/// Below this squared length a segment is treated as a point.
///
/// Squared, so it is `1e-9` in the units the dot products below already
/// produce — no square root on the degenerate check.
const DEGENERATE_LENGTH_SQUARED: f64 = 1.0e-9;

/// A line segment in `f64`.
///
/// The double-precision sibling of [`crate::Segment`]. It is the shape a
/// capsule reduces to: a capsule is the Minkowski sum of a segment and a
/// sphere, so *every* capsule query against static geometry is this segment's
/// closest-feature query plus a radius comparison. That is why
/// [`DSegment::closest_to_triangle`] carries the weight it does — capsule
/// sweeps, capsule overlap, ragdoll bone collision and character probes all
/// reduce to it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DSegment {
    pub start: DVec3,
    pub end: DVec3,
}

impl DSegment {
    /// Endpoint constructor.
    pub const fn new(start: DVec3, end: DVec3) -> Self {
        DSegment { start, end }
    }

    /// `end - start`.
    pub fn direction(self) -> DVec3 {
        self.end.subtract(self.start)
    }

    /// The point at parameter `t`, unclamped.
    pub fn at(self, t: f64) -> DVec3 {
        self.start.add(self.direction().mul_scalar(t))
    }

    /// Closest points between this segment and another (Ericson §5.1.9).
    ///
    /// Both degenerate cases are handled rather than guarded against: a
    /// zero-length segment is a point, and a point-vs-segment query is
    /// well-defined. Callers building a capsule from two coincident sphere
    /// centres depend on that — it is a sphere, not an error.
    pub fn closest_points_to(self, other: DSegment) -> DClosestPair {
        let d1 = self.direction();
        let d2 = other.direction();
        let r = self.start.subtract(other.start);
        let a = d1.length_squared();
        let e = d2.length_squared();
        let f = d2.dot(r);
        let c = d1.dot(r);

        let first_degenerate = a <= DEGENERATE_LENGTH_SQUARED;
        let second_degenerate = e <= DEGENERATE_LENGTH_SQUARED;

        // The general case: both segments have length.
        let b = d1.dot(d2);
        let denominator = a * e - b * b;
        let s_unclamped = ((b * f - c * e) / denominator).clamp(0.0, 1.0);
        // Parallel segments give a zero denominator; pinning `s` at one end is
        // as good as any other choice, and the `t` clamp below then finds the
        // real closest pair along the overlap.
        let s_general = [0.0, s_unclamped][usize::from(denominator != 0.0)];
        let t_general = (b * s_general + f) / e;

        // Re-solving `s` after clamping `t` is what keeps the pair on both
        // segments rather than on one segment and the other's infinite line.
        let t_below = t_general < 0.0;
        let t_above = t_general > 1.0;
        let s_at_start = (-c / a).clamp(0.0, 1.0);
        let s_at_end = ((b - c) / a).clamp(0.0, 1.0);

        let general = [
            [(s_general, t_general), (s_at_end, 1.0)][usize::from(t_above)],
            (s_at_start, 0.0),
        ][usize::from(t_below)];

        // Applied in reverse test order, earliest match winning: both
        // degenerate, then the first alone, then the second alone.
        let picked = [general, (s_at_start, 0.0)][usize::from(second_degenerate)];
        let picked = [picked, (0.0, (f / e).clamp(0.0, 1.0))][usize::from(first_degenerate)];
        let (s, t) = [picked, (0.0, 0.0)][usize::from(first_degenerate & second_degenerate)];

        let on_first = self.start.add(d1.mul_scalar(s));
        let on_second = other.start.add(d2.mul_scalar(t));
        DClosestPair {
            distance_squared: on_first.subtract(on_second).length_squared(),
            on_first,
            on_second,
            first_parameter: s,
            second_parameter: t,
        }
    }

    /// Closest points between this segment and a triangle.
    ///
    /// **The routine a triangle-soup collision world spends its time in.**
    ///
    /// Two stages. First a plane-straddle test: if the segment crosses the
    /// triangle's plane *inside* the triangle, the distance is exactly zero and
    /// the crossing point is the answer — no sub-query needed, and the common
    /// case of an actual intersection costs one plane test and one barycentric
    /// test. Otherwise the minimum is on the boundary, and there are exactly
    /// five candidates: each segment endpoint against the face, and the segment
    /// against each of the three edges.
    ///
    /// [`DClosestPair::second_parameter`] is left at `0.0` on every path but
    /// the straddle one. A triangle has no single parameter to report — the
    /// honest answer would be two barycentric coordinates — and inventing one
    /// would be worse than admitting there isn't one.
    pub fn closest_to_triangle(self, triangle: DTriangle) -> DClosestPair {
        let ab = triangle.b.subtract(triangle.a);
        let ac = triangle.c.subtract(triangle.a);
        let normal = ab.cross(ac);
        let height_start = normal.dot(self.start.subtract(triangle.a));
        let height_end = normal.dot(self.end.subtract(triangle.a));

        let straddles = (height_start > 0.0) != (height_end > 0.0);
        let height_span = height_start - height_end;
        let crossing_parameter = height_start / height_span;
        let crossing = self.at(crossing_parameter);

        // Barycentric containment of the crossing point.
        let v = crossing.subtract(triangle.a);
        let d00 = ab.length_squared();
        let d01 = ab.dot(ac);
        let d11 = ac.length_squared();
        let d20 = v.dot(ab);
        let d21 = v.dot(ac);
        let barycentric_denominator = d00 * d11 - d01 * d01;
        let bary_v = (d11 * d20 - d01 * d21) / barycentric_denominator;
        let bary_w = (d00 * d21 - d01 * d20) / barycentric_denominator;
        let inside = (bary_v >= 0.0) & (bary_w >= 0.0) & (bary_v + bary_w <= 1.0);

        let pierces = straddles
            & (height_span != 0.0)
            & (barycentric_denominator != 0.0)
            & inside;

        let pierced = DClosestPair {
            distance_squared: 0.0,
            on_first: crossing,
            on_second: crossing,
            first_parameter: crossing_parameter,
            second_parameter: 0.0,
        };

        // Candidate order is part of the contract: ties keep the earlier one
        // (see `DClosestPair::nearer`), and which candidate wins on a tie
        // decides the contact point a solver is handed.
        let endpoint = |point: DVec3, parameter: f64| {
            let on_triangle = triangle.closest_point(point);
            DClosestPair {
                distance_squared: point.subtract(on_triangle).length_squared(),
                on_first: point,
                on_second: on_triangle,
                first_parameter: parameter,
                second_parameter: 0.0,
            }
        };
        let edge = |from: DVec3, to: DVec3| self.closest_points_to(DSegment::new(from, to));

        let boundary = [
            endpoint(self.start, 0.0),
            endpoint(self.end, 1.0),
            edge(triangle.a, triangle.b),
            edge(triangle.b, triangle.c),
            edge(triangle.c, triangle.a),
        ]
        .into_iter()
        .fold(DClosestPair::FARTHEST, DClosestPair::nearer);

        // The boundary search reports no second parameter; only the straddle
        // path has one to give.
        let boundary = DClosestPair {
            second_parameter: 0.0,
            ..boundary
        };

        [boundary, pierced][usize::from(pierces)]
    }
}

impl ApproxEq for DSegment {
    fn approx_eq(&self, other: &Self, epsilon: Epsilon) -> bool {
        self.start.approx_eq(&other.start, epsilon) & self.end.approx_eq(&other.end, epsilon)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat_triangle() -> DTriangle {
        DTriangle::new(
            DVec3::ZERO,
            DVec3::new(1.0, 0.0, 0.0),
            DVec3::new(0.0, 1.0, 0.0),
        )
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1.0e-12
    }

    #[test]
    fn direction_and_at_describe_the_segment() {
        let s = DSegment::new(DVec3::ZERO, DVec3::new(2.0, 0.0, 0.0));
        assert_eq!(s.direction(), DVec3::new(2.0, 0.0, 0.0));
        assert_eq!(s.at(0.5), DVec3::new(1.0, 0.0, 0.0));
        assert_eq!(s.at(0.0), s.start);
        assert_eq!(s.at(1.0), s.end);
    }

    #[test]
    fn crossing_segments_meet_at_their_intersection() {
        let a = DSegment::new(DVec3::new(-1.0, 0.0, 0.0), DVec3::new(1.0, 0.0, 0.0));
        let b = DSegment::new(DVec3::new(0.0, -1.0, 0.0), DVec3::new(0.0, 1.0, 0.0));
        let pair = a.closest_points_to(b);
        assert!(close(pair.distance_squared, 0.0));
        assert!(close(pair.first_parameter, 0.5));
        assert!(close(pair.second_parameter, 0.5));
    }

    #[test]
    fn skew_segments_report_their_perpendicular_separation() {
        let a = DSegment::new(DVec3::new(-1.0, 0.0, 0.0), DVec3::new(1.0, 0.0, 0.0));
        let b = DSegment::new(DVec3::new(0.0, -1.0, 3.0), DVec3::new(0.0, 1.0, 3.0));
        let pair = a.closest_points_to(b);
        assert!(close(pair.distance(), 3.0));
    }

    #[test]
    fn parallel_segments_still_find_a_closest_pair() {
        let a = DSegment::new(DVec3::ZERO, DVec3::new(1.0, 0.0, 0.0));
        let b = DSegment::new(DVec3::new(0.0, 2.0, 0.0), DVec3::new(1.0, 2.0, 0.0));
        let pair = a.closest_points_to(b);
        assert!(close(pair.distance(), 2.0));
    }

    #[test]
    fn a_pair_beyond_an_end_clamps_onto_the_endpoint() {
        let a = DSegment::new(DVec3::ZERO, DVec3::new(1.0, 0.0, 0.0));
        // Entirely past `a`'s end, so `t` clamps and `s` re-solves to 1.
        let b = DSegment::new(DVec3::new(5.0, 1.0, 0.0), DVec3::new(6.0, 1.0, 0.0));
        let pair = a.closest_points_to(b);
        assert!(close(pair.first_parameter, 1.0));
        assert_eq!(pair.on_first, DVec3::new(1.0, 0.0, 0.0));
    }

    #[test]
    fn a_pair_before_the_start_clamps_the_other_way() {
        let a = DSegment::new(DVec3::ZERO, DVec3::new(1.0, 0.0, 0.0));
        let b = DSegment::new(DVec3::new(-6.0, 1.0, 0.0), DVec3::new(-5.0, 1.0, 0.0));
        let pair = a.closest_points_to(b);
        assert!(close(pair.first_parameter, 0.0));
    }

    #[test]
    fn a_degenerate_first_segment_is_a_point_query() {
        let point = DSegment::new(DVec3::new(0.5, 2.0, 0.0), DVec3::new(0.5, 2.0, 0.0));
        let line = DSegment::new(DVec3::ZERO, DVec3::new(1.0, 0.0, 0.0));
        let pair = point.closest_points_to(line);
        assert!(close(pair.distance(), 2.0));
        assert!(close(pair.second_parameter, 0.5));
    }

    #[test]
    fn a_degenerate_second_segment_is_a_point_query_too() {
        let line = DSegment::new(DVec3::ZERO, DVec3::new(1.0, 0.0, 0.0));
        let point = DSegment::new(DVec3::new(0.5, 2.0, 0.0), DVec3::new(0.5, 2.0, 0.0));
        let pair = line.closest_points_to(point);
        assert!(close(pair.distance(), 2.0));
        assert!(close(pair.first_parameter, 0.5));
    }

    #[test]
    fn two_degenerate_segments_are_a_point_to_point_distance() {
        let a = DSegment::new(DVec3::ZERO, DVec3::ZERO);
        let b = DSegment::new(DVec3::new(3.0, 4.0, 0.0), DVec3::new(3.0, 4.0, 0.0));
        let pair = a.closest_points_to(b);
        assert!(close(pair.distance(), 5.0));
        assert_eq!(pair.first_parameter, 0.0);
        assert_eq!(pair.second_parameter, 0.0);
    }

    #[test]
    fn a_segment_piercing_the_face_reports_zero_at_the_crossing() {
        let s = DSegment::new(DVec3::new(0.25, 0.25, -1.0), DVec3::new(0.25, 0.25, 1.0));
        let pair = s.closest_to_triangle(flat_triangle());
        assert_eq!(pair.distance_squared, 0.0);
        assert!(close(pair.first_parameter, 0.5));
        assert_eq!(pair.on_first, pair.on_second);
        assert!(close(pair.on_first.z, 0.0));
    }

    #[test]
    fn a_segment_crossing_the_plane_outside_the_triangle_falls_to_the_boundary() {
        let s = DSegment::new(DVec3::new(3.0, 3.0, -1.0), DVec3::new(3.0, 3.0, 1.0));
        let pair = s.closest_to_triangle(flat_triangle());
        assert!(pair.distance_squared > 0.0);
        // Closest feature is vertex B or C region; the point on the triangle
        // must be one of its own points.
        assert!(close(pair.on_second.z, 0.0));
    }

    #[test]
    fn a_segment_hovering_over_the_face_reports_its_height() {
        let s = DSegment::new(DVec3::new(0.25, 0.25, 2.0), DVec3::new(0.3, 0.3, 2.0));
        let pair = s.closest_to_triangle(flat_triangle());
        assert!(close(pair.distance(), 2.0));
    }

    #[test]
    fn a_segment_beside_an_edge_is_closest_to_that_edge() {
        let s = DSegment::new(DVec3::new(0.5, -2.0, 0.0), DVec3::new(0.5, -2.0, 1.0));
        let pair = s.closest_to_triangle(flat_triangle());
        assert!(close(pair.distance(), 2.0));
        assert!(close(pair.on_second.y, 0.0));
        assert!(close(pair.on_second.x, 0.5));
    }

    #[test]
    fn a_segment_entirely_on_one_side_never_takes_the_straddle_path() {
        let s = DSegment::new(DVec3::new(0.25, 0.25, 1.0), DVec3::new(0.25, 0.25, 3.0));
        let pair = s.closest_to_triangle(flat_triangle());
        assert!(close(pair.distance(), 1.0));
        assert_eq!(pair.second_parameter, 0.0);
    }

    /// A degenerate triangle has a zero barycentric denominator, so the
    /// straddle path must not be taken even when the segment crosses its plane.
    #[test]
    fn a_degenerate_triangle_falls_through_to_the_boundary_search() {
        let sliver = DTriangle::new(DVec3::ZERO, DVec3::UNIT_X, DVec3::UNIT_X);
        let s = DSegment::new(DVec3::new(0.5, -1.0, 0.0), DVec3::new(0.5, 1.0, 0.0));
        let pair = s.closest_to_triangle(sliver);
        assert!(pair.distance_squared.is_finite());
    }

    /// A segment lying exactly in the triangle's plane has a zero height span,
    /// which must not divide.
    #[test]
    fn a_coplanar_segment_does_not_divide_by_a_zero_height_span() {
        let s = DSegment::new(DVec3::new(-1.0, 0.25, 0.0), DVec3::new(2.0, 0.25, 0.0));
        let pair = s.closest_to_triangle(flat_triangle());
        assert!(pair.distance_squared.is_finite());
        assert!(close(pair.distance_squared, 0.0));
    }

    #[test]
    fn the_closest_pair_is_no_farther_than_any_sampled_pair() {
        let t = flat_triangle();
        let s = DSegment::new(DVec3::new(-0.4, 1.7, 0.9), DVec3::new(1.6, -0.3, 0.4));
        let best = s.closest_to_triangle(t).distance();
        (0..=20).for_each(|k| {
            let point = s.at(f64::from(k) / 20.0);
            let on_triangle = t.closest_point(point);
            assert!(best <= point.distance(on_triangle) + 1.0e-12);
        });
    }

    #[test]
    fn approx_eq_compares_both_endpoints() {
        let eps = Epsilon::DEFAULT_DOUBLE;
        let s = DSegment::new(DVec3::ZERO, DVec3::ONE);
        assert!(s.approx_eq(&s, eps));
        assert!(!s.approx_eq(&DSegment::new(DVec3::UNIT_X, DVec3::ONE), eps));
        assert!(!s.approx_eq(&DSegment::new(DVec3::ZERO, DVec3::UNIT_X), eps));
    }
}
