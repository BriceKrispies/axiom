//! Parametric curves: polyline, chained cubic Bézier, and Catmull-Rom.
//!
//! A [`Curve`] is *only* the mathematical curve — a kind plus its control
//! points. It knows nothing about meshes, sweeps, or frames; a higher tier
//! turns samples into geometry.
//!
//! ## Parameterization
//! `t` is a [`Ratio`] in `0 ..= 1` spanning the **whole** curve, split evenly
//! across its spans (each polyline segment, each Bézier segment, each
//! Catmull-Rom span gets an equal slice of `t`). That is deliberately *not*
//! arc-length parameterization: a straight span and a tight curl consume the
//! same amount of `t`. [`Curve::sample_uniform`] is the function that undoes
//! that, returning points equally spaced by arc length.
//!
//! **The Catmull-Rom variant is the *uniform* (not centripetal) formulation**
//! — the classic `α = 0` basis. It is the simpler and cheaper of the two, and
//! it is what the sweep tier's tessellation expects: control points that are
//! roughly evenly spaced (the usual authoring case) produce a well-behaved
//! spline. Its known weakness is that wildly uneven spacing can overshoot or
//! cusp; centripetal (`α = 0.5`) would trade cost for that robustness and is a
//! future kind, not a silent change to this one.
//!
//! ## Dispatch
//! Evaluation and differentiation are `const` function tables indexed by
//! [`CurveKind::raw`] — the same idiom as `axiom-proc-mesh`'s operator table.
//! There is no `match` over the kind anywhere in this file.
//!
//! ## Derivatives
//! Every kind has a cheap closed-form derivative (a segment vector for a
//! polyline, the Bernstein/Catmull-Rom basis derivative for the two splines),
//! so no finite difference is used at all. [`Curve::tangent_at`] normalizes
//! that derivative and surfaces a zero-length derivative as the vector layer's
//! own [`crate::MathErrorCode::NormalizeZeroLength`] — a genuinely undefined
//! direction, reported by the primitive that discovered it rather than
//! relabelled.

use axiom_kernel::{Meters, Ratio};

use crate::curve_kind::CurveKind;
use crate::curve_sample::CurveSample;
use crate::math_error::MathError;
use crate::math_result::MathResult;
use crate::vec3::Vec3;

/// Chord subdivisions built per requested sample when inverting arc length.
/// Sixteen chords per output sample keeps the piecewise-linear approximation of
/// the length table well under the spacing being solved for.
const ARC_TABLE_DENSITY: u32 = 16;
/// Never fewer than this many chords, so a 2-sample request still measures the
/// curve rather than one straight chord end to end.
const ARC_TABLE_MIN_NODES: u32 = 64;
/// Never more than this many chords, so a huge `count` cannot make the table
/// allocation unbounded.
const ARC_TABLE_MAX_NODES: u32 = 8192;

/// A curve evaluator: control points and a clamped parameter in, a point (or a
/// derivative) out.
type CurveFn = fn(&[Vec3], f32) -> Vec3;

/// Position dispatch. Order mirrors [`CurveKind`] so `kind.raw()` selects.
const EVAL: [CurveFn; 3] = [polyline_at, bezier_at, catmull_at];

/// Derivative dispatch (un-normalized `dP/dt`, up to a positive scale). Order
/// mirrors [`CurveKind`] so `kind.raw()` selects.
const DERIVATIVE: [CurveFn; 3] = [polyline_derivative, bezier_derivative, catmull_derivative];

/// A validated parametric curve.
///
/// Construction is the only place validity is decided: a `Curve` that exists
/// has enough points for its kind, all-finite coordinates, and no two
/// *consecutive* points at the same place (a zero-length span would make the
/// derivative — and therefore every tangent and frame built on it —
/// undefined). Every evaluation below is therefore total.
#[derive(Debug, Clone, PartialEq)]
pub struct Curve {
    kind: CurveKind,
    points: Vec<Vec3>,
}

