//! The ball-carry arm overlay and the shared pose primitives.
//!
//! Split out of [`super::animation`], which owns the per-state override poses,
//! so each file stays narrowly owned. Pure relocation — every pose is
//! unchanged, and `animation::apply_hold` still names this entry point.

use axiom_math::Quat;

use super::animation::{qx, qz, BallHold, JointPose};
use super::model::{
    L_FOOT, L_FOREARM, L_SHIN, L_THIGH, L_UPPER_ARM, R_FOOT, R_FOREARM, R_SHIN, R_THIGH,
    R_HAND, R_UPPER_ARM, TORSO,
};

/// Overlay the ball-carry arms onto a resolved pose (stage 3 of composition):
/// cradled in the crook, or cocked throw-ready by the ear. A `None` hold leaves
/// the pose untouched. The render side pins the ball to the matching arm.
pub fn apply_hold(out: &mut JointPose, hold: BallHold) {
    match hold {
        BallHold::Cradle => carry_tuck(out),
        BallHold::ThrowReady => throw_ready_arms(out),
        BallHold::None => {}
    }
}

/// The throw-ready hold: the throwing (right) arm cocks the ball up beside the
/// helmet, the off hand braces across it — a quarterback ready to fire in the
/// pocket. The render side pins the ball to the raised right hand.
pub(super) fn throw_ready_arms(out: &mut JointPose) {
    out.joints[R_UPPER_ARM] = Quat::from_euler_xyz(-1.5, 0.0, 0.35);
    out.joints[R_FOREARM] = qx(-1.9);
    out.joints[R_HAND] = qx(-0.2);
    out.joints[L_UPPER_ARM] = Quat::from_euler_xyz(-1.2, 0.0, -0.6);
    out.joints[L_FOREARM] = qx(-1.5);
}

/// The ball-carry tuck: right upper arm pinned in against the ribs, forearm
/// folded up across the torso so the elbow makes a shelf, hand capping over the
/// top. This replaces the free right-arm swing so the ball nestles in the crook
/// (the render side pins the ball's rear tip to this forearm).
pub(super) fn carry_tuck(out: &mut JointPose) {
    out.joints[R_UPPER_ARM] = Quat::from_euler_xyz(-0.35, 0.0, 0.28);
    out.joints[R_FOREARM] = qx(-1.85);
    out.joints[R_HAND] = qx(-0.5);
}

/// Smoothstep.
pub(super) fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A knees-bent athletic crouch (`k` = how deep).
pub(super) fn crouch(out: &mut JointPose, k: f32) {
    out.joints[L_THIGH] = qx(-0.55 * k);
    out.joints[R_THIGH] = qx(-0.55 * k);
    out.joints[L_SHIN] = qx(0.9 * k);
    out.joints[R_SHIN] = qx(0.9 * k);
    out.joints[L_FOOT] = qx(-0.35 * k);
    out.joints[R_FOOT] = qx(-0.35 * k);
    out.root_pitch = 0.22 * k;
    out.root_lift = -0.16 * k;
}

/// The throw: wind up (ball back beside the helmet), then whip through.
pub(super) fn throw_pose(out: &mut JointPose, t: f32) {
    let windup = smooth((t / 8.0).min(1.0));
    let release = smooth(((t - 8.0) / 6.0).clamp(0.0, 1.0));
    crouch(out, 0.25);
    // Right (throwing) arm: back and up, then forward.
    let arm = -2.5 * windup + 3.1 * release;
    out.joints[R_UPPER_ARM] = Quat::from_euler_xyz(arm.min(0.9), 0.0, 0.35 * (1.0 - release));
    out.joints[R_FOREARM] = qx(-1.1 * (1.0 - release));
    // Off arm points at the target then tucks.
    out.joints[L_UPPER_ARM] = qx(-1.6 * (1.0 - release) * windup);
    out.joints[L_FOREARM] = qx(-0.3);
    // Torso twist through the throw.
    out.joints[TORSO] = Quat::from_euler_xyz(0.0, 0.5 * windup - 0.9 * release, 0.0);
    out.root_pitch = 0.05 + 0.22 * release;
}
