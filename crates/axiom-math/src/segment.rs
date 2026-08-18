//! A finite line segment, and the closest-point solves built on it.

use crate::approx_eq::ApproxEq;
use crate::epsilon::Epsilon;
use crate::math_error::MathError;
use crate::math_result::MathResult;
use crate::triangle::Triangle;
use crate::vec3::Vec3;

/// The smallest denominator a closest-point solve will divide by. Every solve
/// below pairs it with a numerator that is exactly zero in the degenerate case,
/// so the guarded quotient is `0`, not an infinity — that is what lets the
/// degenerate cases share the general code path instead of branching around it.
const SAFE_DENOMINATOR: f32 = f32::MIN_POSITIVE;

/// How small `a * e - b * b` may get, relative to `a * e`, before two segments
/// count as parallel and the general two-segment solve is abandoned for the
/// pinned `s = 0` form.
const PARALLEL_RATIO: f32 = 1.0e-6;

/// A finite line `start + t * (end - start)` for `t` in `[0, 1]`.
///
/// [`Segment::new`] rejects non-finite endpoints but **accepts** a zero-length
/// segment: a degenerate segment is a point, a [`crate::Capsule`] built on one
/// is a sphere, and every solve here is written to answer correctly for it. That
/// is deliberate — mesh and skeleton data contain degenerate segments, and a
/// constructor that rejected them would push a branch into every caller.
#[derive(Debug, Clone, Copy)]
pub struct Segment {
    start: Vec3,
    end: Vec3,
}

impl Segment {
    /// Construct from two finite endpoints. The endpoints may coincide.
    pub fn new(start: Vec3, end: Vec3) -> MathResult<Segment> {
        let all_finite = [start.x, start.y, start.z, end.x, end.y, end.z]
            .into_iter()
            .all(|component| component.is_finite());
        all_finite
            .then_some(Segment { start, end })
            .ok_or_else(|| MathError::non_finite_scalar("Segment endpoints must be finite"))
    }

    /// Construct from endpoints a caller has already validated. Internal to the
    /// layer: the swept tests translate and slice validated geometry thousands
    /// of times per frame, and re-validating known-finite points would only
    /// force a `Result` no caller could act on.
    pub(crate) const fn from_points(start: Vec3, end: Vec3) -> Segment {
        Segment { start, end }
    }

    /// The `t = 0` endpoint.
    pub const fn start(&self) -> Vec3 {
        self.start
    }

    /// The `t = 1` endpoint.
    pub const fn end(&self) -> Vec3 {
        self.end
    }

    /// `end - start`: the (unnormalized) direction, whose length is the
    /// segment's length.
    pub const fn delta(&self) -> Vec3 {
        self.end.subtract(self.start)
    }

    /// Euclidean length.
    pub fn length(&self) -> f32 {
        self.delta().length()
    }

    /// `start + t * delta`. The parameter is **not** clamped, so `t` outside
    /// `[0, 1]` names a point on the segment's infinite line.
    pub fn point_at(&self, t: f32) -> Vec3 {
        self.start.add(self.delta().mul_scalar(t))
    }

    /// Translate both endpoints by `offset` — the segment as it stands after
    /// part of a sweep.
    pub fn translated(&self, offset: Vec3) -> Segment {
        Segment {
            start: self.start.add(offset),
            end: self.end.add(offset),
        }
    }

    /// The parameter in `[0, 1]` of the closest point on this segment to `p`.
    /// A degenerate segment answers `0`: its numerator is exactly zero, so the
    /// guarded division yields `0` and the clamp keeps it there.
    pub fn closest_param_to(&self, p: Vec3) -> f32 {
        let d = self.delta();
        let projected = d.dot(p.subtract(self.start));
        (projected / d.length_squared().max(SAFE_DENOMINATOR)).clamp(0.0, 1.0)
    }

    /// The closest point on this segment to `p`.
    pub fn closest_point_to(&self, p: Vec3) -> Vec3 {
        self.point_at(self.closest_param_to(p))
    }

    /// The squared distance from `p` to this segment.
    pub fn distance_squared_to_point(&self, p: Vec3) -> f32 {
        self.closest_point_to(p).subtract(p).length_squared()
    }

