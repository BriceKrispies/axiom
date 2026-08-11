//! Procedural OVERRIDE animation: the pure, per-tick body poses for the states
//! that pose their own body — throw, catch, block, tackle, dive, hit reaction,
//! stumble, airborne fall, ground impact, recovery — plus the ball-carry arm
//! overlay. Everything keys off fixed ticks; no wall clock anywhere.
//!
//! Normal on-feet locomotion (idle / jog / sprint / backpedal) is NOT here — it
//! is owned by the app-local, distance-driven, planted-foot locomotion animator
//! ([`crate::presentation::locomotion`]). This module is stages 3-5 of the
//! pose-composition order (carry overlay + action / fall / recovery overrides);
//! the locomotion animator owns stages 1-2 and composes this on top by state.

use axiom_math::Quat;

use super::model::{
    L_FOOT, L_FOREARM, L_SHIN, L_THIGH, L_UPPER_ARM, PART_COUNT, R_FOOT, R_FOREARM, R_HAND, R_SHIN,
    R_THIGH, R_UPPER_ARM, TORSO,
};
use super::AnimState;

// `apply_hold` lives in the overlay module; keep its original path working.
pub use super::pose_overlay::apply_hold;
// The pose primitives moved with it; the state poses below still compose them.
use super::pose_overlay::{crouch, smooth, throw_pose};

/// A resolved pose: per-joint local rotations plus the **visual body root** —
/// the cosmetic frame the rig derives from the authoritative gameplay root.
///
/// The `root_*` fields are presentation-only by construction: they are read by
/// [`crate::player::rig::body_transform`] and by nothing else. The simulation's
/// position, facing, collision shape and tackle geometry all use the gameplay
/// root directly and never see these offsets.
#[derive(Debug, Clone, Copy)]
pub struct JointPose {
    pub joints: [Quat; PART_COUNT],
    /// Visual body root vertical offset (weight transfer, bob, falls), yards.
    pub root_lift: f32,
    /// Visual body root lateral offset along the facing-right axis — the weight
    /// shift toward the stance leg, yards.
    pub root_lateral: f32,
    /// Root pitch (forward lean +, backward -), radians.
    pub root_pitch: f32,
    /// Root roll, radians.
    pub root_roll: f32,
}

impl JointPose {
    /// A rest pose: identity joints, no root adjustment. The base the
    /// locomotion animator and the override poses both build on.
    pub fn neutral() -> Self {
        JointPose {
            joints: [Quat::IDENTITY; PART_COUNT],
            root_lift: 0.0,
            root_lateral: 0.0,
            root_pitch: 0.0,
            root_roll: 0.0,
        }
    }
}

/// Rotation about X (limb swing).
pub(super) fn qx(a: f32) -> Quat {
    Quat::from_euler_xyz(a, 0.0, 0.0)
}

/// Rotation about Z (limb spread).
pub(super) fn qz(a: f32) -> Quat {
    Quat::from_euler_xyz(0.0, 0.0, a)
}

/// How a carrier holds the ball this tick — chosen from the carrier's role and
/// field position, then applied to both the arm pose here and the ball's world
/// transform in `scene_sync`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BallHold {
    /// Not carrying, or in a throw / catch / down anim that poses its own arms.
    None,
    /// Quarterback in the pocket: ball up by the ear, ready to throw.
    ThrowReady,
    /// Cradled in the crook of the arm: a scrambling quarterback past the line,
    /// or any ball-carrier running after a catch or handoff.
    Cradle,
}

/// Decide the hold from the carrier's situation: a quarterback still behind the
/// line of scrimmage keeps the ball throw-ready; once past the line — or for any
/// non-quarterback carrier — it is cradled. A player not holding the ball in
/// hand (throwing, catching, down, or simply not the carrier) gets no override.
pub fn ball_hold(
    carrying: bool,
    is_quarterback: bool,
    past_line: bool,
    anim: AnimState,
) -> BallHold {
    if !(carrying && anim.holds_ball()) {
        BallHold::None
    } else if is_quarterback && !past_line {
        BallHold::ThrowReady
    } else {
        BallHold::Cradle
    }
}