/// Linear blend, `u = 0 -> a`, `u = 1 -> b`.
fn lerp(a: Vec3, b: Vec3, u: f32) -> Vec3 {
    a.mul_scalar(1.0 - u).add(b.mul_scalar(u))
}

/// Split a whole-curve `t` into `(span index, local u)` across `spans` spans.
/// The index is clamped to the last span so `t = 1` lands at the end of the
/// final span rather than one past it.
fn span_of(spans: usize, t: f32) -> (usize, f32) {
    let scaled = t * spans as f32;
    let index = (scaled.floor().max(0.0) as usize).min(spans.saturating_sub(1));
    (index, scaled - index as f32)
}

/// Piecewise-linear position.
fn polyline_at(points: &[Vec3], t: f32) -> Vec3 {
    let (i, u) = span_of(points.len().saturating_sub(1), t);
    lerp(points[i], points[i + 1], u)
}

/// Piecewise-linear derivative: the current segment's vector. Non-zero because
/// construction rejects coincident consecutive points. At an interior joint the
/// segment *starting* there is chosen, so a corner's tangent is its outgoing
/// direction.
fn polyline_derivative(points: &[Vec3], t: f32) -> Vec3 {
    let (i, _) = span_of(points.len().saturating_sub(1), t);
    points[i + 1].subtract(points[i])
}

/// The four control points of Bézier segment `i`.
fn bezier_span(points: &[Vec3], t: f32) -> ([Vec3; 4], f32) {
    let (i, u) = span_of(points.len().saturating_sub(1) / 3, t);
    let base = i * 3;
    (
        [
            points[base],
            points[base + 1],
            points[base + 2],
            points[base + 3],
        ],
        u,
    )
}

/// Cubic Bernstein position.
fn bezier_at(points: &[Vec3], t: f32) -> Vec3 {
    let ([p0, p1, p2, p3], u) = bezier_span(points, t);
    let v = 1.0 - u;
    p0.mul_scalar(v * v * v)
        .add(p1.mul_scalar(3.0 * v * v * u))
        .add(p2.mul_scalar(3.0 * v * u * u))
        .add(p3.mul_scalar(u * u * u))
}

/// Cubic Bernstein derivative: `3(1-u)^2(P1-P0) + 6(1-u)u(P2-P1) + 3u^2(P3-P2)`.
fn bezier_derivative(points: &[Vec3], t: f32) -> Vec3 {
    let ([p0, p1, p2, p3], u) = bezier_span(points, t);
    let v = 1.0 - u;
    p1.subtract(p0)
        .mul_scalar(3.0 * v * v)
        .add(p2.subtract(p1).mul_scalar(6.0 * v * u))
        .add(p3.subtract(p2).mul_scalar(3.0 * u * u))
}

/// The four control points of Catmull-Rom span `i`. Span `i` interpolates
/// `points[i + 1]` to `points[i + 2]`; `points[i]` and `points[i + 3]` only
/// shape the tangents.
fn catmull_span(points: &[Vec3], t: f32) -> ([Vec3; 4], f32) {
    let (i, u) = span_of(points.len().saturating_sub(3), t);
    ([points[i], points[i + 1], points[i + 2], points[i + 3]], u)
}

/// Uniform Catmull-Rom position (the `α = 0` basis).
fn catmull_at(points: &[Vec3], t: f32) -> Vec3 {
    let ([p0, p1, p2, p3], u) = catmull_span(points, t);
    let quad = p0
        .mul_scalar(2.0)
        .add(p1.mul_scalar(-5.0))
        .add(p2.mul_scalar(4.0))
        .subtract(p3);
    let cubic = p1
        .mul_scalar(3.0)
        .subtract(p0)
        .subtract(p2.mul_scalar(3.0))
        .add(p3);
    p1.mul_scalar(2.0)
        .add(p2.subtract(p0).mul_scalar(u))
        .add(quad.mul_scalar(u * u))
        .add(cubic.mul_scalar(u * u * u))
        .mul_scalar(0.5)
}

