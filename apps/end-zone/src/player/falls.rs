//! Dive commitment and the fall/recovery progression.
//!
//! Split out of [`super::contact`], which owns tackle resolution, so each file
//! stays narrowly owned. Pure relocation — no behaviour changed.

use axiom::prelude::Vec3;
use super::contact::{GROUND_TICKS, STUMBLE_TICKS};
use crate::ai::PlayerIntent;
use crate::collision_rig::CollisionRig;
use crate::data::BehaviorTuning;
use crate::identity::PlayerId;
use super::tackle::{self, TackleContest};
use super::{AnimState, PlayerSim};

/// Commit diving tackles: a chaser holding a `Tackle` intent whose carrier is
/// just beyond standing range, closing fast, and actually escaping (moving)
/// leaves their feet — a ballistic forward lunge. The dive is landed later by
/// [`resolve_tackle`]'s dive path, or whiffed into the turf by [`advance_falls`].
/// Called only when no standing tackle landed this tick.
pub fn commit_dives(
    players: &mut [PlayerSim],
    intents: &[PlayerIntent],
    carrier: Option<PlayerId>,
    tuning: &BehaviorTuning,
) {
    let Some(carrier) = carrier else {
        return;
    };
    if !players[carrier.index()].anim.can_act() {
        return;
    }
    for index in 0..players.len() {
        let PlayerIntent::Tackle { target, .. } = intents[index] else {
            continue;
        };
        if target != carrier || !players[index].anim.can_act() {
            continue;
        }
        let tackler_pos = players[index].pos;
        let carrier_sim = &players[carrier.index()];
        let to = Vec3::new(
            carrier_sim.pos.x - tackler_pos.x,
            0.0,
            carrier_sim.pos.z - tackler_pos.z,
        );
        let distance = to.length();
        let in_window =
            distance > tuning.tackle_range && distance <= tuning.tackle_range * tuning.dive_window;
        let relative = players[index].vel.subtract(carrier_sim.vel);
        let closing = relative.length() + players[index].speed() * 0.25;
        let escaping = carrier_sim.speed() >= tuning.dive_carrier_min_speed;
        // A flat-out runner matched for speed is WRAPPED standing, not dived at.
        // A committed dive is ballistic: it whiffs against a juke and, having left
        // its feet, the diver is spent and removed from the play. So when the
        // carrier is at a full sprint (>= 85% of its own top speed) AND this
        // tackler is fast enough to stay stride-for-stride (its top speed meets
        // the carrier's current speed), the tackler keeps its feet and lets the
        // standing tracking-tackle in `resolve_tackle` finish the play — a
        // juke-proof run-down. The gate keys on the carrier being FLAT-OUT so a
        // slower carrier (a scrambling QB) is still dived at, and the fast-chaser-
        // on-slow-carrier dive path stays intact.
        let carrier_flat_out = carrier_sim.speed() >= 0.85 * carrier_sim.archetype.max_speed;
        let can_stay_with = players[index].archetype.max_speed >= carrier_sim.speed();
        let wrap_instead = carrier_flat_out && can_stay_with;
        if in_window
            && closing >= tuning.dive_min_closing_speed
            && escaping
            && !wrap_instead
            && distance > 1.0e-4
        {
            let dir = to.mul_scalar(1.0 / distance);
            let diver = &mut players[index];
            diver.facing = dir.x.atan2(dir.z);
            diver.vel = dir.mul_scalar(tuning.dive_launch_forward);
            diver.vertical_vel = tuning.dive_launch_up;
            diver.impact_strength = tuning.dive_whiff_impact;
            diver.set_anim(AnimState::Dive);
        }
    }
}

/// Advance controlled falls: airborne arcs under gravity, stumbles that trip,
/// the ground-impact hold, and recovery back to standing. Returns the players
/// who hit the turf this tick (with their stored impact strengths).
pub fn advance_falls(
    players: &mut [PlayerSim],
    tuning: &BehaviorTuning,
    dt: f32,
) -> Vec<(PlayerId, f32)> {
    let mut impacts = Vec::new();
    for player in players.iter_mut() {
        match player.anim {
            AnimState::Dive => {
                // Ballistic forward lunge under gravity; a landed dive is
                // grounded by `resolve_tackle` before this runs, so reaching
                // the turf here is a whiff.
                player.vertical_vel -= tuning.gravity * dt;
                player.pos = Vec3::new(
                    player.pos.x + player.vel.x * dt,
                    (player.pos.y + player.vertical_vel * dt).max(0.0),
                    player.pos.z + player.vel.z * dt,
                );
                player.vel = player.vel.mul_scalar(0.99);
                if player.pos.y <= 0.0 && player.vertical_vel < 0.0 {
                    player.pos = Vec3::new(player.pos.x, 0.0, player.pos.z);
                    player.vertical_vel = 0.0;
                    player.vel = player.vel.mul_scalar(0.15);
                    player.set_anim(AnimState::GroundImpact);
                    impacts.push((player.id, player.impact_strength));
                }
            }
            AnimState::AirborneFall => {
                player.vertical_vel -= tuning.gravity * dt;
                player.pos = Vec3::new(
                    player.pos.x + player.vel.x * dt,
                    (player.pos.y + player.vertical_vel * dt).max(0.0),
                    player.pos.z + player.vel.z * dt,
                );
                player.vel = player.vel.mul_scalar(0.985);
                if player.pos.y <= 0.0 && player.vertical_vel < 0.0 {
                    player.pos = Vec3::new(player.pos.x, 0.0, player.pos.z);
                    player.vertical_vel = 0.0;
                    player.vel = player.vel.mul_scalar(0.2);
                    player.set_anim(AnimState::GroundImpact);
                    impacts.push((player.id, player.impact_strength));
                }
            }
            // Bounced off a carrier he could not bring down: on his feet, but
            // out of the play for a beat. Without this the shed defender simply
            // re-attempts on the very next tick and nothing was survived.
            AnimState::HitReaction => {
                player.pos = Vec3::new(
                    player.pos.x + player.vel.x * dt,
                    0.0,
                    player.pos.z + player.vel.z * dt,
                );
                player.vel = player.vel.mul_scalar(0.86);
                if player.anim_ticks >= tuning.hit_reaction_ticks {
                    player.set_anim(AnimState::Idle);
                }
            }
            AnimState::Stumble => {
                player.pos = Vec3::new(
                    player.pos.x + player.vel.x * dt,
                    0.0,
                    player.pos.z + player.vel.z * dt,
                );
                player.vel = player.vel.mul_scalar(0.92);
                if player.anim_ticks >= STUMBLE_TICKS {
                    player.vel = player.vel.mul_scalar(0.2);
                    player.set_anim(AnimState::GroundImpact);
                    impacts.push((player.id, player.impact_strength));
                }
            }
            AnimState::GroundImpact => {
                player.vel = player.vel.mul_scalar(0.8);
                if player.anim_ticks >= GROUND_TICKS {
                    player.set_anim(AnimState::Recovery);
                }
            }
            AnimState::Recovery => {
                if player.anim_ticks >= tuning.recovery_ticks {
                    player.balance = 1.0;
                    player.set_anim(AnimState::Idle);
                }
            }
            _ => {}
        }
    }
    impacts
}
