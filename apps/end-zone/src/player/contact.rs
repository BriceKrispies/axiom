//! The contact framework: blocking engagements, tackle evaluation, and the
//! controlled fall (stumble → airborne → ground impact → recovery). Outcomes
//! are deterministic and authoritative; there is no ragdoll — falls are
//! procedural pose states driven by the fixed tick.

use axiom::prelude::Vec3;

use crate::ai::PlayerIntent;
use crate::collision_rig::CollisionRig;
use crate::data::BehaviorTuning;
use crate::identity::PlayerId;

use super::{AnimState, PlayerSim};

/// Ticks a stumble lasts before the trip completes.
const STUMBLE_TICKS: u32 = 10;
/// Ticks the ground-impact pose holds before recovery starts.
const GROUND_TICKS: u32 = 16;

/// A tackle that landed this tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TackleOutcome {
    pub tackler: PlayerId,
    pub target: PlayerId,
    pub contact_point: Vec3,
    pub contact_direction: Vec3,
    pub relative_speed: f32,
    pub strength: f32,
    pub target_airborne: bool,
}

/// Blocking contact: every blocker in engage range walls their target — the
/// defender's velocity is resisted by the strength contest. Returns the pairs
/// in contact (the sim announces NEW pairs as `BlockEngaged`).
pub fn resolve_blocks(
    players: &mut [PlayerSim],
    intents: &[PlayerIntent],
    tuning: &BehaviorTuning,
) -> Vec<(PlayerId, PlayerId)> {
    let mut pairs = Vec::new();
    for index in 0..players.len() {
        let PlayerIntent::Block { target, .. } = intents[index] else {
            continue;
        };
        if !players[index].anim.can_act() || !players[target.index()].anim.can_act() {
            continue;
        }
        let blocker_pos = players[index].pos;
        let defender = &players[target.index()];
        let away = Vec3::new(
            defender.pos.x - blocker_pos.x,
            0.0,
            defender.pos.z - blocker_pos.z,
        );
        if away.length() <= tuning.block_engage_range {
            let win = (0.5
                + 0.5
                    * (players[index].archetype.block_strength
                        - defender.archetype.block_strength))
                .clamp(0.0, 1.0);
            let resist = 1.0 - tuning.block_resist * win;
            let id = players[index].id;
            let defender = &mut players[target.index()];
            defender.vel = defender.vel.mul_scalar(resist);
            defender.balance = (defender.balance - 0.02).max(0.2);
            pairs.push((id, target));
        }
    }
    pairs
}

/// Tackle evaluation: the first (in tackler id order) in-range, closing-fast
/// tackle on the carrier lands. The hit is authoritative and deterministic:
/// impulse to the carrier, controlled stumble or airborne fall — no ragdoll.
pub fn resolve_tackle(
    players: &mut [PlayerSim],
    intents: &[PlayerIntent],
    carrier: Option<PlayerId>,
    tuning: &BehaviorTuning,
    collision: &CollisionRig,
) -> Option<TackleOutcome> {
    let carrier = carrier?;
    if !players[carrier.index()].anim.can_act() {
        return None;
    }
    for index in 0..players.len() {
        // Either a standing chaser holding a `Tackle` intent, or a committed
        // diver mid-lunge (whose intent has already lapsed — the dive is the
        // commitment) can land the hit here.
        let diving = players[index].anim == AnimState::Dive;
        let standing = matches!(
            intents[index],
            PlayerIntent::Tackle { target, .. } if target == carrier
        ) && players[index].anim.can_act();
        if !(diving || standing) {
            continue;
        }
        let tackler_pos = players[index].pos;
        let carrier_sim = &players[carrier.index()];
        let to_carrier = Vec3::new(
            carrier_sim.pos.x - tackler_pos.x,
            0.0,
            carrier_sim.pos.z - tackler_pos.z,
        );
        let distance = to_carrier.length();
        // A dive lands only on real body contact from the collision world (arc
        // height included); a standing tackle keeps its horizontal arm-reach.
        let landed = if diving {
            collision.in_contact(players[index].id, carrier)
        } else {
            distance <= tuning.tackle_range
        };
        if !landed {
            continue;
        }
        let relative = players[index].vel.subtract(carrier_sim.vel);
        let relative_speed = relative.length() + players[index].speed() * 0.25;
        // A diver is already airborne and past the point of no return — no
        // minimum-closing-speed gate; a standing tackle still needs pop.
        if !diving && relative_speed < tuning.tackle_min_closing_speed {
            continue;
        }
        let direction = if distance > 1.0e-4 {
            to_carrier.mul_scalar(1.0 / distance)
        } else {
            players[index].facing_dir()
        };
        let power = 0.5 + 0.5 * players[index].archetype.tackle_strength;
        let mass_edge = (players[index].archetype.mass / carrier_sim.archetype.mass).min(1.6);
        let strength = ((relative_speed / tuning.tackle_full_strength_speed) * power * mass_edge)
            .clamp(0.05, 1.0);
        let airborne = strength >= tuning.airborne_threshold;

        let contact_point = carrier_sim.pos.add(Vec3::new(0.0, 1.0, 0.0));
        let tackler_id = players[index].id;

        // The carrier takes the hit.
        let hit = &mut players[carrier.index()];
        hit.balance = 0.0;
        hit.impact_strength = strength;
        hit.vel = direction.mul_scalar(relative_speed * 0.35);
        if airborne {
            hit.vertical_vel = tuning.launch_up_speed * strength;
            hit.set_anim(AnimState::AirborneFall);
        } else {
            hit.set_anim(AnimState::Stumble);
        }

        // The tackler commits: a diver wraps and lands prone; a standing
        // tackler plants into the wrap.
        let tackler = &mut players[index];
        if diving {
            tackler.pos = Vec3::new(tackler.pos.x, 0.0, tackler.pos.z);
            tackler.vertical_vel = 0.0;
            tackler.vel = tackler.vel.mul_scalar(0.2);
            tackler.set_anim(AnimState::GroundImpact);
        } else {
            tackler.vel = tackler.vel.mul_scalar(0.25);
            tackler.set_anim(AnimState::Tackle);
        }

        return Some(TackleOutcome {
            tackler: tackler_id,
            target: carrier,
            contact_point,
            contact_direction: direction,
            relative_speed,
            strength,
            target_airborne: airborne,
        });
    }
    None
}

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
        if in_window && closing >= tuning.dive_min_closing_speed && escaping && distance > 1.0e-4 {
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
