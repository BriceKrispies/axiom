//! The lab's drive scripts: the isolated kinematic actor and how each
//! [`Path`] moves it per tick.
//!
//! Split out of [`super::stage`] so the lab's orchestration (clip selection,
//! snapshot, camera) stays separate from the motion authoring. The scripts
//! exist because the real animator advances the gait on *actual* displacement —
//! a treadmill would hide exactly the foot-skate and weight transfer the lab is
//! there to inspect, so every moving clip travels real ground.

use axiom::prelude::Vec3;

use crate::config::DT;
use crate::lab::catalog::{LabClip, Path};
use crate::player::AnimState;

/// The isolated kinematic body the drive scripts move. This is the lab's stand-in
/// for the simulation's authoritative movement — the *gameplay root*. The
/// animator derives the visual body root from it exactly as it does in a game.
#[derive(Debug, Clone, Copy)]
pub struct Actor {
    pub pos: Vec3,
    pub vel: Vec3,
    pub facing: f32,
    pub anim: AnimState,
    pub anim_ticks: u32,
    /// Circle-path angle (rad) or backpedal travel (yd), by path kind.
    pub path_t: f32,
    /// Ticks elapsed on the current clip, for the speed-ramp script.
    pub script_ticks: u32,
}

impl Actor {
    pub fn rest(anim: AnimState) -> Self {
        Actor {
            pos: Vec3::ZERO,
            vel: Vec3::ZERO,
            facing: 0.0,
            anim,
            anim_ticks: 0,
            path_t: 0.0,
            script_ticks: 0,
        }
    }
}

/// Move the actor one tick along its clip's path. Returns `true` when the path
/// wrapped and the animator must re-anchor (a synthetic play reset) instead of
/// reading the jump as a giant stride.
pub fn advance(actor: &mut Actor, clip: LabClip) -> bool {
    actor.anim = clip.anim;
    actor.anim_ticks += 1;
    actor.script_ticks += 1;
    match clip.path {
        Path::Still => {
            actor.vel = Vec3::ZERO;
            let wrap = clip.loop_ticks > 0 && actor.anim_ticks >= clip.loop_ticks;
            actor.anim_ticks = if wrap { 0 } else { actor.anim_ticks };
            false
        }
        Path::Circle { speed, radius } => {
            circle(actor, speed, radius);
            false
        }
        Path::SpeedRamp {
            low,
            high,
            radius,
            period_ticks,
        } => {
            // A smooth cosine sweep low → high → low: one clip that shows both
            // acceleration into a sprint and deceleration back out of it, with
            // the gait, stride and carriage responding continuously throughout.
            let period = period_ticks.max(1) as f32;
            let t = (actor.script_ticks as f32 / period).rem_euclid(1.0);
            let ramp = 0.5 - 0.5 * (t * core::f32::consts::TAU).cos();
            circle(actor, low + (high - low) * ramp, radius);
            false
        }
        Path::Backpedal { speed, reach } => {
            actor.facing = 0.0;
            actor.path_t += speed * DT;
            let wrap = actor.path_t >= reach;
            actor.path_t = if wrap { 0.0 } else { actor.path_t };
            actor.pos = Vec3::new(0.0, 0.0, -actor.path_t);
            actor.vel = Vec3::new(0.0, 0.0, -speed);
            wrap
        }
    }
}

/// Run the actor along its circle at `speed`, facing along the tangent.
fn circle(actor: &mut Actor, speed: f32, radius: f32) {
    let radius = radius.max(0.5);
    actor.path_t += speed * DT / radius;
    let th = actor.path_t;
    let dir = Vec3::new(th.cos(), 0.0, -th.sin());
    actor.pos = Vec3::new(radius * th.sin(), 0.0, radius * th.cos());
    actor.vel = dir.mul_scalar(speed);
    actor.facing = dir.x.atan2(dir.z);
}
