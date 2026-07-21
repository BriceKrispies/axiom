//! Building the procedural whole-body pose for a running player from the
//! advanced gait state: the two legs are solved by inverse kinematics to their
//! planted / swinging world foot targets (so the stance foot stays fixed on the
//! turf while the body travels over it — the anti-skate), and the pelvis, torso,
//! shoulders, and arms move with the gait phase and resolved motion. This is
//! stage 2 of the pose-composition order; overrides and the carry overlay are
//! applied by the module facade.

use axiom::prelude::Vec3;
use axiom_math::{Quat, Transform};

use crate::data::LocomotionTuning;
use crate::player::animation::JointPose;
use crate::player::model::{
    HELMET, L_FOOT, L_FOREARM, L_SHIN, L_THIGH, L_UPPER_ARM, PARTS, PELVIS, R_FOOT, R_FOREARM,
    R_SHIN, R_THIGH, R_UPPER_ARM, TORSO,
};
use crate::player::rig;
use crate::player::AnimState;

use super::gait::GaitState;
use super::leg::{self, LegDims};

const TAU: f32 = core::f32::consts::TAU;

/// Rotation about X (limb pitch) — elbow/knee-style flexion.
fn qx(a: f32) -> Quat {
    Quat::from_euler_xyz(a, 0.0, 0.0)
}

/// Build the locomotion pose for one player and fill each foot's solved ankle +
/// lock error into the gait state. `anim` distinguishes the standing stances
/// (a ready crouch, an upright backpedal) that share the distance-driven cycle.
pub fn locomotion_pose(
    gait: &mut GaitState,
    facing: f32,
    ground: Vec3,
    anim: AnimState,
    tuning: &LocomotionTuning,
) -> JointPose {
    let mut out = JointPose::neutral();
    let phase_ang = gait.phase * TAU;
    let swing = phase_ang.sin();
    let amp = 0.45 + 0.55 * gait.norm_speed;
    let speed = gait.norm_speed.clamp(0.0, 1.0);
    let backpedal = matches!(anim, AnimState::DropBack);
    let ready = matches!(anim, AnimState::ReadyStance);

    // Whole-body motion (stage 2b): pelvis bob + yaw, torso counter-rotation and
    // lean/bank, shoulder counter-rotation, arm swing. Bounded by the tuning.
    let forward = Vec3::new(facing.sin(), 0.0, facing.cos());
    let accel_fwd = gait.accel.dot(forward);
    let lean_sign = if backpedal { -1.0 } else { 1.0 };
    // Forward carriage from the waist while running (a backpedal leans back; a
    // set ready stance stays upright), layered on the whole-body `root_pitch`.
    let waist = tuning.waist_lean * speed * lean_sign * f32::from(!ready);
    out.root_pitch = (lean_sign * 0.08 * gait.norm_speed
        + (accel_fwd * tuning.torso_lean_per_accel))
        .clamp(-0.18, tuning.torso_lean_max)
        * f32::from(!ready)
        + ready_crouch_pitch(ready);
    out.root_roll =
        (tuning.torso_bank * gait.turn_bank).clamp(-tuning.torso_bank, tuning.torso_bank);
    // Two vertical dips per cycle (one per foot strike) plus a landing dip, over
    // hips that ride lower the faster the runner goes — the crouch that leaves
    // the stance knee room to flex instead of solving to a locked-straight leg.
    let bob = (phase_ang * 2.0).sin().abs() * tuning.pelvis_bob;
    out.root_lift = bob - landing_dip(gait.phase, tuning) + ready_crouch_lift(ready)
        - tuning.run_crouch * speed * f32::from(!ready);

    out.joints[PELVIS] = Quat::from_euler_xyz(0.0, tuning.pelvis_yaw * swing, 0.0);
    out.joints[TORSO] = Quat::from_euler_xyz(waist, -tuning.shoulder_counter * swing, 0.0);
    // Keep the head up: counter the whole-body root lean and most of the waist
    // lean the head inherits from the torso, so eyes stay forward at speed.
    out.joints[HELMET] = Quat::from_euler_xyz(-out.root_pitch * 0.6 - waist * 0.75, 0.0, 0.0);
    out.joints[L_UPPER_ARM] = Quat::from_euler_xyz(-swing * tuning.arm_swing * amp, 0.0, 0.0);
    out.joints[R_UPPER_ARM] = Quat::from_euler_xyz(swing * tuning.arm_swing * amp, 0.0, 0.0);
    // Bent elbows: hold a base flex that deepens toward a full run, and let each
    // elbow pump — closing on the arm that drives forward this half-cycle. The
    // left arm leads while `swing > 0` (its shoulder pitches back), the right
    // while `swing < 0`, mirroring the upper-arm swing above. (`apply_hold`
    // overrides these for a ball carrier's tucked/throwing arm.)
    let elbow = tuning.elbow_flex_idle + (tuning.elbow_flex_run - tuning.elbow_flex_idle) * speed;
    out.joints[L_FOREARM] = qx(-(elbow + tuning.elbow_pump * swing.max(0.0)));
    out.joints[R_FOREARM] = qx(-(elbow + tuning.elbow_pump * (-swing).max(0.0)));

    // Legs (stage 2a): solve each to its world ankle target.
    solve_legs(gait, facing, ground, &mut out);
    out
}

