//! Building the procedural whole-body pose for a running player from the
//! advanced gait state. Stage 2 of the pose-composition order (overrides and
//! the carry overlay are applied by the module facade).
//!
//! Three passes, in order:
//!
//! 1. **carriage** ([`super::carriage`]) — resolve the whole-body weight
//!    transfer *targets*: where the visual body root and pelvis want to be this
//!    tick, and how the spine counter-rotates against them.
//! 2. **virtual muscle** ([`super::spring`]) — chase those targets with
//!    per-region damped springs at the fixed simulation step, so the body
//!    settles into the pose instead of snapping onto it. The pelvis is stiff,
//!    the arms are loose, which is what makes the body read as connected.
//! 3. **legs** — two-bone IK to the world-locked foot targets. Because the
//!    pelvis has already moved, this *is* the stance-leg compression and
//!    push-off: the planted foot holds its world position while the leg folds
//!    and extends under the travelling body.

use axiom::prelude::Vec3;
use axiom_math::{Quat, Transform};

use crate::data::{BiomechTuning, LocomotionTuning};
use crate::player::animation::JointPose;
use crate::player::model::{
    HELMET, L_FOOT, L_FOREARM, L_SHIN, L_THIGH, L_UPPER_ARM, PADS, PARTS, PELVIS, R_FOOT,
    R_FOREARM, R_SHIN, R_THIGH, R_UPPER_ARM, TORSO,
};
use crate::player::rig;
use crate::player::AnimState;

use super::carriage::{self, Carriage, Carry};
use super::gait::GaitState;
use super::leg::{self, LegDims};
use super::spring::BodySprings;

const TAU: f32 = core::f32::consts::TAU;

/// Rotation about X (limb pitch) — elbow/knee-style flexion.
fn qx(a: f32) -> Quat {
    Quat::from_euler_xyz(a, 0.0, 0.0)
}

/// Build the locomotion pose for one player and fill each foot's solved ankle +
/// lock error into the gait state. `anim` distinguishes the standing stances
/// (a ready crouch, an upright backpedal) that share the distance-driven cycle.
///
/// `springs` is the player's persistent virtual-muscle bank; it is advanced one
/// fixed tick here. The returned [`Carriage`] is the resolved target set, kept
/// for the diagnostic overlay and the biomechanical debug view.
pub fn locomotion_pose(
    gait: &mut GaitState,
    springs: &mut BodySprings,
    facing: f32,
    ground: Vec3,
    anim: AnimState,
    loco: &LocomotionTuning,
    bio: &BiomechTuning,
) -> (JointPose, Carriage) {
    let mut out = JointPose::neutral();
    let carry = match anim {
        AnimState::ReadyStance => Carry::Ready,
        AnimState::DropBack => Carry::Backpedal,
        _ => Carry::Running,
    };

    // Stage 1: the whole-body carriage targets.
    let target = carriage::solve(gait, facing, carry, loco, bio);

    // Stage 2: chase them with the per-region springs. Pelvis and the visual
    // body root are stiffest (weight must look controlled); the spine sits in
    // the middle; the arms are loosest and are allowed to trail.
    let pos_step = bio.max_position_step;
    let ang_step = bio.max_angular_step;
    let (pk, pd) = (bio.pelvis_stiffness, bio.pelvis_damping);
    let (sk, sd) = (bio.spine_stiffness, bio.spine_damping);
    let (ak, ad) = (bio.arm_stiffness, bio.arm_damping);
    let (hk, hd) = (bio.head_stiffness, bio.head_damping);

    // The gait-driven weight transfer rides on top of the carriage's posture
    // offset (how low and how far forward the body is *held* for this stance).
    // Both go through the spring together: a posture is where the body is
    // heading, not something stamped onto the solved pose, so changing stance
    // settles over a few ticks instead of popping the root in one frame.
    out.root_lift =
        springs
            .root_lift
            .step(target.root_lift + target.posture_lift, pk, pd, pos_step);
    out.root_lateral = springs
        .root_lateral
        .step(target.root_lateral, pk, pd, pos_step);
    out.root_pitch =
        springs
            .root_pitch
            .step(target.root_pitch + target.posture_pitch, sk, sd, ang_step);
    out.root_roll = springs.root_roll.step(target.root_roll, sk, sd, ang_step);

    let pelvis_yaw = springs.pelvis_yaw.step(target.pelvis_yaw, pk, pd, ang_step);
    let pelvis_roll = springs
        .pelvis_roll
        .step(target.pelvis_roll, pk, pd, ang_step);
    let pelvis_pitch = springs
        .pelvis_pitch
        .step(target.pelvis_pitch, pk, pd, ang_step);
    let spine_yaw = springs.spine_yaw.step(target.spine_yaw, sk, sd, ang_step);
    let spine_roll = springs.spine_roll.step(target.spine_roll, sk, sd, ang_step);
    let spine_pitch = springs
        .spine_pitch
        .step(target.spine_pitch, sk, sd, ang_step);
    let ribcage_yaw = springs
        .ribcage_yaw
        .step(target.ribcage_yaw, sk, sd, ang_step);
    let ribcage_pitch = springs
        .ribcage_pitch
        .step(target.ribcage_pitch, sk, sd, ang_step);
    let head_pitch = springs.head_pitch.step(target.head_pitch, hk, hd, ang_step);
    let head_yaw = springs.head_yaw.step(target.head_yaw, hk, hd, ang_step);

    out.joints[PELVIS] = Quat::from_euler_xyz(pelvis_pitch, pelvis_yaw, pelvis_roll);
    out.joints[TORSO] = Quat::from_euler_xyz(spine_pitch, spine_yaw, spine_roll);
    out.joints[PADS] = Quat::from_euler_xyz(ribcage_pitch, ribcage_yaw, 0.0);
    out.joints[HELMET] = Quat::from_euler_xyz(head_pitch, head_yaw, 0.0);

    // Arms: swung opposite the legs, through their own (loosest) spring so they
    // trail the torso slightly. They hang off the pad girdle, so they already
    // carry its counter-rotation.
    let speed = gait.norm_speed.clamp(0.0, 1.0);
    let amp = 0.45 + 0.55 * speed;
    let swing_target = (gait.phase * TAU).sin() * loco.arm_swing * amp;
    let swing = springs.arm_swing.step(swing_target, ak, ad, ang_step);
    out.joints[L_UPPER_ARM] = qx(-swing);
    out.joints[R_UPPER_ARM] = qx(swing);
    // Bent elbows: a base flex that deepens toward a full run, plus a pump that
    // closes the elbow of whichever arm drives forward this half-cycle.
    // (`apply_hold` overrides these for a carrier's tucked/throwing arm.)
    let elbow = loco.elbow_flex_idle + (loco.elbow_flex_run - loco.elbow_flex_idle) * speed;
    let pump = loco.elbow_pump * (swing / loco.arm_swing.max(1.0e-3)).clamp(-1.0, 1.0);
    out.joints[L_FOREARM] = qx(-(elbow + pump.max(0.0)));
    out.joints[R_FOREARM] = qx(-(elbow + (-pump).max(0.0)));

    // Stage 3: legs solved to their world ankle targets.
    solve_legs(gait, facing, ground, &mut out);
    (out, target)
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