/// Resolve the OVERRIDE pose for one player this tick — the action / fall /
/// recovery states that pose their own body. Normal locomotion states (idle,
/// jog, sprint, backpedal, ready stance) are handled by the locomotion animator
/// and never routed here; if one arrives (defensively), it yields the neutral
/// base so the caller's own locomotion pose stands.
pub fn override_pose(anim: AnimState, ticks: u32) -> JointPose {
    let mut out = JointPose::neutral();
    let t = ticks as f32;
    match anim {
        AnimState::ReadyStance
        | AnimState::Idle
        | AnimState::Jog
        | AnimState::Sprint
        | AnimState::DropBack => {}
        AnimState::Throw => throw_pose(&mut out, t),
        // The exchange: the quarterback's arms extended low and forward,
        // offering the ball into the back's belly. Short, still, and readable —
        // the whole point is that the player can see the ball change hands.
        AnimState::HandOff => {
            out.joints[L_UPPER_ARM] = qx(-1.15);
            out.joints[R_UPPER_ARM] = qx(-1.15);
            out.joints[L_FOREARM] = qx(-0.25);
            out.joints[R_FOREARM] = qx(-0.25);
            out.root_pitch = 0.22;
        }
        // The plant-and-cut: the whole body thrown across the new line. The
        // bank is what makes a juke *read* as a deliberate move rather than as
        // the runner sliding sideways, so it is deliberately large.
        AnimState::Juke => {
            out.root_roll = 0.42;
            out.root_pitch = 0.10;
            out.joints[L_UPPER_ARM] = Quat::from_euler_xyz(-0.7, 0.0, -0.9);
            out.joints[R_UPPER_ARM] = Quat::from_euler_xyz(-0.3, 0.0, 0.5);
            out.joints[L_THIGH] = qx(-0.75);
            out.joints[R_THIGH] = qx(0.35);
            out.joints[L_SHIN] = qx(0.55);
        }
        // The shoulder: low, square, and driving. Pad height, not head height.
        AnimState::Shoulder => {
            crouch(&mut out, 0.75);
            out.root_pitch = 0.62;
            out.joints[L_UPPER_ARM] = Quat::from_euler_xyz(-0.5, 0.0, -0.35);
            out.joints[R_UPPER_ARM] = Quat::from_euler_xyz(-0.9, 0.0, 0.25);
            out.joints[R_FOREARM] = qx(-1.1);
        }
        // The leap: tucked knees and an outstretched lead arm. Deliberately not
        // the airborne-FALL pose — that is a man who has lost the play, and this
        // is a man who is winning one.
        AnimState::Leap => {
            out.root_pitch = 0.18;
            out.joints[L_THIGH] = qx(-1.25);
            out.joints[R_THIGH] = qx(-0.85);
            out.joints[L_SHIN] = qx(1.35);
            out.joints[R_SHIN] = qx(0.95);
            out.joints[L_UPPER_ARM] = qx(-2.0);
            out.joints[R_UPPER_ARM] = Quat::from_euler_xyz(-0.6, 0.0, 0.4);
        }
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
            // Arms punched forward at pad height AND spread wide to the sides:
            // an engaged lineman hand-fights with his hands apart, controlling
            // the defender — a broad wingspan wall, not two poles poking ahead.
            out.joints[L_UPPER_ARM] = Quat::from_euler_xyz(-1.4, 0.0, -0.6);
            out.joints[R_UPPER_ARM] = Quat::from_euler_xyz(-1.4, 0.0, 0.6);
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
        AnimState::Dive => {
            // Left the feet: body pitched flat-forward, both arms thrown out to
            // wrap, legs trailing straight back. Ramps in over the first ticks.
            let k = (t / 6.0).min(1.0);
            out.root_pitch = 0.5 + 0.85 * k;
            out.joints[L_UPPER_ARM] = Quat::from_euler_xyz(-2.3, 0.0, -0.45);
            out.joints[R_UPPER_ARM] = Quat::from_euler_xyz(-2.3, 0.0, 0.45);
            out.joints[L_FOREARM] = qx(-0.35);
            out.joints[R_FOREARM] = qx(-0.35);
            out.joints[L_THIGH] = qx(0.45 * k);
            out.joints[R_THIGH] = qx(0.55 * k);
            out.joints[L_SHIN] = qx(0.25 * k);
            out.joints[R_SHIN] = qx(0.35 * k);
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