    /// The parameters `(s, t)` of the closest pair of points — `self.point_at(s)`
    /// and `other.point_at(t)` — of two segments.
    ///
    /// This is the clamped-parameter solve: it solves the two infinite lines,
    /// clamps `s` into `[0, 1]`, re-solves `t` from the clamped `s`, and then
    /// re-solves `s` whenever `t` had to be clamped in turn. Parallel and
    /// degenerate inputs are not special-cased with a branch — they *collapse*
    /// the shared denominator, and the two selections below pick the pinned form
    /// instead of the general one.
    pub fn closest_params_to_segment(&self, other: &Segment) -> (f32, f32) {
        let d1 = self.delta();
        let d2 = other.delta();
        let r = self.start.subtract(other.start);
        let a = d1.length_squared();
        let e = d2.length_squared();
        let b = d1.dot(d2);
        let c = d1.dot(r);
        let f = d2.dot(r);
        // Cauchy-Schwarz makes `denom` non-negative, and exactly zero when the
        // two directions are parallel (either of them degenerate counts).
        let denom = a * e - b * b;
        let parallel = denom <= a * e * PARALLEL_RATIO;
        let general = (b * f - c * e) / denom.max(SAFE_DENOMINATOR);
        let s_line = [general, 0.0][usize::from(parallel)].clamp(0.0, 1.0);
        let t_line = (b * s_line + f) / e.max(SAFE_DENOMINATOR);
        // `other` degenerate is exactly the `t` clamped to zero case: its own
        // numerator is zero, so `t_line` is already 0 and only `s` needs the
        // pinned-endpoint re-solve.
        let below = (t_line < 0.0) | (e <= SAFE_DENOMINATOR);
        let above = !below & (t_line > 1.0);
        let s_below = (-c / a.max(SAFE_DENOMINATOR)).clamp(0.0, 1.0);
        let s_above = ((b - c) / a.max(SAFE_DENOMINATOR)).clamp(0.0, 1.0);
        let s_clamped = [s_line, s_above][usize::from(above)];
        (
            [s_clamped, s_below][usize::from(below)],
            t_line.clamp(0.0, 1.0),
        )
    }

    /// The closest pair of points `(on self, on other)` between two segments.
    pub fn closest_points_to_segment(&self, other: &Segment) -> (Vec3, Vec3) {
        let (s, t) = self.closest_params_to_segment(other);
        (self.point_at(s), other.point_at(t))
    }

    /// The closest pair of points `(on self, on triangle)` between this segment
    /// and `triangle`.
    ///
    /// The pair is found over the five boundary features that can realize it —
    /// this segment against each of the triangle's three edges, and each of this
    /// segment's endpoints against the triangle's face. That is exact while the
    /// two are disjoint, which is the state every overlap and swept query above
    /// uses it in; a segment that *pierces* the triangle is at distance zero and
    /// is detected by [`Triangle::intersect_segment`] instead.
    pub fn closest_points_to_triangle(&self, triangle: &Triangle) -> (Vec3, Vec3) {
        let edges = triangle.edges();
        let candidates = [
            self.closest_points_to_segment(&edges[0]),
            self.closest_points_to_segment(&edges[1]),
            self.closest_points_to_segment(&edges[2]),
            (self.end, triangle.closest_point_to(self.end)),
        ];
        candidates.into_iter().fold(
            (self.start, triangle.closest_point_to(self.start)),
            |best, candidate| {
                let closer = candidate.0.subtract(candidate.1).length_squared()
                    < best.0.subtract(best.1).length_squared();
                [best, candidate][usize::from(closer)]
            },
        )
    }
}