/// Uniform Catmull-Rom derivative — the basis above differentiated in `u`.
fn catmull_derivative(points: &[Vec3], t: f32) -> Vec3 {
    let ([p0, p1, p2, p3], u) = catmull_span(points, t);
    let quad = p0
        .mul_scalar(2.0)
        .add(p1.mul_scalar(-5.0))
        .add(p2.mul_scalar(4.0))
        .subtract(p3);
    let cubic = p1
        .mul_scalar(3.0)
        .subtract(p0)
        .subtract(p2.mul_scalar(3.0))
        .add(p3);
    p2.subtract(p0)
        .add(quad.mul_scalar(2.0 * u))
        .add(cubic.mul_scalar(3.0 * u * u))
        .mul_scalar(0.5)
}

impl Curve {
    /// A piecewise-linear curve through every point. Needs `>= 2` points.
    pub fn polyline(points: Vec<Vec3>) -> MathResult<Curve> {
        let enough = points.len() >= 2;
        Curve::build(
            CurveKind::Polyline,
            points,
            enough,
            "a polyline curve needs at least 2 points",
        )
    }

    /// A chain of cubic Bézier segments, segment `i` consuming
    /// `points[3i ..= 3i + 3]`. Needs `3n + 1` points with `n >= 1`.
    pub fn cubic_bezier(points: Vec<Vec3>) -> MathResult<Curve> {
        let len = points.len();
        let enough = (len >= 4) & (len % 3 == 1);
        Curve::build(
            CurveKind::CubicBezier,
            points,
            enough,
            "a cubic Bezier curve needs 3n+1 points with n >= 1",
        )
    }

    /// A uniform Catmull-Rom spline through `points[1 ..= len - 2]`; the first
    /// and last points are tangent controls. Needs `>= 4` points.
    pub fn catmull_rom(points: Vec<Vec3>) -> MathResult<Curve> {
        let enough = points.len() >= 4;
        Curve::build(
            CurveKind::CatmullRom,
            points,
            enough,
            "a Catmull-Rom curve needs at least 4 points",
        )
    }

    /// The one validation gate. `enough` is the kind-specific point-count rule;
    /// finiteness and consecutive-distinctness are universal.
    fn build(
        kind: CurveKind,
        points: Vec<Vec3>,
        enough: bool,
        shortfall: &'static str,
    ) -> MathResult<Curve> {
        let finite = points
            .iter()
            .all(|p| p.x.is_finite() & p.y.is_finite() & p.z.is_finite());
        let distinct = points
            .windows(2)
            .all(|w| w[0].subtract(w[1]).length_squared() != 0.0);
        // Report the *first* rule that failed: index 0 while the count is
        // wrong, 1 once the count is right but a coordinate is not finite,
        // 2 otherwise. A table index, not a chain of ifs.
        let stage = usize::from(enough) + usize::from(enough & finite);
        let reason = [
            shortfall,
            "curve points must all be finite",
            "consecutive curve points must be distinct",
        ][stage];
        (enough & finite & distinct)
            .then_some(points)
            .map(|points| Curve { kind, points })
            .ok_or_else(|| MathError::invalid_curve(reason))
    }

    /// Which parametric family this curve belongs to.
    pub const fn kind(&self) -> CurveKind {
        self.kind
    }

    /// The control points, in construction order.
    pub fn points(&self) -> &[Vec3] {
        &self.points
    }

    /// Position at `t`, where `t` spans the whole curve. Out-of-range ratios
    /// are clamped into `0 ..= 1` rather than extrapolated.
    pub fn position_at(&self, t: Ratio) -> Vec3 {
        self.evaluate(t.get())
    }

    /// Unit-length curve direction at `t`. Fails with
    /// [`crate::MathErrorCode::NormalizeZeroLength`] where the derivative
    /// vanishes (possible for a spline whose control points double back within
    /// one span, even though no two *consecutive* points coincide).
    pub fn tangent_at(&self, t: Ratio) -> MathResult<Vec3> {
        self.differentiate(t.get()).normalize()
    }