/// A deeper knee bend and lower hips for the pre-snap ready stance.
fn ready_crouch_pitch(ready: bool) -> f32 {
    f32::from(ready) * 0.2
}
fn ready_crouch_lift(ready: bool) -> f32 {
    f32::from(ready) * -0.14
}

/// The compression dip near each foot strike (phase 0 and ½), yd.
fn landing_dip(phase: f32, tuning: &LocomotionTuning) -> f32 {
    let d0 = (phase * TAU).cos().max(0.0);
    let d1 = ((phase - 0.5) * TAU).cos().max(0.0);
    d0.max(d1) * tuning.landing_dip
}

/// Solve both legs to their gait foot targets and record the solved ankle +
/// the planted-foot lock error (how far the solve missed the world target).
fn solve_legs(gait: &mut GaitState, facing: f32, ground: Vec3, out: &mut JointPose) {
    let dims = LegDims::from_model();
    let body = rig::body_transform(ground, facing, out, 0.0);
    let pelvis_local = Transform::new(PARTS[PELVIS].offset, out.joints[PELVIS], Vec3::ONE);
    let r_parent = body.rotation.multiply(out.joints[PELVIS]);
    let forward = Vec3::new(facing.sin(), 0.0, facing.cos());
    let level = Quat::from_euler_xyz(0.0, facing, 0.0);

    let hip_l = body.transform_point(pelvis_local.transform_point(PARTS[L_THIGH].offset));
    let hip_r = body.transform_point(pelvis_local.transform_point(PARTS[R_THIGH].offset));
    let left = leg::solve(dims, r_parent, hip_l, gait.left.target, forward);
    let right = leg::solve(dims, r_parent, hip_r, gait.right.target, forward);

    out.joints[L_THIGH] = left.thigh;
    out.joints[L_SHIN] = left.shin;
    out.joints[L_FOOT] = fl_level(r_parent, left.thigh, left.shin, level);
    out.joints[R_THIGH] = right.thigh;
    out.joints[R_SHIN] = right.shin;
    out.joints[R_FOOT] = fl_level(r_parent, right.thigh, right.shin, level);

    gait.left.ankle = left.ankle;
    gait.left.lock_error = gait.left.target.distance(left.ankle);
    gait.right.ankle = right.ankle;
    gait.right.lock_error = gait.right.target.distance(right.ankle);
}

/// The foot joint that levels the sole with the field (counter-rotates the
/// accumulated thigh + shin so the foot world orientation is a plain yaw).
fn fl_level(r_parent: Quat, thigh: Quat, shin: Quat, level: Quat) -> Quat {
    let shin_world = r_parent.multiply(thigh).multiply(shin);
    shin_world
        .inverse()
        .map(|inv| inv.multiply(level))
        .unwrap_or(Quat::IDENTITY)
}