impl ApproxEq for Segment {
    fn approx_eq(&self, other: &Self, epsilon: Epsilon) -> bool {
        self.start.approx_eq(&other.start, epsilon) & self.end.approx_eq(&other.end, epsilon)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math_error_code::MathErrorCode;

    fn eps() -> Epsilon {
        Epsilon::DEFAULT
    }

    fn segment(start: Vec3, end: Vec3) -> Segment {
        Segment::new(start, end).unwrap()
    }

    #[test]
    fn new_rejects_non_finite_endpoints() {
        assert_eq!(
            Segment::new(Vec3::new(f32::NAN, 0.0, 0.0), Vec3::ZERO)
                .unwrap_err()
                .code(),
            MathErrorCode::NonFiniteScalar
        );
        assert_eq!(
            Segment::new(Vec3::ZERO, Vec3::new(0.0, f32::INFINITY, 0.0))
                .unwrap_err()
                .code(),
            MathErrorCode::NonFiniteScalar
        );
    }

    #[test]
    fn accessors_report_endpoints_delta_and_length() {
        let s = segment(Vec3::new(1.0, 0.0, 0.0), Vec3::new(4.0, 4.0, 0.0));
        assert!(s.start().approx_eq(&Vec3::new(1.0, 0.0, 0.0), eps()));
        assert!(s.end().approx_eq(&Vec3::new(4.0, 4.0, 0.0), eps()));
        assert!(s.delta().approx_eq(&Vec3::new(3.0, 4.0, 0.0), eps()));
        assert_eq!(s.length(), 5.0);
    }

    #[test]
    fn point_at_is_unclamped() {
        let s = segment(Vec3::ZERO, Vec3::new(2.0, 0.0, 0.0));
        assert!(s.point_at(0.5).approx_eq(&Vec3::new(1.0, 0.0, 0.0), eps()));
        assert!(s.point_at(2.0).approx_eq(&Vec3::new(4.0, 0.0, 0.0), eps()));
        assert!(s
            .point_at(-1.0)
            .approx_eq(&Vec3::new(-2.0, 0.0, 0.0), eps()));
    }

    #[test]
    fn translated_moves_both_endpoints() {
        let s = segment(Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0));
        let moved = s.translated(Vec3::new(0.0, 3.0, 0.0));
        assert!(moved.start().approx_eq(&Vec3::new(0.0, 3.0, 0.0), eps()));
        assert!(moved.end().approx_eq(&Vec3::new(1.0, 3.0, 0.0), eps()));
    }

    #[test]
    fn closest_point_clamps_to_each_endpoint_and_interior() {
        let s = segment(Vec3::ZERO, Vec3::new(4.0, 0.0, 0.0));
        assert_eq!(s.closest_param_to(Vec3::new(-3.0, 1.0, 0.0)), 0.0);
        assert_eq!(s.closest_param_to(Vec3::new(9.0, 1.0, 0.0)), 1.0);
        assert_eq!(s.closest_param_to(Vec3::new(1.0, 2.0, 0.0)), 0.25);
        assert!(s
            .closest_point_to(Vec3::new(1.0, 2.0, 0.0))
            .approx_eq(&Vec3::new(1.0, 0.0, 0.0), eps()));
        assert_eq!(s.distance_squared_to_point(Vec3::new(1.0, 2.0, 0.0)), 4.0);
    }

    #[test]
    fn degenerate_segment_answers_its_single_point() {
        let point = segment(Vec3::new(2.0, 2.0, 2.0), Vec3::new(2.0, 2.0, 2.0));
        assert_eq!(point.length(), 0.0);
        assert_eq!(point.closest_param_to(Vec3::new(9.0, -4.0, 0.0)), 0.0);
        assert!(point
            .closest_point_to(Vec3::new(9.0, -4.0, 0.0))
            .approx_eq(&Vec3::new(2.0, 2.0, 2.0), eps()));
    }

    #[test]
    fn crossing_segments_meet_in_both_interiors() {
        let a = segment(Vec3::ZERO, Vec3::new(2.0, 0.0, 0.0));
        let b = segment(Vec3::new(1.0, -1.0, 1.0), Vec3::new(1.0, 1.0, 1.0));
        let (s, t) = a.closest_params_to_segment(&b);
        assert_eq!((s, t), (0.5, 0.5));
        let (pa, pb) = a.closest_points_to_segment(&b);
        assert!(pa.approx_eq(&Vec3::new(1.0, 0.0, 0.0), eps()));
        assert!(pb.approx_eq(&Vec3::new(1.0, 0.0, 1.0), eps()));
    }

