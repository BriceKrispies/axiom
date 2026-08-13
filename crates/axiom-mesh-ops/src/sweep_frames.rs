//! Rotation-minimising frames along a sampled curve.
//!
//! A sweep needs an orthonormal basis at every point of its path: a tangent to
//! advance along, and two cross-section axes to place the profile in. Getting
//! that basis right is the single hardest part of a sweep, and the classic wrong
//! answer is a *fixed-up Frenet-ish frame* — `binormal = up.cross(tangent)` for
//! some global `up`. That construction is undefined exactly where the tangent
//! becomes parallel to `up`, so a path that climbs through vertical does not
//! degrade gracefully: the cross-section snaps through a half-turn in one span
//! and the swept surface tears. It is the most common sweep bug there is.
//!
//! What this module builds instead is a **rotation-minimising frame** (also
//! called a parallel-transport or Bishop frame). Frame `0` is seeded once, and
//! every later frame is the previous one carried along by the *minimal* rotation
//! that takes `tangent[i - 1]` onto `tangent[i]` — a rotation about
//! `cross(t[i-1], t[i])` by the angle between them, applied with Rodrigues'
//! formula. Because that rotation is minimal, the frame never spins about its
//! own tangent: it accumulates no twist the path did not ask for, it is defined
//! for every tangent including a vertical one, and consecutive normals can never
//! flip sign. There is no global up-vector anywhere in this file, by design.
//!
//! ## Why the policy lives here and not in `axiom-math`
//!
//! `axiom_math::Curve` owns the *mathematics* of a curve — where it is, which
//! way it points, how long it is. Which orthonormal basis a swept cross-section
//! should ride in is a **geometry-construction policy**: the seeding rule, the
//! collinear-carry rule, and the re-orthogonalisation are choices this layer
//! makes so its operators agree with one another. Pushing them down into `math`
//! would give the curve primitive an opinion about meshing that nothing else in
//! `math` needs.

use axiom_math::{CurveSample, Vec3};
use axiom_mesh::{MeshError, MeshErrorCode, MeshResult};

/// How short a vector may be before it stops being a usable direction.
///
/// Used for two independent tests: whether the cross product of two consecutive
/// tangents is a real rotation axis (below this the tangents are collinear and
/// the frame is carried unchanged), and whether a candidate seed reference has a
/// meaningful component perpendicular to the first tangent.
const FRAME_EPSILON: f32 = 1.0e-6;

/// The deterministic fallback candidates for a seed reference, in the order they
/// are scored. Nothing about this order is meaningful except that it is fixed:
/// the point is that two runs on the same path produce the same frames.
const WORLD_AXES: [Vec3; 3] = [Vec3::UNIT_X, Vec3::UNIT_Y, Vec3::UNIT_Z];

/// One orthonormal station along a swept path.
///
/// `tangent`, `normal` and `binormal` are mutually perpendicular unit vectors
/// with `binormal == tangent.cross(normal)`, so `(normal, binormal, tangent)` is
/// a right-handed basis in which `tangent` plays the role of `+Z`. A profile
/// authored in the XY plane therefore maps into the frame as
/// `position + normal * x + binormal * y`, and its own `+Z` points the way the
/// sweep is travelling.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SweepFrame {
    position: Vec3,
    tangent: Vec3,
    normal: Vec3,
    binormal: Vec3,
}

impl SweepFrame {
    /// Where on the path this frame sits.
    pub const fn position(&self) -> Vec3 {
        self.position
    }

    /// The unit direction the path is travelling in.
    pub const fn tangent(&self) -> Vec3 {
        self.tangent
    }

    /// The cross-section axis a profile's local `+X` maps onto.
    pub const fn normal(&self) -> Vec3 {
        self.normal
    }

    /// The cross-section axis a profile's local `+Y` maps onto,
    /// `tangent.cross(normal)`.
    pub const fn binormal(&self) -> Vec3 {
        self.binormal
    }
}

