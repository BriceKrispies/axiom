//! Two-bone analytic inverse kinematics, and the distance-driven stride cycle
//! that feeds it.
//!
//! Both halves here are **pure functions of their arguments** — no state, no
//! clock, no accumulator. That is deliberate and it is what makes the whole
//! animation replayable: given a tick you can compute the pose, and given the
//! same tick you get the same pose, in this process or the next one.
//!
//! ## The solver
//!
//! Two bones, a root (hip/shoulder) and a target (ankle/wrist), is the one
//! articulation problem with a closed-form answer, so it gets one: the law of
//! cosines, not an iterative solver.
//!
//! With `d` the root→target distance and `a`, `b` the bone lengths, the angle
//! between the root→target line and the first bone is
//!
//! ```text
//! cos A = (a² + d² − b²) / (2 a d)
//! ```
//!
//! and the joint sits at `root + dir·(a cos A) + bend·(a sin A)`, where `dir` is
//! the unit root→target direction and `bend` is the unit component of the
//! **pole** perpendicular to it. Because `dir` and `bend` are orthonormal, that
//! places the joint at *exactly* `a` from the root, and the law of cosines then
//! puts it at exactly `b` from the target — the bones never stretch.
//!
//! ### Why it cannot produce a NaN
//!
//! Three separate guards, each closing a distinct hole:
//!
//! 1. `d` is **clamped** into `[|a−b| + ε, (a+b) − ε]` before anything divides
//!    by it. Beyond `a+b` the triangle does not close and `acos` would be
//!    handed a value outside `±1`; below `|a−b|` the chain has folded through
//!    itself. The `ε` also keeps `2 a d` away from zero.
//! 2. `cos A` is **clamped** into `±1` anyway, so floating-point drift at a
//!    fully-extended pose cannot walk `acos` off its domain.
//! 3. Every `normalize` has a **fallback**: a degenerate direction or a pole
//!    parallel to it yields a chosen perpendicular rather than a divide by
//!    zero.
//!
//! When the target was out of reach the solver reports the *reachable* end
//! point it actually hit, so the caller draws the foot where the leg really
//! ends instead of where it wished the leg ended.
//!
//! ### Why the pole is not optional
//!
//! A two-bone chain has a whole *circle* of valid joint positions — the elbow
//! can be anywhere on a cone about the root→target axis. Nothing in the lengths
//! chooses between them. Handing the solver an explicit pole picks one, and
//! picking it from the creature's own facing (forward for a knee, backward for
//! an elbow) is what keeps a leg from popping inside-out between two frames
//! whose targets differ by a millimetre.
//!
//! ## The stride cycle
//!
//! [`stride_phase`] is driven by **distance travelled**, never by elapsed time.
//! A foot's place in its cycle is `distance / stride + offset`; the integer part
//! names *which* step this is and the fraction says how far through it we are.
//! Naming the step is the whole trick: a planted foot's world position is a
//! function of the step number alone, so it is *identically* constant for the
//! entire stance no matter how the speed varies, and the feet cannot skate.

use axiom_math::{Transform, Vec3};

use crate::creature_rig::aim;

/// The clearance held back from full extension and from the folded limit, in
/// the same units as the bone lengths.
const REACH_EPSILON: f32 = 1.0e-3;

/// A solved two-bone chain, in the space its arguments were given in.
#[derive(Debug, Clone, Copy)]
pub struct TwoBone {
    /// The upper bone, pivoting at the root and pointing at the joint.
    pub upper: Transform,
    /// The lower bone, pivoting at the joint and pointing at [`TwoBone::end`].
    pub lower: Transform,
    /// The knee/elbow position.
    pub joint: Vec3,
    /// Where the chain actually ends — the requested target, or the nearest
    /// reachable point on the root→target ray when the target was out of reach.
    pub end: Vec3,
    /// Whether the target had to be pulled into reach.
    pub clamped: bool,
}