    #[test]
    fn parallel_offset_segments_pin_the_overlap() {
        let a = segment(Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0));
        let b = segment(Vec3::new(0.5, 1.0, 0.0), Vec3::new(1.5, 1.0, 0.0));
        let (s, t) = a.closest_params_to_segment(&b);
        assert_eq!(t, 0.0);
        assert_eq!(s, 0.5);
        let (pa, pb) = a.closest_points_to_segment(&b);
        assert_eq!(pa.subtract(pb).length(), 1.0);
    }

    #[test]
    fn collinear_disjoint_segments_meet_at_facing_endpoints() {
        let a = segment(Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0));
        let b = segment(Vec3::new(3.0, 0.0, 0.0), Vec3::new(5.0, 0.0, 0.0));
        let (pa, pb) = a.closest_points_to_segment(&b);
        assert!(pa.approx_eq(&Vec3::new(1.0, 0.0, 0.0), eps()));
        assert!(pb.approx_eq(&Vec3::new(3.0, 0.0, 0.0), eps()));
    }

    #[test]
    fn skew_segments_clamp_the_second_parameter_high() {
        // `b` runs away from `a` before its own closest approach, so the
        // re-solve after clamping `t` to 1 is the arm under test.
        let a = segment(Vec3::ZERO, Vec3::new(4.0, 0.0, 0.0));
        let b = segment(Vec3::new(2.0, 1.0, -3.0), Vec3::new(2.0, 1.0, -1.0));
        let (s, t) = a.closest_params_to_segment(&b);
        assert_eq!(t, 1.0);
        assert_eq!(s, 0.5);
    }

    #[test]
    fn degenerate_second_segment_becomes_a_point_query() {
        let a = segment(Vec3::ZERO, Vec3::new(4.0, 0.0, 0.0));
        let point = segment(Vec3::new(1.0, 5.0, 0.0), Vec3::new(1.0, 5.0, 0.0));
        let (s, t) = a.closest_params_to_segment(&point);
        assert_eq!((s, t), (0.25, 0.0));
    }

    #[test]
    fn both_segments_degenerate_answers_their_two_points() {
        let a = segment(Vec3::ZERO, Vec3::ZERO);
        let b = segment(Vec3::new(3.0, 0.0, 0.0), Vec3::new(3.0, 0.0, 0.0));
        assert_eq!(a.closest_params_to_segment(&b), (0.0, 0.0));
    }

    #[test]
    fn first_segment_degenerate_projects_onto_the_second() {
        let point = segment(Vec3::new(1.0, 2.0, 0.0), Vec3::new(1.0, 2.0, 0.0));
        let b = segment(Vec3::ZERO, Vec3::new(4.0, 0.0, 0.0));
        let (s, t) = point.closest_params_to_segment(&b);
        assert_eq!((s, t), (0.0, 0.25));
    }
}

#[cfg(test)]
mod triangle_tests {
    use super::*;

    fn eps() -> Epsilon {
        Epsilon::DEFAULT
    }

    fn unit_triangle() -> Triangle {
        Triangle::new(
            Vec3::ZERO,
            Vec3::new(4.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 4.0),
        )
        .unwrap()
    }

    #[test]
    fn segment_above_the_face_meets_it_perpendicularly() {
        let tri = unit_triangle();
        let seg = Segment::new(Vec3::new(1.0, 2.0, 1.0), Vec3::new(1.0, 5.0, 1.0)).unwrap();
        let (on_segment, on_triangle) = seg.closest_points_to_triangle(&tri);
        assert!(on_segment.approx_eq(&Vec3::new(1.0, 2.0, 1.0), eps()));
        assert!(on_triangle.approx_eq(&Vec3::new(1.0, 0.0, 1.0), eps()));
    }

    #[test]
    fn segment_beside_an_edge_meets_that_edge() {
        let tri = unit_triangle();
        let seg = Segment::new(Vec3::new(-2.0, 0.0, 1.0), Vec3::new(-2.0, 0.0, 3.0)).unwrap();
        let (on_segment, on_triangle) = seg.closest_points_to_triangle(&tri);
        assert_eq!(on_segment.subtract(on_triangle).length(), 2.0);
        assert_eq!(on_triangle.x, 0.0);
    }

    #[test]
    fn segment_past_a_vertex_meets_that_vertex() {
        let tri = unit_triangle();
        let seg = Segment::new(Vec3::new(8.0, 1.0, 0.0), Vec3::new(9.0, 1.0, 0.0)).unwrap();
        let (on_segment, on_triangle) = seg.closest_points_to_triangle(&tri);
        assert!(on_segment.approx_eq(&Vec3::new(8.0, 1.0, 0.0), eps()));
        assert!(on_triangle.approx_eq(&Vec3::new(4.0, 0.0, 0.0), eps()));
    }
}
