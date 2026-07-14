//! Possession sockets and deterministic catch evaluation.
//!
//! The carry socket is the AUTHORITATIVE held-ball position: a pure function
//! of the carrier's simulation pose (position, facing, animation state). The
//! render model draws the ball exactly at the sim socket, so what the camera
//! frames is what the simulation owns.

use axiom::prelude::Vec3;

use crate::data::PlayerArchetype;
use crate::player::AnimState;

/// Height of the carry tuck above the ground, yards.
const CARRY_HEIGHT: f32 = 1.05;
/// Forward/lateral tuck offsets from the carrier center, yards.
const CARRY_FORWARD: f32 = 0.34;
const CARRY_SIDE: f32 = 0.30;
/// The throw wind-up raises the ball beside the helmet.
const WINDUP_HEIGHT: f32 = 1.95;

/// The world-space socket a held ball follows. `facing` is the carrier's yaw
/// (radians; `0` faces `+Z`), `ground` the carrier's ground position.
pub fn carry_socket(ground: Vec3, facing: f32, anim: AnimState) -> Vec3 {
    let forward = Vec3::new(facing.sin(), 0.0, facing.cos());
    let right = Vec3::new(forward.z, 0.0, -forward.x);
    match anim {
        AnimState::Throw => ground
            .add(right.mul_scalar(CARRY_SIDE + 0.12))
            .add(forward.mul_scalar(-0.15))
            .add(Vec3::new(0.0, WINDUP_HEIGHT, 0.0)),
        AnimState::Catch => ground
            .add(forward.mul_scalar(CARRY_FORWARD + 0.3))
            .add(Vec3::new(0.0, 1.45, 0.0)),
        _ => ground
            .add(right.mul_scalar(CARRY_SIDE))
            .add(forward.mul_scalar(CARRY_FORWARD))
            .add(Vec3::new(0.0, CARRY_HEIGHT, 0.0)),
    }
}

/// The point a receiver tries to put the ball in (chest height above their
/// ground position).
pub fn catch_point(ground: Vec3) -> Vec3 {
    ground.add(Vec3::new(0.0, 1.45, 0.0))
}

/// The outcome of one tick's catch evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatchVerdict {
    /// Ball too far from the catch volume — keep running the route.
    OutOfReach,
    /// Inside the volume but outside the timing window (too early/late).
    BadTiming,
    /// Catch!
    Caught,
}

/// Deterministic catch evaluation: the ball must be inside the receiver's
/// catch volume (a sphere of `archetype.catch_radius` around the catch point)
/// AND the tick must be within the archetype's tolerance of the predicted
/// arrival. A player who is falling or down cannot catch (gated by `can_act`).
pub fn evaluate_catch(
    ball_pos: Vec3,
    receiver_ground: Vec3,
    archetype: &PlayerArchetype,
    tick: u64,
    arrival_tick: u64,
    can_act: bool,
) -> CatchVerdict {
    if !can_act {
        return CatchVerdict::OutOfReach;
    }
    let center = catch_point(receiver_ground);
    let distance = ball_pos.subtract(center).length();
    if distance > archetype.catch_radius {
        return CatchVerdict::OutOfReach;
    }
    let tolerance = u64::from(archetype.catch_tolerance_ticks);
    let window = tick.abs_diff(arrival_tick) <= tolerance;
    if window {
        CatchVerdict::Caught
    } else {
        CatchVerdict::BadTiming
    }
}