    /// Chord-length approximation of total arc length over `samples`
    /// subdivisions. `samples` is raised to at least `1` internally, so the
    /// step can never be a division by zero; more subdivisions converge from
    /// below toward the true length.
    pub fn arc_length(&self, samples: u32) -> Meters {
        let steps = samples.max(1);
        Meters::finite_or_zero(
            (0..steps)
                .map(|i| {
                    let a = self.evaluate(i as f32 / steps as f32);
                    let b = self.evaluate((i + 1) as f32 / steps as f32);
                    a.distance(b)
                })
                .sum(),
        )
    }

    /// `count` samples spaced (approximately) **equally by arc length**, from
    /// the start of the curve to its end. Needs `count >= 2`.
    ///
    /// This is the reason the type exists: sweeping a profile along a
    /// parameter-uniform curve bunches the geometry wherever the curve is
    /// tight. The inversion builds a cumulative chord-length table over a dense
    /// node grid, then for each target distance finds the bracketing pair of
    /// nodes and linearly interpolates the parameter between them. The first
    /// sample's distance is exactly `0` and the last is exactly the measured
    /// total.
    pub fn sample_uniform(&self, count: u32) -> MathResult<Vec<CurveSample>> {
        (count >= 2)
            .then_some(count)
            .ok_or_else(|| {
                MathError::invalid_curve("a uniform curve sampling needs at least 2 samples")
            })
            .and_then(|count| self.resample(count))
    }

    /// Raw position evaluation with a clamped `f32` parameter — the internal
    /// path that avoids re-wrapping a `Ratio` per node of the length table.
    fn evaluate(&self, t: f32) -> Vec3 {
        EVAL[self.kind.raw() as usize](&self.points, t.clamp(0.0, 1.0))
    }

    /// Raw derivative evaluation with a clamped `f32` parameter.
    fn differentiate(&self, t: f32) -> Vec3 {
        DERIVATIVE[self.kind.raw() as usize](&self.points, t.clamp(0.0, 1.0))
    }

    /// Cumulative chord length at each of `nodes + 1` evenly-parameterized
    /// nodes. `table[0]` is `0`; `table[nodes]` is the measured total.
    fn arc_table(&self, nodes: u32) -> Vec<f32> {
        let positions: Vec<Vec3> = (0..=nodes)
            .map(|i| self.evaluate(i as f32 / nodes as f32))
            .collect();
        core::iter::once(0.0)
            .chain(positions.windows(2).scan(0.0f32, |run, w| {
                *run += w[0].distance(w[1]);
                Some(*run)
            }))
            .collect()
    }

    /// The arc-length inversion proper. `count` is already `>= 2`.
    fn resample(&self, count: u32) -> MathResult<Vec<CurveSample>> {
        let nodes = count
            .saturating_sub(1)
            .saturating_mul(ARC_TABLE_DENSITY)
            .clamp(ARC_TABLE_MIN_NODES, ARC_TABLE_MAX_NODES);
        let table = self.arc_table(nodes);
        let total = table[nodes as usize];
        let last = count.saturating_sub(1) as f32;
        (0..count)
            .map(|j| self.sample_at_distance(&table, nodes, total * (j as f32 / last)))
            .collect()
    }

