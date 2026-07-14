//! Procedural animation: one pure function from explicit state — animation
//! state, ticks in state, accumulated stride distance, speed — to a joint
//! pose. Stride distance (not time) drives the leg cycle, so feet do not
//! slide; everything else keys off fixed ticks. No wall clock anywhere.

use axiom_math::Quat;

use super::model::{
    HELMET, L_FOOT, L_SHIN, L_THIGH, L_UPPER_ARM, PART_COUNT, R_FOREARM, R_SHIN, R_THIGH,
    R_UPPER_ARM, TORSO,
};
use super::model::{L_FOREARM, PELVIS, R_FOOT};
use super::AnimState;

/// A resolved pose: per-joint local rotations plus the root adjustments the
/// rig applies to the whole body.
#[derive(Debug, Clone, Copy)]
pub struct JointPose {
    pub joints: [Quat; PART_COUNT],
    /// Root vertical offset (bob, falls), yards.
    pub root_lift: f32,
    /// Root pitch (forward lean +, backward -), radians.
    pub root_pitch: f32,
    /// Root roll, radians.
    pub root_roll: f32,
}

impl JointPose {
    fn neutral() -> Self {
        JointPose {
            joints: [Quat::IDENTITY; PART_COUNT],
            root_lift: 0.0,
            root_pitch: 0.0,
            root_roll: 0.0,
        }
    }
}

/// Rotation about X (limb swing).
fn qx(a: f32) -> Quat {
    Quat::from_euler_xyz(a, 0.0, 0.0)
}

/// Rotation about Z (limb spread).
fn qz(a: f32) -> Quat {
    Quat::from_euler_xyz(0.0, 0.0, a)
}

/// Leg-cycle radians per yard of stride (one full cycle ≈ 1.9 yd).
const STRIDE_RATE: f32 = core::f32::consts::TAU / 1.9;

/// Resolve the pose for one player this tick.
pub fn pose(anim: AnimState, ticks: u32, stride: f32, speed: f32) -> JointPose {
    let mut out = JointPose::neutral();
    let t = ticks as f32;
    match anim {
        AnimState::ReadyStance => {
            crouch(&mut out, 0.5);
            arms_down(&mut out, 0.25);
        }
        AnimState::Idle => {
            crouch(&mut out, 0.18);
            arms_down(&mut out, 0.12);
            out.root_lift = (t * 0.045).sin() * 0.012;
        }
        AnimState::Jog => run_cycle(&mut out, stride, speed, 0.55),
        AnimState::Sprint => run_cycle(&mut out, stride, speed, 1.0),
        AnimState::DropBack => {
            // Backpedal: shorter inverted cycle, torso upright, ball high.
            run_cycle(&mut out, stride, speed, 0.45);
            out.root_pitch = -0.08;
            out.joints[R_UPPER_ARM] = qx(-0.9);
            out.joints[R_FOREARM] = qx(-1.2);
            out.joints[L_UPPER_ARM] = qx(-0.5);
            out.joints[L_FOREARM] = qx(-1.0);
        }
        AnimState::Throw => throw_pose(&mut out, t),
        AnimState::Catch => {
            // Both arms reach up and forward for the ball.
            out.joints[L_UPPER_ARM] = qx(-2.4);
            out.joints[R_UPPER_ARM] = qx(-2.4);
            out.joints[L_FOREARM] = qx(-0.3);
            out.joints[R_FOREARM] = qx(-0.3);
            out.root_pitch = 0.06;
        }
        AnimState::Block => {
            crouch(&mut out, 0.6);
            // Arms punched forward at pad height.
            out.joints[L_UPPER_ARM] = qx(-1.5);
            out.joints[R_UPPER_ARM] = qx(-1.5);
            out.joints[L_FOREARM] = qx(-0.35);
            out.joints[R_FOREARM] = qx(-0.35);
            out.root_pitch = 0.16;
        }
        AnimState::Tackle => {
            // A wrapping lunge.
            crouch(&mut out, 0.4);
            out.root_pitch = 0.55;
            out.joints[L_UPPER_ARM] = Quat::from_euler_xyz(-1.2, 0.0, -0.5);
            out.joints[R_UPPER_ARM] = Quat::from_euler_xyz(-1.2, 0.0, 0.5);
            out.joints[L_FOREARM] = qx(-0.9);
            out.joints[R_FOREARM] = qx(-0.9);
        }
        AnimState::HitReaction => {
            let k = (1.0 - t / 10.0).max(0.0);
            out.root_pitch = -0.4 * k;
            out.joints[L_UPPER_ARM] = qz(-1.1 * k);
            out.joints[R_UPPER_ARM] = qz(1.1 * k);
        }
        AnimState::Stumble => {
            let k = (t / 10.0).min(1.0);
            out.root_pitch = 0.35 + 0.75 * k;
            out.root_lift = -0.28 * k;
            out.joints[L_UPPER_ARM] = qx(-1.4 * k);
            out.joints[R_UPPER_ARM] = qx(-1.4 * k);
            out.joints[L_THIGH] = qx(0.4 * k);
            out.joints[R_THIGH] = qx(-0.3 * k);
        }
        AnimState::AirborneFall => {
            // Launched: arch back, limbs spread. Height comes from the sim.
            let k = (t / 14.0).min(1.0);
            out.root_pitch = -0.8 - 0.5 * k;
            out.joints[L_UPPER_ARM] = qz(-1.5);
            out.joints[R_UPPER_ARM] = qz(1.5);
            out.joints[L_THIGH] = qx(-0.5);
            out.joints[R_THIGH] = qx(-0.8);
            out.joints[L_SHIN] = qx(0.7);
            out.joints[R_SHIN] = qx(0.9);
        }
        AnimState::GroundImpact => {
            // Flat on the back with a small landing recoil bounce.
            let recoil = (1.0 - t / 6.0).max(0.0);
            out.root_pitch = -core::f32::consts::FRAC_PI_2 * 0.94;
            out.root_lift = -0.72 + recoil * 0.10;
            out.joints[L_UPPER_ARM] = qz(-1.2);
            out.joints[R_UPPER_ARM] = qz(1.2);
            out.joints[L_THIGH] = qx(-0.25);
            out.joints[R_THIGH] = qx(-0.4);
        }
        AnimState::Recovery => {
            // Rise from the turf back to standing over the recovery window.
            let k = (t / 34.0).min(1.0);
            let up = smooth(k);
            out.root_pitch = -core::f32::consts::FRAC_PI_2 * 0.94 * (1.0 - up);
            out.root_lift = -0.72 * (1.0 - up);
            crouch(&mut out, 0.5 * up * (1.0 - up) * 4.0_f32.min(1.0));
        }
    }
    out
}