/// Build a rotation-minimising frame at every sample of a path.
///
/// # Seeding
///
/// Frame `0`'s normal is `initial_reference` with its component along the first
/// tangent removed. When that leaves nothing usable — `initial_reference` is
/// zero, or parallel to the first tangent — the reference is replaced by the
/// **world axis least aligned with the first tangent**, scored over
/// `[+X, +Y, +Z]` in that order and taking the first minimum. A unit tangent
/// cannot be within 55 degrees of all three axes at once, so that fallback
/// always yields a healthy perpendicular, and it is a pure function of the
/// tangent, so it is deterministic and replayable.
///
/// A zero `initial_reference` is therefore the honest way to say *"no
/// preference, pick for me"*.
///
/// # Propagation
///
/// Each later normal is the previous one rotated by the minimal rotation
/// carrying `tangent[i - 1]` onto `tangent[i]`, then re-projected onto the plane
/// perpendicular to `tangent[i]` and renormalised so accumulated floating-point
/// drift cannot let the basis go skew over thousands of samples. Where the two
/// tangents are collinear (`cross` below [`FRAME_EPSILON`]) the rotation is
/// exactly the identity and the previous normal is carried through unchanged —
/// which is what makes a straight run, and a straight *vertical* run, produce a
/// constant cross-section instead of an undefined one.
///
/// # Errors
///
/// - [`MeshErrorCode::InvalidPath`] when fewer than two samples are supplied:
///   one station has no span to sweep across.
/// - [`MeshErrorCode::DegenerateAxis`] when `initial_reference` is not finite.
///   A `NaN` or infinite reference is a corrupted input, not a request for the
///   fallback: silently substituting a world axis would hide the caller's bug
///   and hand back frames nothing asked for. (Zero and parallel references *are*
///   well-formed requests for the fallback, and are not errors.)
pub fn parallel_transport_frames(
    samples: &[CurveSample],
    initial_reference: Vec3,
) -> MeshResult<Vec<SweepFrame>> {
    (samples.len() >= 2)
        .then_some(())
        .ok_or_else(|| {
            MeshError::new(
                MeshErrorCode::InvalidPath,
                "framing a path needs at least two samples",
            )
        })
        .and_then(|()| {
            finite(initial_reference).then_some(()).ok_or_else(|| {
                MeshError::new(
                    MeshErrorCode::DegenerateAxis,
                    "a sweep's initial reference must be finite; use Vec3::ZERO to request the deterministic fallback axis",
                )
            })
        })
        .map(|()| transport(samples, initial_reference))
}

/// Whether every component of `v` is finite.
fn finite(v: Vec3) -> bool {
    v.x.is_finite() & v.y.is_finite() & v.z.is_finite()
}

/// The seed cross-section axis for a path whose first tangent is `unit_tangent`.
///
/// Shared with the revolution operator, which needs exactly the same thing — a
/// deterministic unit vector perpendicular to a given axis — and would otherwise
/// re-derive the rule and risk disagreeing with the sweep about it.
pub(crate) fn seed_normal(unit_tangent: Vec3, reference: Vec3) -> Vec3 {
    let from_reference = orthogonalize(reference, unit_tangent);
    let usable = from_reference.length() > FRAME_EPSILON;
    let from_fallback = orthogonalize(least_aligned_axis(unit_tangent), unit_tangent);
    [from_fallback, from_reference][usize::from(usable)]
        .normalize()
        .unwrap_or(Vec3::UNIT_X)
}

/// `v` with its component along the unit vector `unit_axis` removed.
fn orthogonalize(v: Vec3, unit_axis: Vec3) -> Vec3 {
    v.subtract(unit_axis.mul_scalar(v.dot(unit_axis)))
}

/// The world axis with the smallest absolute dot product against `t`, first
/// minimum wins. Deterministic by construction.
fn least_aligned_axis(t: Vec3) -> Vec3 {
    let scores = [t.x.abs(), t.y.abs(), t.z.abs()];
    let best = (0..3).fold(0usize, |b, i| [b, i][usize::from(scores[i] < scores[b])]);
    WORLD_AXES[best]
}

/// Seed frame 0 and carry it along every span. Preconditions (`len >= 2`, finite
/// reference) are established by [`parallel_transport_frames`].
fn transport(samples: &[CurveSample], reference: Vec3) -> Vec<SweepFrame> {
    let tangents: Vec<Vec3> = samples.iter().map(CurveSample::tangent).collect();
    let seed = seed_normal(tangents[0], reference);
    let normals: Vec<Vec3> = core::iter::once(seed)
        .chain(tangents.windows(2).scan(seed, |carried, span| {
            *carried = advance(*carried, span[0], span[1]);
            Some(*carried)
        }))
        .collect();
    samples
        .iter()
        .zip(tangents.iter())
        .zip(normals.iter())
        .map(|((sample, tangent), normal)| SweepFrame {
            position: sample.position(),
            tangent: *tangent,
            normal: *normal,
            binormal: tangent.cross(*normal),
        })
        .collect()
}