/// Solve a two-bone chain from `root` to `target`, bending toward `pole`.
///
/// `len_a` is the root→joint bone, `len_b` the joint→end bone. The returned
/// transforms carry unit scale and are oriented for a bone authored along local
/// `-Z` (see [`crate::creature_rig`]); a caller drawing a scaled creature
/// overwrites the scale itself.
pub fn solve_two_bone(root: Vec3, target: Vec3, pole: Vec3, len_a: f32, len_b: f32) -> TwoBone {
    let a = len_a.abs().max(REACH_EPSILON);
    let b = len_b.abs().max(REACH_EPSILON);
    let far = (a + b - REACH_EPSILON).max(REACH_EPSILON);
    let near = ((a - b).abs() + REACH_EPSILON).min(far);

    let to_target = target.subtract(root);
    let raw = to_target.length();
    let clamped = raw > far || raw < near;
    let distance = raw.clamp(near, far);
    // A degenerate root→target vector still has to yield *a* direction; the
    // pole is the only other information in the problem, so lean on it.
    let direction = to_target
        .normalize()
        .or_else(|_| pole.normalize())
        .unwrap_or(Vec3::UNIT_Z);

    // The bend axis: the part of the pole perpendicular to the chain. This is
    // the one degree of freedom the lengths do not determine.
    let along = direction.mul_scalar(pole.dot(direction));
    let bend = pole
        .subtract(along)
        .normalize()
        .unwrap_or_else(|_| perpendicular_to(direction));

    let cosine = ((a * a + distance * distance - b * b) / (2.0 * a * distance)).clamp(-1.0, 1.0);
    let angle = cosine.acos();
    let joint = root
        .add(direction.mul_scalar(a * angle.cos()))
        .add(bend.mul_scalar(a * angle.sin()));
    let end = root.add(direction.mul_scalar(distance));

    let upper_dir = joint.subtract(root).normalize().unwrap_or(direction);
    let lower_dir = end.subtract(joint).normalize().unwrap_or(direction);
    TwoBone {
        upper: Transform::new(root, aim(upper_dir, bend), Vec3::ONE),
        lower: Transform::new(joint, aim(lower_dir, bend), Vec3::ONE),
        joint,
        end,
        clamped,
    }
}

/// Some unit vector orthogonal to `direction`, for the degenerate-pole case.
fn perpendicular_to(direction: Vec3) -> Vec3 {
    let axis = [Vec3::UNIT_Y, Vec3::UNIT_X][usize::from(direction.y.abs() > 0.9)];
    direction
        .cross(axis)
        .normalize()
        .unwrap_or(Vec3::UNIT_X)
        .cross(direction)
        .normalize()
        .unwrap_or(Vec3::UNIT_Y)
}

/// Where one foot is in its step, derived from **distance travelled**.
#[derive(Debug, Clone, Copy)]
pub struct StridePhase {
    /// Which step this is. A planted foot's world position depends on this and
    /// nothing else, which is what makes the plant exact.
    pub step: f32,
    /// Progress through the whole step, `0..1`.
    pub fraction: f32,
    /// Whether the foot is on the ground this instant.
    pub planted: bool,
    /// Progress through the *swing*, `0..1`. Zero for the whole stance.
    pub swing: f32,
}

/// Split `distance` into a step number and a phase within that step.
///
/// `offset` is the leg's share of the cycle — `0.5` puts a leg exactly opposite
/// its partner. `duty` is the fraction of the step the foot spends planted; a
/// trot is around `0.55`, a run less.
pub fn stride_phase(distance: f32, stride: f32, offset: f32, duty: f32) -> StridePhase {
    let stride = stride.abs().max(REACH_EPSILON);
    let duty = duty.clamp(0.05, 0.95);
    let u = distance / stride + offset;
    let step = u.floor();
    let fraction = u - step;
    let planted = fraction < duty;
    StridePhase {
        step,
        fraction,
        planted,
        swing: ((fraction - duty) / (1.0 - duty)).clamp(0.0, 1.0),
    }
}

