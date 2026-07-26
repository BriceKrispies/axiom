//! Render interpolation for the decision window's slow motion.
//!
//! # Why this exists
//!
//! The simulation is fixed-step: one tick is always 1/60 s of *simulated* time,
//! and dilation is implemented by spending ticks more slowly (see
//! [`crate::app::EndZoneApp::advance`]). At the window's 0.13× that is one tick
//! every ~7.7 rendered frames — so without interpolation the composition layer
//! re-presents a byte-identical frame seven times and then jumps a whole tick's
//! motion in one frame. That is an 8 fps slideshow played back at 60 Hz, and
//! the eye reads it as stutter rather than as slow motion. Making the dilation
//! shallower only trades stutter frequency for stutter size; it is the *hold
//! and jump* cadence that looks broken, not the step size.
//!
//! The fix is the standard one for a fixed timestep: keep the previous and the
//! current simulation states and draw the frame *between* them, at
//! `alpha = credit toward the next tick`. Every rendered frame then shows a
//! distinct, evenly-spaced pose and the motion is continuous at display rate.
//!
//! Two properties this deliberately keeps:
//!
//! - **The simulation is untouched.** This reads two immutable snapshots and
//!   returns a third; it cannot write authoritative state, so the app's
//!   "presentation never mutates simulation" boundary still holds.
//! - **It renders one tick in the past.** That is inherent to interpolating
//!   (as opposed to extrapolating, which overshoots and jitters on direction
//!   changes). At 60 Hz it is ~16 ms of latency during a slow-motion window
//!   where nothing needs frame-accurate input, and the caller skips
//!   interpolation entirely at full speed so normal play keeps zero latency.

use axiom::prelude::Vec3;

use crate::player::animation::JointPose;

use super::locomotion::PlayerPose;
use super::snapshot::PresentationSnapshot;

/// Linear blend of two scalars.
fn mix(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn mix_vec(a: Vec3, b: Vec3, t: f32) -> Vec3 {
    Vec3::new(mix(a.x, b.x, t), mix(a.y, b.y, t), mix(a.z, b.z, t))
}

/// Blend two yaw angles the short way round, so a player crossing the ±π seam
/// does not spin the long way through a whole revolution mid-window.
fn mix_angle(a: f32, b: f32, t: f32) -> f32 {
    let mut delta = b - a;
    while delta > core::f32::consts::PI {
        delta -= core::f32::consts::TAU;
    }
    while delta < -core::f32::consts::PI {
        delta += core::f32::consts::TAU;
    }
    a + delta * t
}

/// Blend the moving parts of two snapshots.
///
/// Everything discrete — animation state, roles, possession, the attempt view,
/// the end reason — is taken from `curr` unblended: a half-caught ball is not a
/// state, and interpolating an enum would invent one.
pub fn snapshot(
    prev: &PresentationSnapshot,
    curr: &PresentationSnapshot,
    alpha: f32,
) -> PresentationSnapshot {
    let t = alpha.clamp(0.0, 1.0);
    let mut out = curr.clone();
    for (index, view) in out.players.iter_mut().enumerate() {
        let Some(before) = prev.players.get(index) else {
            continue;
        };
        // A teleport (a reset, a re-spot) must not be smeared across the field:
        // blending through it would drag every player across the pitch over
        // several frames. Past this distance the new position simply wins.
        let jumped = mix_vec(before.pos, view.pos, 1.0)
            .subtract(before.pos)
            .length()
            > TELEPORT_YARDS;
        if jumped {
            continue;
        }
        view.pos = mix_vec(before.pos, view.pos, t);
        view.vel = mix_vec(before.vel, view.vel, t);
        view.facing = mix_angle(before.facing, view.facing, t);
    }
    let ball_jumped = out.ball.pos.subtract(prev.ball.pos).length() > TELEPORT_YARDS;
    if !ball_jumped {
        out.ball.pos = mix_vec(prev.ball.pos, out.ball.pos, t);
        out.ball.spin_angle = mix(prev.ball.spin_angle, out.ball.spin_angle, t);
    }
    out
}

/// How far a body may move in one tick before the move is treated as a
/// discontinuity rather than motion, yards. A sprinter covers ~0.15 yd/tick, so
/// this only ever catches resets and re-spots.
const TELEPORT_YARDS: f32 = 3.0;

/// Blend two frames of composed player poses.
///
/// The limbs matter as much as the bodies here: a smoothly gliding torso with
/// legs snapping at 8 Hz reads as a different, weirder artifact rather than as
/// a fix. `Quat::nlerp` is the right tool at these deltas — the per-tick joint
/// rotations are small, where nlerp and slerp are visually identical.
pub fn poses(prev: &[PlayerPose], curr: &[PlayerPose], alpha: f32) -> Vec<PlayerPose> {
    let t = alpha.clamp(0.0, 1.0);
    curr.iter()
        .enumerate()
        .map(|(index, pose)| {
            let Some(before) = prev.get(index) else {
                return *pose;
            };
            // The ball-hold and the diagnostic sample are discrete reads; only
            // the skeleton is blended.
            PlayerPose {
                pose: blend_joints(&before.pose, &pose.pose, t),
                ..*pose
            }
        })
        .collect()
}

fn blend_joints(prev: &JointPose, curr: &JointPose, t: f32) -> JointPose {
    let mut out = *curr;
    for (index, joint) in out.joints.iter_mut().enumerate() {
        let Some(before) = prev.joints.get(index) else {
            continue;
        };
        // A failed blend (a degenerate pair) keeps the current joint, which is
        // the honest fallback: one un-smoothed joint beats an invalid rotation.
        *joint = before.nlerp(*joint, t).unwrap_or(*joint);
    }
    out.root_lift = mix(prev.root_lift, curr.root_lift, t);
    out.root_lateral = mix(prev.root_lateral, curr.root_lateral, t);
    out.root_pitch = mix(prev.root_pitch, curr.root_pitch, t);
    out.root_roll = mix(prev.root_roll, curr.root_roll, t);
    out
}