/// Carry `previous_normal` across one span by the minimal rotation from
/// `from_tangent` to `to_tangent`.
fn advance(previous_normal: Vec3, from_tangent: Vec3, to_tangent: Vec3) -> Vec3 {
    let axis = from_tangent.cross(to_tangent);
    let sine = axis.length();
    let turning = sine > FRAME_EPSILON;
    // A collinear span contributes the identity rotation: a zero axis and a zero
    // angle, so Rodrigues' formula returns the previous normal untouched. The
    // divisor is forced to 1 there so the normalisation is never a 0/0.
    let gate = f32::from(u8::from(turning));
    let unit_axis = axis.mul_scalar(gate / [1.0, sine][usize::from(turning)]);
    let angle = sine.atan2(from_tangent.dot(to_tangent)) * gate;
    let rotated = rodrigues(previous_normal, unit_axis, angle);
    orthogonalize(rotated, to_tangent)
        .normalize()
        .unwrap_or(previous_normal)
}

/// Rotate `v` about the unit axis `k` by `angle`.
fn rodrigues(v: Vec3, k: Vec3, angle: f32) -> Vec3 {
    let (sine, cosine) = (angle.sin(), angle.cos());
    v.mul_scalar(cosine)
        .add(k.cross(v).mul_scalar(sine))
        .add(k.mul_scalar(k.dot(v) * (1.0 - cosine)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_math::Curve;

    fn framed(points: Vec<Vec3>, count: u32, reference: Vec3) -> Vec<SweepFrame> {
        let curve = Curve::polyline(points).unwrap();
        let samples = curve.sample_uniform(count).unwrap();
        parallel_transport_frames(&samples, reference).unwrap()
    }

    fn orthonormal(frame: &SweepFrame) -> bool {
        let (t, n, b) = (frame.tangent(), frame.normal(), frame.binormal());
        (t.length() - 1.0).abs() < 1.0e-4
            && (n.length() - 1.0).abs() < 1.0e-4
            && (b.length() - 1.0).abs() < 1.0e-4
            && t.dot(n).abs() < 1.0e-4
            && t.dot(b).abs() < 1.0e-4
            && n.dot(b).abs() < 1.0e-4
            && t.cross(n).subtract(b).length() < 1.0e-4
    }

    #[test]
    fn fewer_than_two_samples_is_an_invalid_path() {
        assert_eq!(
            parallel_transport_frames(&[], Vec3::UNIT_Y)
                .unwrap_err()
                .code(),
            MeshErrorCode::InvalidPath
        );
    }

    #[test]
    fn a_single_sample_is_also_an_invalid_path() {
        let curve = Curve::polyline(vec![Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0)]).unwrap();
        let samples = curve.sample_uniform(2).unwrap();
        assert_eq!(
            parallel_transport_frames(&samples[..1], Vec3::UNIT_Y)
                .unwrap_err()
                .code(),
            MeshErrorCode::InvalidPath
        );
    }

    #[test]
    fn a_non_finite_reference_is_a_degenerate_axis() {
        let curve = Curve::polyline(vec![Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0)]).unwrap();
        let samples = curve.sample_uniform(4).unwrap();
        assert_eq!(
            parallel_transport_frames(&samples, Vec3::new(f32::NAN, 0.0, 0.0))
                .unwrap_err()
                .code(),
            MeshErrorCode::DegenerateAxis
        );
        assert_eq!(
            parallel_transport_frames(&samples, Vec3::new(0.0, f32::INFINITY, 0.0))
                .unwrap_err()
                .code(),
            MeshErrorCode::DegenerateAxis
        );
        assert_eq!(
            parallel_transport_frames(&samples, Vec3::new(0.0, 0.0, f32::NEG_INFINITY))
                .unwrap_err()
                .code(),
            MeshErrorCode::DegenerateAxis
        );
    }

    #[test]
    fn a_straight_path_keeps_one_constant_frame() {
        let frames = framed(
            vec![Vec3::ZERO, Vec3::new(4.0, 0.0, 0.0)],
            5,
            Vec3::UNIT_Y,
        );
        assert_eq!(frames.len(), 5);
        for f in &frames {
            assert!(orthonormal(f));
            assert!(f.tangent().subtract(Vec3::UNIT_X).length() < 1.0e-5);
            assert!(f.normal().subtract(Vec3::UNIT_Y).length() < 1.0e-5);
            assert!(f.binormal().subtract(Vec3::UNIT_Z).length() < 1.0e-5);
        }
        // Positions are the sample positions, evenly spread over 4 metres.
        assert!(frames[0].position().length() < 1.0e-5);
        assert!(frames[4].position().subtract(Vec3::new(4.0, 0.0, 0.0)).length() < 1.0e-4);
    }

    #[test]
    fn a_reference_parallel_to_the_tangent_falls_back_to_a_world_axis() {
        // +X path with a +X reference: the reference is entirely useless, so the
        // least-aligned world axis (+Y, the first minimum) seeds the frame.
        let frames = framed(
            vec![Vec3::ZERO, Vec3::new(3.0, 0.0, 0.0)],
            3,
            Vec3::UNIT_X,
        );
        for f in &frames {
            assert!(orthonormal(f));
        }
        assert!(frames[0].normal().subtract(Vec3::UNIT_Y).length() < 1.0e-5);
    }

    #[test]
    fn a_zero_reference_requests_the_fallback_and_is_not_an_error() {
        let frames = framed(vec![Vec3::ZERO, Vec3::new(0.0, 0.0, 5.0)], 3, Vec3::ZERO);
        for f in &frames {
            assert!(orthonormal(f));
            assert!(f.tangent().subtract(Vec3::UNIT_Z).length() < 1.0e-5);
        }
        // Least-aligned axis of +Z is +X (first minimum of |dot| = [0, 0, 1]).
        assert!(frames[0].normal().subtract(Vec3::UNIT_X).length() < 1.0e-5);
    }

    #[test]
    fn a_vertical_path_frames_without_a_flip() {
        // The pathological case for a fixed-up frame: the tangent is exactly the
        // conventional up-vector, and the caller even asks for +Y as reference.
        let frames = framed(vec![Vec3::ZERO, Vec3::new(0.0, 6.0, 0.0)], 8, Vec3::UNIT_Y);
        for f in &frames {
            assert!(orthonormal(f));
            assert!(f.tangent().subtract(Vec3::UNIT_Y).length() < 1.0e-5);
        }
        for pair in frames.windows(2) {
            assert!(pair[0].normal().dot(pair[1].normal()) > 0.0);
        }
    }

    #[test]
    fn a_path_climbing_through_vertical_never_flips_its_normal() {
        // Horizontal, then straight up, then horizontal again in a new
        // direction: a fixed-up construction is undefined on the middle leg and
        // snaps through a half turn entering and leaving it.
        let curve = Curve::catmull_rom(vec![
            Vec3::new(-5.0, -1.0, 0.0),
            Vec3::new(-4.0, 0.0, 0.0),
            Vec3::ZERO,
            Vec3::new(0.0, 4.0, 0.0),
            Vec3::new(0.0, 4.0, 4.0),
            Vec3::new(0.0, 4.0, 5.0),
        ])
        .unwrap();
        let samples = curve.sample_uniform(60).unwrap();
        let frames = parallel_transport_frames(&samples, Vec3::UNIT_Y).unwrap();
        // The path really does pass through vertical.
        assert!(frames.iter().any(|f| f.tangent().y > 0.99));
        for f in &frames {
            assert!(orthonormal(f));
        }
        for (i, pair) in frames.windows(2).enumerate() {
            let coherence = pair[0].normal().dot(pair[1].normal());
            assert!(coherence > 0.0, "frame normal flipped after {i}: dot = {coherence}");
        }
    }

    #[test]
    fn a_right_angled_corner_turns_the_normal_by_exactly_the_tangents_turn() {
        // The sharp-corner limit: parallel transport rotates the cross-section
        // by the same rotation the tangent underwent and no more. At a 90-degree
        // corner that is a 90-degree turn — not the 180-degree snap a fixed-up
        // frame would suffer, and not the identity either.
        let frames = framed(
            vec![
                Vec3::new(-4.0, 0.0, 0.0),
                Vec3::ZERO,
                Vec3::new(0.0, 4.0, 0.0),
                Vec3::new(0.0, 4.0, 4.0),
            ],
            40,
            Vec3::UNIT_Y,
        );
        for f in &frames {
            assert!(orthonormal(f));
        }
        for (i, pair) in frames.windows(2).enumerate() {
            let turned = pair[0].normal().dot(pair[1].normal());
            let tangents = pair[0].tangent().dot(pair[1].tangent());
            assert!(
                turned >= tangents - 1.0e-4,
                "normal at {i} turned further than the tangent did: {turned} vs {tangents}"
            );
        }
        // Entering the climb the tangent goes +X -> +Y about +Z, so the seeded
        // +Y normal is carried to -X: a quarter turn, not an inversion.
        let (first, last) = (frames[0].normal(), frames[frames.len() - 1].normal());
        assert!(first.subtract(Vec3::UNIT_Y).length() < 1.0e-5);
        assert!(last.subtract(Vec3::new(-1.0, 0.0, 0.0)).length() < 1.0e-4);
    }

    #[test]
    fn an_inflection_does_not_disturb_the_frame() {
        // An S: the curvature changes sign in the middle. A rotation-minimising
        // frame must roll through it continuously.
        let curve = Curve::catmull_rom(vec![
            Vec3::new(-1.0, -1.0, 0.0),
            Vec3::ZERO,
            Vec3::new(2.0, 2.0, 0.0),
            Vec3::new(4.0, 0.0, 0.0),
            Vec3::new(6.0, -2.0, 0.0),
            Vec3::new(7.0, -3.0, 0.0),
        ])
        .unwrap();
        let samples = curve.sample_uniform(60).unwrap();
        let frames = parallel_transport_frames(&samples, Vec3::UNIT_Z).unwrap();
        for f in &frames {
            assert!(orthonormal(f));
        }
        for pair in frames.windows(2) {
            assert!(pair[0].normal().dot(pair[1].normal()) > 0.9);
        }
        // The path lies in the XY plane, so a +Z reference stays exactly +Z: the
        // frame accrues no twist the path did not ask for.
        for f in &frames {
            assert!(f.normal().subtract(Vec3::UNIT_Z).length() < 1.0e-3);
        }
    }

    #[test]
    fn a_helix_accrues_no_spurious_twist() {
        // A curve turning in all three axes. Parallel transport must keep the
        // normal perpendicular to the tangent the whole way round.
        let points: Vec<Vec3> = (0..24)
            .map(|i| {
                let a = i as f32 * 0.4;
                Vec3::new(a.cos() * 2.0, i as f32 * 0.3, a.sin() * 2.0)
            })
            .collect();
        let curve = Curve::polyline(points).unwrap();
        let samples = curve.sample_uniform(120).unwrap();
        let frames = parallel_transport_frames(&samples, Vec3::UNIT_Y).unwrap();
        for f in &frames {
            assert!(orthonormal(f));
        }
        for pair in frames.windows(2) {
            assert!(pair[0].normal().dot(pair[1].normal()) > 0.0);
        }
    }

    #[test]
    fn frames_are_reproducible() {
        let a = framed(
            vec![Vec3::ZERO, Vec3::new(1.0, 2.0, 3.0), Vec3::new(4.0, 0.0, 1.0)],
            17,
            Vec3::UNIT_Y,
        );
        let b = framed(
            vec![Vec3::ZERO, Vec3::new(1.0, 2.0, 3.0), Vec3::new(4.0, 0.0, 1.0)],
            17,
            Vec3::UNIT_Y,
        );
        assert_eq!(a, b);
    }

    #[test]
    fn the_least_aligned_axis_prefers_the_first_minimum() {
        assert_eq!(least_aligned_axis(Vec3::UNIT_X), Vec3::UNIT_Y);
        assert_eq!(least_aligned_axis(Vec3::UNIT_Y), Vec3::UNIT_X);
        assert_eq!(least_aligned_axis(Vec3::UNIT_Z), Vec3::UNIT_X);
        assert_eq!(
            least_aligned_axis(Vec3::new(0.6, 0.8, 0.0).normalize().unwrap()),
            Vec3::UNIT_Z
        );
    }

    #[test]
    fn a_reversing_span_carries_the_normal_rather_than_negating_it() {
        // An exactly-collinear-but-opposed pair of tangents has a zero cross
        // product; the carry rule keeps the previous normal instead of letting a
        // 180-degree rotation invert it.
        let carried = advance(Vec3::UNIT_Y, Vec3::UNIT_X, Vec3::UNIT_X.mul_scalar(-1.0));
        assert!(carried.subtract(Vec3::UNIT_Y).length() < 1.0e-6);
    }
}
