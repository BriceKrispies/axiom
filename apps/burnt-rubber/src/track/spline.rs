//! Catmull-Rom interpolation of the control points, and the arc-length
//! resampling that turns a parametric curve into the app's single spatial index.
//!
//! Why arc length matters here: everything downstream addresses the course by
//! **metres travelled** — the car's progress coordinate, traffic spawn spacing,
//! chunk boundaries, lane dashes, reflector-post spacing, the reset points, the
//! HUD's progress bar. If the sample table were parameter-uniform instead of
//! arc-length-uniform, every one of those would stretch and squash with the
//! local curve speed, dashes would visibly bunch in corners, and "the car is
//! 4 200 m along" would not be a distance. So the curve is walked once into a
//! dense polyline, and samples are laid down at an exact fixed spacing along it.
//!
//! Catmull-Rom is the right spline for this: it passes *through* its control
//! points (so the constrained heading walk in [`super::generate`] is the road,
//! not a suggestion), and it is C¹ continuous, so tangents match across every
//! segment boundary — which is what makes the chunked road mesh watertight.

use axiom_math::Vec3;

/// A unit vector, falling back to `fallback` for a degenerate input. The math
/// layer's `normalize` is fallible; the road builder is not allowed to fail
/// halfway through, so a degenerate tangent resolves to the previous direction
/// instead of poisoning the table with a `NaN`.
pub fn unit_or(v: Vec3, fallback: Vec3) -> Vec3 {
    v.normalize().unwrap_or(fallback)
}

/// The standard uniform Catmull-Rom basis evaluated at `t` in `[0, 1]` between
/// `p1` and `p2`, with `p0`/`p3` as the neighbouring control points.
pub fn catmull_rom(p0: Vec3, p1: Vec3, p2: Vec3, p3: Vec3, t: f32) -> Vec3 {
    let t2 = t * t;
    let t3 = t2 * t;
    let a = p1.mul_scalar(2.0);
    let b = p2.subtract(p0).mul_scalar(t);
    let c = p0
        .mul_scalar(2.0)
        .subtract(p1.mul_scalar(5.0))
        .add(p2.mul_scalar(4.0))
        .subtract(p3)
        .mul_scalar(t2);
    let d = p1
        .mul_scalar(3.0)
        .subtract(p0)
        .subtract(p2.mul_scalar(3.0))
        .add(p3)
        .mul_scalar(t3);
    a.add(b).add(c).add(d).mul_scalar(0.5)
}

/// Substeps evaluated per control-point segment when building the dense
/// polyline. At the shipping 40 m control spacing this is a point every 1 m —
/// well below the 2 m sample spacing, so the resampling never has to
/// extrapolate through a corner.
const SUBSTEPS: usize = 40;

/// One point of the dense polyline, carrying the fractional control-point index
/// it came from so per-control attributes (width, section) can be interpolated
/// onto the final samples.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DensePoint {
    pub position: Vec3,
    /// Distance along the polyline from the start (m).
    pub distance: f32,
    /// Position in control-point space, e.g. `12.5` is halfway between control
    /// points 12 and 13.
    pub control_t: f32,
}

/// Walk the control points into a dense, distance-stamped polyline.
///
/// Fewer than two control points yields an empty polyline — a course that short
/// is not a course, and the caller ([`super::Track::generate`]) treats it as
/// such rather than fabricating geometry.
pub fn densify(points: &[Vec3]) -> Vec<DensePoint> {
    if points.len() < 2 {
        return Vec::new();
    }
    let clamped = |i: isize| points[i.clamp(0, points.len() as isize - 1) as usize];
    let mut dense = Vec::with_capacity((points.len() - 1) * SUBSTEPS + 1);
    let mut distance = 0.0f32;
    let mut previous = points[0];
    dense.push(DensePoint {
        position: previous,
        distance: 0.0,
        control_t: 0.0,
    });
    for i in 0..points.len() - 1 {
        let p0 = clamped(i as isize - 1);
        let p1 = points[i];
        let p2 = points[i + 1];
        let p3 = clamped(i as isize + 2);
        for s in 1..=SUBSTEPS {
            let t = s as f32 / SUBSTEPS as f32;
            let position = catmull_rom(p0, p1, p2, p3, t);
            distance += position.distance(previous);
            previous = position;
            dense.push(DensePoint {
                position,
                distance,
                control_t: i as f32 + t,
            });
        }
    }
    dense
}