    /// One sample at a target arc length, found by inverting the table.
    fn sample_at_distance(
        &self,
        table: &[f32],
        nodes: u32,
        target: f32,
    ) -> MathResult<CurveSample> {
        // First node at or past the target. `target <= table[nodes]` always
        // holds, so the fallback is a defensive floating-point backstop.
        let hi = table
            .iter()
            .position(|&d| d >= target)
            .unwrap_or(nodes as usize);
        let lo = hi.saturating_sub(1);
        let span = table[hi] - table[lo];
        // `span == 0` at the very first node (and for a degenerate zero-length
        // table); the quotient there is discarded by the table select.
        let local = [(target - table[lo]) / span, 0.0][usize::from(span == 0.0)];
        let t = (lo as f32 + local) / nodes as f32;
        self.differentiate(t).normalize().map(|tangent| {
            CurveSample::new(
                self.evaluate(t),
                tangent,
                Ratio::finite_or_zero(t),
                Meters::finite_or_zero(target),
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math_error_code::MathErrorCode;

    fn ratio(v: f32) -> Ratio {
        Ratio::new(v).unwrap()
    }

    fn close(a: f32, b: f32, eps: f32) -> bool {
        (a - b).abs() <= eps
    }

    fn vec_close(a: Vec3, b: Vec3, eps: f32) -> bool {
        close(a.x, b.x, eps) && close(a.y, b.y, eps) && close(a.z, b.z, eps)
    }

    fn straight() -> Curve {
        Curve::polyline(vec![Vec3::ZERO, Vec3::new(2.0, 0.0, 0.0)]).unwrap()
    }

    /// An L: 3 along +X, then 1 along +Y. Total length 4, but the corner sits
    /// at t = 0.5 — parameter-uniform sampling would bunch badly.
    fn ell() -> Curve {
        Curve::polyline(vec![
            Vec3::ZERO,
            Vec3::new(3.0, 0.0, 0.0),
            Vec3::new(3.0, 1.0, 0.0),
        ])
        .unwrap()
    }

    fn arc_bezier() -> Curve {
        Curve::cubic_bezier(vec![
            Vec3::ZERO,
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(1.0, 2.0, 0.0),
            Vec3::new(2.0, 2.0, 0.0),
        ])
        .unwrap()
    }

    fn spline() -> Curve {
        Curve::catmull_rom(vec![
            Vec3::new(-1.0, 0.0, 0.0),
            Vec3::ZERO,
            Vec3::new(1.0, 1.0, 0.0),
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(3.0, 0.0, 0.0),
        ])
        .unwrap()
    }

    #[test]
    fn kind_and_points_round_trip() {
        let c = straight();
        assert_eq!(c.kind(), CurveKind::Polyline);
        assert_eq!(c.points(), &[Vec3::ZERO, Vec3::new(2.0, 0.0, 0.0)]);
        assert_eq!(arc_bezier().kind(), CurveKind::CubicBezier);
        assert_eq!(spline().kind(), CurveKind::CatmullRom);
    }

    #[test]
    fn curve_clones_debugs_and_compares() {
        let c = straight();
        let copy = c.clone();
        assert_eq!(c, copy);
        assert_ne!(c, ell());
        assert!(format!("{c:?}").starts_with("Curve"));
    }

    #[test]
    fn polyline_midpoint_and_tangent_are_exact() {
        let c = straight();
        assert_eq!(c.position_at(ratio(0.5)), Vec3::new(1.0, 0.0, 0.0));
        assert_eq!(c.position_at(ratio(0.0)), Vec3::ZERO);
        assert_eq!(c.position_at(ratio(1.0)), Vec3::new(2.0, 0.0, 0.0));
        assert_eq!(c.tangent_at(ratio(0.5)).unwrap(), Vec3::UNIT_X);
        assert_eq!(c.tangent_at(ratio(1.0)).unwrap(), Vec3::UNIT_X);
    }

    #[test]
    fn polyline_walks_each_segment_in_turn() {
        let c = ell();
        assert_eq!(c.position_at(ratio(0.25)), Vec3::new(1.5, 0.0, 0.0));
        assert_eq!(c.position_at(ratio(0.5)), Vec3::new(3.0, 0.0, 0.0));
        assert_eq!(c.position_at(ratio(0.75)), Vec3::new(3.0, 0.5, 0.0));
        assert_eq!(c.tangent_at(ratio(0.25)).unwrap(), Vec3::UNIT_X);
        assert_eq!(c.tangent_at(ratio(0.75)).unwrap(), Vec3::UNIT_Y);
    }

    #[test]
    fn out_of_range_parameters_clamp_rather_than_extrapolate() {
        let c = straight();
        assert_eq!(c.position_at(ratio(-4.0)), Vec3::ZERO);
        assert_eq!(c.position_at(ratio(9.0)), Vec3::new(2.0, 0.0, 0.0));
    }

    #[test]
    fn bezier_endpoints_are_its_outer_control_points() {
        let c = arc_bezier();
        assert!(vec_close(c.position_at(ratio(0.0)), Vec3::ZERO, 1.0e-6));
        assert!(vec_close(
            c.position_at(ratio(1.0)),
            Vec3::new(2.0, 2.0, 0.0),
            1.0e-6
        ));
        // Start tangent points along P1 - P0 (+Y), end tangent along P3 - P2 (+X).
        assert!(vec_close(
            c.tangent_at(ratio(0.0)).unwrap(),
            Vec3::UNIT_Y,
            1.0e-6
        ));
        assert!(vec_close(
            c.tangent_at(ratio(1.0)).unwrap(),
            Vec3::UNIT_X,
            1.0e-6
        ));
    }

    #[test]
    fn chained_bezier_segments_meet_at_the_shared_control_point() {
        let joint = Vec3::new(3.0, 0.0, 0.0);
        let c = Curve::cubic_bezier(vec![
            Vec3::ZERO,
            Vec3::new(1.0, 1.0, 0.0),
            Vec3::new(2.0, 1.0, 0.0),
            joint,
            Vec3::new(4.0, -1.0, 0.0),
            Vec3::new(5.0, -1.0, 0.0),
            Vec3::new(6.0, 0.0, 0.0),
        ])
        .unwrap();
        assert!(vec_close(c.position_at(ratio(0.5)), joint, 1.0e-5));
        assert!(vec_close(
            c.position_at(ratio(1.0)),
            Vec3::new(6.0, 0.0, 0.0),
            1.0e-5
        ));
    }

    #[test]
    fn catmull_rom_passes_through_the_interior_points() {
        let c = spline();
        assert!(vec_close(c.position_at(ratio(0.0)), Vec3::ZERO, 1.0e-5));
        assert!(vec_close(
            c.position_at(ratio(0.5)),
            Vec3::new(1.0, 1.0, 0.0),
            1.0e-5
        ));
        assert!(vec_close(
            c.position_at(ratio(1.0)),
            Vec3::new(2.0, 0.0, 0.0),
            1.0e-5
        ));
    }

    #[test]
    fn catmull_rom_tangent_matches_the_uniform_basis() {
        // Uniform Catmull-Rom tangent at an interior knot is (P2 - P0) / 2,
        // here ((1,1,0) - (-1,0,0)) / 2 = (1, 0.5, 0) -> normalized.
        let t = spline().tangent_at(ratio(0.0)).unwrap();
        let expected = Vec3::new(1.0, 0.5, 0.0).normalize().unwrap();
        assert!(vec_close(t, expected, 1.0e-5));
        assert!(close(t.length(), 1.0, 1.0e-6));
    }

    #[test]
    fn every_kind_returns_a_unit_tangent() {
        for c in [straight(), ell(), arc_bezier(), spline()] {
            for step in 0..=10 {
                let t = c.tangent_at(ratio(step as f32 / 10.0)).unwrap();
                assert!(close(t.length(), 1.0, 1.0e-5), "tangent must be unit");
            }
        }
    }

    #[test]
    fn a_vanishing_derivative_is_rejected() {
        // A Bezier that doubles straight back on itself: no two consecutive
        // control points coincide, but dP/du is zero at the midpoint.
        let c = Curve::cubic_bezier(vec![
            Vec3::ZERO,
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::ZERO,
            Vec3::new(1.0, 0.0, 0.0),
        ])
        .unwrap();
        assert_eq!(
            c.tangent_at(ratio(0.5)).unwrap_err().code(),
            MathErrorCode::NormalizeZeroLength
        );
    }

    #[test]
    fn arc_length_of_a_polyline_is_exact() {
        assert_eq!(straight().arc_length(8).get(), 2.0);
        assert!(close(ell().arc_length(64).get(), 4.0, 1.0e-4));
    }

    #[test]
    fn arc_length_clamps_zero_samples_to_one_chord() {
        // Zero subdivisions cannot divide by zero; it degrades to the single
        // start-to-end chord, which for a straight line is the exact length.
        assert_eq!(straight().arc_length(0).get(), 2.0);
        // On the L the one chord is the hypotenuse, shorter than the true 4.
        let coarse = ell().arc_length(0).get();
        assert!(close(coarse, 10.0f32.sqrt(), 1.0e-5));
        assert!(coarse < ell().arc_length(64).get());
    }

    #[test]
    fn arc_length_converges_from_below_on_a_curve() {
        let c = arc_bezier();
        let coarse = c.arc_length(2).get();
        let fine = c.arc_length(512).get();
        assert!(coarse < fine, "chord sums under-measure a curve");
        assert!(close(fine, c.arc_length(1024).get(), 1.0e-3));
    }

    #[test]
    fn sample_uniform_spaces_an_l_shape_by_arc_length() {
        // THE proof: the corner of this L sits at t = 0.5 but at 3/4 of the
        // arc length. Parameter-uniform samples would cluster; these must not.
        let samples = ell().sample_uniform(5).unwrap();
        assert_eq!(samples.len(), 5);
        let step = 4.0 / 4.0;
        for (i, s) in samples.iter().enumerate() {
            let want = i as f32 * step;
            let got = s.distance().get();
            assert!(
                close(got, want, 1.0e-3),
                "sample {i}: {got} should be {want}"
            );
        }
        let expected = [
            Vec3::ZERO,
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(3.0, 0.0, 0.0),
            Vec3::new(3.0, 1.0, 0.0),
        ];
        for (s, want) in samples.iter().zip(expected) {
            let got = s.position();
            assert!(
                vec_close(got, want, 1.0e-3),
                "expected {want:?}, got {got:?}"
            );
        }
        // Consecutive spacing is equal — the uniformity claim, stated directly.
        for pair in samples.windows(2) {
            let gap = pair[0].position().distance(pair[1].position());
            assert!(close(gap, 1.0, 1.0e-3), "consecutive gap was {gap}");
        }
    }

    #[test]
    fn sample_uniform_endpoints_pin_the_curve_and_its_length() {
        let c = ell();
        let samples = c.sample_uniform(9).unwrap();
        assert_eq!(samples.first().unwrap().distance().get(), 0.0);
        assert_eq!(samples.first().unwrap().parameter().get(), 0.0);
        assert!(vec_close(
            samples.first().unwrap().position(),
            Vec3::ZERO,
            1.0e-5
        ));
        let end = samples.last().unwrap();
        assert!(close(end.parameter().get(), 1.0, 1.0e-5));
        assert!(close(end.distance().get(), 4.0, 1.0e-3));
        assert!(vec_close(end.position(), Vec3::new(3.0, 1.0, 0.0), 1.0e-3));
    }

    #[test]
    fn sample_uniform_is_arc_uniform_on_a_curved_path() {
        let samples = arc_bezier().sample_uniform(17).unwrap();
        let gaps: Vec<f32> = samples
            .windows(2)
            .map(|p| p[0].position().distance(p[1].position()))
            .collect();
        let mean = gaps.iter().sum::<f32>() / gaps.len() as f32;
        for gap in &gaps {
            assert!(
                close(*gap, mean, mean * 0.02),
                "gap {gap} strays from the mean {mean}"
            );
        }
        // Parameters are NOT uniform — that is the whole point of inverting.
        let params: Vec<f32> = samples.iter().map(|s| s.parameter().get()).collect();
        assert!(
            !close(params[1], 1.0 / 16.0, 1.0e-4),
            "parameters must not be evenly spaced"
        );
    }

    #[test]
    fn sample_uniform_carries_unit_tangents_for_every_kind() {
        for c in [straight(), ell(), arc_bezier(), spline()] {
            for s in c.sample_uniform(6).unwrap() {
                assert!(close(s.tangent().length(), 1.0, 1.0e-5));
            }
        }
    }

    #[test]
    fn sample_uniform_is_deterministic() {
        assert_eq!(
            spline().sample_uniform(7).unwrap(),
            spline().sample_uniform(7).unwrap()
        );
    }

    #[test]
    fn sample_uniform_needs_at_least_two_samples() {
        for count in [0u32, 1] {
            let err = ell().sample_uniform(count).unwrap_err();
            assert_eq!(err.code(), MathErrorCode::InvalidCurve);
        }
        assert_eq!(ell().sample_uniform(2).unwrap().len(), 2);
    }

    #[test]
    fn sample_uniform_propagates_a_vanishing_tangent() {
        let c = Curve::cubic_bezier(vec![
            Vec3::ZERO,
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::ZERO,
            Vec3::new(1.0, 0.0, 0.0),
        ])
        .unwrap();
        assert_eq!(
            c.sample_uniform(3).unwrap_err().code(),
            MathErrorCode::NormalizeZeroLength
        );
    }

    #[test]
    fn sample_uniform_bounds_the_node_table_for_a_huge_count() {
        // `count - 1` times the density saturates well past the cap; the table
        // is clamped, so this stays a bounded allocation and still solves.
        let samples = straight().sample_uniform(2000).unwrap();
        assert_eq!(samples.len(), 2000);
        assert!(close(samples.last().unwrap().distance().get(), 2.0, 1.0e-3));
    }

    #[test]
    fn too_few_points_are_rejected_per_kind() {
        let one = vec![Vec3::ZERO];
        assert_eq!(
            Curve::polyline(one.clone()).unwrap_err().code(),
            MathErrorCode::InvalidCurve
        );
        assert_eq!(
            Curve::polyline(vec![]).unwrap_err().message(),
            "a polyline curve needs at least 2 points"
        );
        assert_eq!(
            Curve::catmull_rom(vec![Vec3::ZERO, Vec3::UNIT_X, Vec3::UNIT_Y])
                .unwrap_err()
                .message(),
            "a Catmull-Rom curve needs at least 4 points"
        );
    }

    #[test]
    fn bezier_rejects_a_point_count_that_is_not_three_n_plus_one() {
        for len in [1usize, 2, 3, 5, 6, 8] {
            let points: Vec<Vec3> = (0..len).map(|i| Vec3::new(i as f32, 0.0, 0.0)).collect();
            let err = Curve::cubic_bezier(points).unwrap_err();
            assert_eq!(err.code(), MathErrorCode::InvalidCurve);
            assert_eq!(
                err.message(),
                "a cubic Bezier curve needs 3n+1 points with n >= 1"
            );
        }
        assert!(
            Curve::cubic_bezier((0..7).map(|i| Vec3::new(i as f32, 0.0, 0.0)).collect()).is_ok()
        );
    }

    #[test]
    fn non_finite_points_are_rejected() {
        for bad in [
            Vec3::new(f32::NAN, 0.0, 0.0),
            Vec3::new(0.0, f32::INFINITY, 0.0),
            Vec3::new(0.0, 0.0, f32::NEG_INFINITY),
        ] {
            let err = Curve::polyline(vec![Vec3::ZERO, bad]).unwrap_err();
            assert_eq!(err.code(), MathErrorCode::InvalidCurve);
            assert_eq!(err.message(), "curve points must all be finite");
        }
    }

    #[test]
    fn coincident_consecutive_points_are_rejected() {
        let err = Curve::polyline(vec![Vec3::ZERO, Vec3::ZERO, Vec3::UNIT_X]).unwrap_err();
        assert_eq!(err.code(), MathErrorCode::InvalidCurve);
        assert_eq!(err.message(), "consecutive curve points must be distinct");
        // Non-consecutive repeats are fine: a closed loop is legal.
        assert!(Curve::polyline(vec![Vec3::ZERO, Vec3::UNIT_X, Vec3::UNIT_Y, Vec3::ZERO]).is_ok());
    }
}