/// Smooth Hermite ease over `0..1` — the swing's fore/aft travel, so a foot
/// leaves and lands with zero horizontal speed instead of jerking.
pub fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The swing's vertical arc: zero at lift-off and at touchdown, `height` at the
/// midpoint.
pub fn swing_lift(swing: f32, height: f32) -> f32 {
    (swing.clamp(0.0, 1.0) * core::f32::consts::PI).sin() * height
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lengths(solved: &TwoBone, root: Vec3) -> (f32, f32) {
        (
            solved.joint.subtract(root).length(),
            solved.end.subtract(solved.joint).length(),
        )
    }

    #[test]
    fn a_reachable_target_is_reached_with_exact_bone_lengths() {
        let root = Vec3::new(0.0, 4.0, 0.0);
        let target = Vec3::new(0.0, 0.4, 0.0);
        let solved = solve_two_bone(root, target, Vec3::new(0.0, 0.0, -1.0), 2.0, 2.0);
        assert!(!solved.clamped);
        assert!(solved.end.distance(target) < 1.0e-3, "end {:?}", solved.end);
        let (a, b) = lengths(&solved, root);
        assert!((a - 2.0).abs() < 1.0e-3, "upper bone is {a}");
        assert!((b - 2.0).abs() < 1.0e-3, "lower bone is {b}");
    }

    #[test]
    fn an_unreachable_target_clamps_without_a_nan() {
        let root = Vec3::new(0.0, 10.0, 0.0);
        let target = Vec3::new(0.0, 0.0, 0.0);
        let solved = solve_two_bone(root, target, Vec3::new(0.0, 0.0, -1.0), 2.0, 2.0);
        assert!(solved.clamped);
        let (a, b) = lengths(&solved, root);
        assert!((a - 2.0).abs() < 1.0e-3, "upper bone stretched to {a}");
        assert!((b - 2.0).abs() < 1.0e-3, "lower bone stretched to {b}");
        for v in [solved.joint, solved.end] {
            assert!(v.x.is_finite() && v.y.is_finite() && v.z.is_finite(), "{v:?}");
        }
        // The reachable end sits on the root→target ray, just inside full reach.
        assert!((solved.end.distance(root) - 4.0).abs() < 1.0e-2);
    }

    #[test]
    fn a_coincident_target_clamps_instead_of_dividing_by_zero() {
        let root = Vec3::new(1.0, 1.0, 1.0);
        let solved = solve_two_bone(root, root, Vec3::UNIT_Z, 2.0, 1.0);
        assert!(solved.clamped);
        assert!(solved.joint.x.is_finite() && solved.joint.y.is_finite());
        let (a, b) = lengths(&solved, root);
        assert!((a - 2.0).abs() < 1.0e-2 && (b - 1.0).abs() < 1.0e-2);
    }

    #[test]
    fn the_joint_bends_toward_the_pole_and_follows_it_when_it_flips() {
        let root = Vec3::new(0.0, 4.0, 0.0);
        let target = Vec3::new(0.0, 1.0, 0.0);
        let forward = solve_two_bone(root, target, Vec3::new(0.0, 0.0, -1.0), 2.0, 2.0);
        let backward = solve_two_bone(root, target, Vec3::new(0.0, 0.0, 1.0), 2.0, 2.0);
        assert!(forward.joint.z < -0.5, "knee did not lead forward: {:?}", forward.joint);
        assert!(backward.joint.z > 0.5, "elbow did not lead back: {:?}", backward.joint);
        // A pole parallel to the chain still yields a finite, exact solution.
        let degenerate = solve_two_bone(root, target, Vec3::UNIT_Y, 2.0, 2.0);
        let (a, b) = lengths(&degenerate, root);
        assert!((a - 2.0).abs() < 1.0e-3 && (b - 2.0).abs() < 1.0e-3);
    }

    #[test]
    fn the_stride_names_the_step_and_splits_stance_from_swing() {
        // Half a stride in, with a 0.6 duty: still planted, no swing yet.
        let mid = stride_phase(5.0, 10.0, 0.0, 0.6);
        assert_eq!(mid.step, 0.0);
        assert!(mid.planted && mid.swing == 0.0);
        // Four fifths in: swinging, and the step number has not advanced.
        let late = stride_phase(8.0, 10.0, 0.0, 0.6);
        assert_eq!(late.step, 0.0);
        assert!(!late.planted && late.swing > 0.4 && late.swing < 0.6);
        // Past the stride: the next step.
        assert_eq!(stride_phase(11.0, 10.0, 0.0, 0.6).step, 1.0);
        // The offset is exactly half a cycle out. At a duty under 0.5 the two
        // halves cannot overlap, so one foot is down exactly when the other is
        // not. (Above 0.5 they legitimately share ground — that is what a
        // double-support gait is — so the assertion is made at a running duty.)
        let a = stride_phase(0.0, 10.0, 0.0, 0.45);
        let b = stride_phase(0.0, 10.0, 0.5, 0.45);
        assert!(a.planted && !b.planted);
    }

    #[test]
    fn the_swing_curves_ease_and_arc() {
        assert_eq!(ease(0.0), 0.0);
        assert_eq!(ease(1.0), 1.0);
        assert!((ease(0.5) - 0.5).abs() < 1.0e-6);
        assert_eq!(swing_lift(0.0, 3.0), 0.0);
        assert!((swing_lift(0.5, 3.0) - 3.0).abs() < 1.0e-5);
        assert!(swing_lift(1.0, 3.0).abs() < 1.0e-5);
    }
}