/// Smoothstep.
fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A knees-bent athletic crouch (`k` = how deep).
fn crouch(out: &mut JointPose, k: f32) {
    out.joints[L_THIGH] = qx(-0.55 * k);
    out.joints[R_THIGH] = qx(-0.55 * k);
    out.joints[L_SHIN] = qx(0.9 * k);
    out.joints[R_SHIN] = qx(0.9 * k);
    out.joints[L_FOOT] = qx(-0.35 * k);
    out.joints[R_FOOT] = qx(-0.35 * k);
    out.root_pitch = 0.22 * k;
    out.root_lift = -0.16 * k;
}

/// Relaxed hanging arms with a slight elbow bend.
fn arms_down(out: &mut JointPose, bend: f32) {
    out.joints[L_UPPER_ARM] = qz(-0.18);
    out.joints[R_UPPER_ARM] = qz(0.18);
    out.joints[L_FOREARM] = qx(-bend);
    out.joints[R_FOREARM] = qx(-bend);
}

/// The locomotion cycle: legs swing with STRIDE (distance), arms counter-swing,
/// torso leans with intensity. `k` scales amplitude (jog vs sprint).
fn run_cycle(out: &mut JointPose, stride: f32, speed: f32, k: f32) {
    let phase = stride * STRIDE_RATE;
    let swing = phase.sin();
    let counter = -swing;
    let amp = 0.5 + 0.55 * k;
    out.joints[L_THIGH] = qx(swing * amp);
    out.joints[R_THIGH] = qx(counter * amp);
    // Shins fold on the back-swing (offset the cycle a quarter phase).
    let fold = (phase - core::f32::consts::FRAC_PI_2).sin().max(0.0);
    let counter_fold = (phase + core::f32::consts::FRAC_PI_2).sin().max(0.0);
    out.joints[L_SHIN] = qx(0.35 + fold * 0.9 * k);
    out.joints[R_SHIN] = qx(0.35 + counter_fold * 0.9 * k);
    out.joints[L_UPPER_ARM] = qx(counter * amp * 0.8);
    out.joints[R_UPPER_ARM] = qx(swing * amp * 0.8);
    out.joints[L_FOREARM] = qx(-0.9 * k);
    out.joints[R_FOREARM] = qx(-0.9 * k);
    out.root_pitch = 0.10 + 0.16 * k * (speed / 9.0).min(1.0);
    out.root_lift = phase.cos().abs() * 0.05 * k;
    // A hint of shoulder roll and head steadiness.
    out.joints[TORSO] = Quat::from_euler_xyz(0.0, swing * 0.08 * k, 0.0);
    out.joints[HELMET] = Quat::from_euler_xyz(-out.root_pitch * 0.6, 0.0, 0.0);
    out.joints[PELVIS] = Quat::IDENTITY;
}

/// The throw: wind up (ball back beside the helmet), then whip through.
fn throw_pose(out: &mut JointPose, t: f32) {
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