/// Lay samples down along `dense` at exactly `spacing` metres apart.
///
/// The walk is a single forward pass with a cursor that only ever advances, so
/// it is `O(n)` and cannot loop: each emitted sample either consumes polyline or
/// ends the walk.
pub fn resample(dense: &[DensePoint], spacing: f32) -> Vec<DensePoint> {
    if dense.len() < 2 || spacing <= 0.0 {
        return Vec::new();
    }
    let total = dense[dense.len() - 1].distance;
    let count = (total / spacing).floor() as usize + 1;
    let mut out = Vec::with_capacity(count);
    let mut cursor = 0usize;
    for i in 0..count {
        let target = i as f32 * spacing;
        while cursor + 2 < dense.len() && dense[cursor + 1].distance < target {
            cursor += 1;
        }
        let a = dense[cursor];
        let b = dense[cursor + 1];
        let span = b.distance - a.distance;
        let t = ((target - a.distance) / span.max(1.0e-6)).clamp(0.0, 1.0);
        out.push(DensePoint {
            position: a.position.add(b.position.subtract(a.position).mul_scalar(t)),
            distance: target,
            control_t: a.control_t + (b.control_t - a.control_t) * t,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn straight(n: usize, step: f32) -> Vec<Vec3> {
        (0..n).map(|i| Vec3::new(0.0, 0.0, i as f32 * step)).collect()
    }

    #[test]
    fn the_curve_passes_through_its_control_points() {
        let p = [
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 4.0),
            Vec3::new(6.0, 2.0, 7.0),
            Vec3::new(9.0, 1.0, 12.0),
        ];
        let at_zero = catmull_rom(p[0], p[1], p[2], p[3], 0.0);
        let at_one = catmull_rom(p[0], p[1], p[2], p[3], 1.0);
        assert!(at_zero.distance(p[1]) < 1.0e-5, "t=0 is p1");
        assert!(at_one.distance(p[2]) < 1.0e-5, "t=1 is p2");
    }

    /// C¹ continuity across a segment boundary is what makes the chunked road
    /// mesh watertight, so it is asserted numerically rather than assumed.
    #[test]
    fn segment_boundaries_are_position_and_tangent_continuous() {
        let points: Vec<Vec3> = (0..8)
            .map(|i| {
                let f = i as f32;
                Vec3::new((f * 0.7).sin() * 12.0, (f * 0.4).cos() * 3.0, f * 20.0)
            })
            .collect();
        let h = 1.0e-3;
        // Segment `i` spans points[i]..points[i+1]; segment `i+1` spans
        // points[i+1]..points[i+2] and needs points[i+3], hence the upper bound.
        for i in 1..points.len() - 3 {
            let end = catmull_rom(points[i - 1], points[i], points[i + 1], points[i + 2], 1.0);
            let start = catmull_rom(points[i], points[i + 1], points[i + 2], points[i + 3], 0.0);
            assert!(end.distance(start) < 1.0e-4, "position is continuous at {i}");

            let before = catmull_rom(points[i - 1], points[i], points[i + 1], points[i + 2], 1.0 - h);
            let tangent_in = end.subtract(before).mul_scalar(1.0 / h);
            let after = catmull_rom(points[i], points[i + 1], points[i + 2], points[i + 3], h);
            let tangent_out = after.subtract(start).mul_scalar(1.0 / h);
            let a = unit_or(tangent_in, Vec3::UNIT_Z);
            let b = unit_or(tangent_out, Vec3::UNIT_Z);
            assert!(
                a.dot(b) > 0.999,
                "tangent is continuous at {i}: {a:?} vs {b:?}"
            );
        }
    }

    #[test]
    fn densifying_stamps_monotonically_increasing_distance() {
        let dense = densify(&straight(6, 40.0));
        assert!(dense.len() > 200);
        for w in dense.windows(2) {
            assert!(w[1].distance >= w[0].distance, "distance never goes backwards");
            assert!(w[1].control_t >= w[0].control_t);
        }
        let total = dense[dense.len() - 1].distance;
        assert!((total - 200.0).abs() < 0.5, "a straight is its own length: {total}");
    }

    #[test]
    fn resampling_lands_exactly_on_the_requested_spacing() {
        let dense = densify(&straight(6, 40.0));
        let samples = resample(&dense, 2.0);
        assert!(samples.len() >= 100);
        for (i, s) in samples.iter().enumerate() {
            assert!(
                (s.distance - i as f32 * 2.0).abs() < 1.0e-3,
                "sample {i} is at {}",
                s.distance
            );
        }
        // And the positions track the straight line they came from.
        for s in &samples {
            assert!(s.position.x.abs() < 1.0e-3);
            assert!((s.position.z - s.distance).abs() < 0.05);
        }
    }

    /// A curve's arc length is longer than the chord through its control points,
    /// so resampling a curve must produce more samples than the chord would.
    #[test]
    fn resampling_a_curve_follows_the_arc_not_the_chord() {
        let points: Vec<Vec3> = (0..10)
            .map(|i| {
                let a = i as f32 * 0.25;
                Vec3::new(a.sin() * 100.0, 0.0, a.cos() * 100.0)
            })
            .collect();
        let dense = densify(&points);
        let arc = dense[dense.len() - 1].distance;
        let chord: f32 = points.windows(2).map(|w| w[0].distance(w[1])).sum();
        assert!(arc >= chord * 0.98, "arc {arc} tracks the polyline {chord}");
        let samples = resample(&dense, 2.0);
        for w in samples.windows(2) {
            let step = w[0].position.distance(w[1].position);
            assert!(
                (1.5..=2.05).contains(&step),
                "consecutive samples are ~2 m apart, got {step}"
            );
        }
    }

    #[test]
    fn degenerate_inputs_yield_nothing_rather_than_garbage() {
        assert!(densify(&[]).is_empty());
        assert!(densify(&[Vec3::ZERO]).is_empty());
        assert!(resample(&[], 2.0).is_empty());
        let dense = densify(&straight(4, 10.0));
        assert!(resample(&dense, 0.0).is_empty());
        assert!(resample(&dense, -1.0).is_empty());
    }

    #[test]
    fn unit_or_falls_back_instead_of_producing_a_nan() {
        assert_eq!(unit_or(Vec3::ZERO, Vec3::UNIT_Z), Vec3::UNIT_Z);
        let u = unit_or(Vec3::new(0.0, 0.0, 4.0), Vec3::UNIT_X);
        assert!((u.length() - 1.0).abs() < 1.0e-6);
        assert!(u.z > 0.99);
    }
}
