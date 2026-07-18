//! The player controller: the ONLY code that moves standing players. It
//! executes typed AI intents under acceleration/turn-rate limits, applies
//! teammate separation and boundary clamping, and derives the locomotion
//! animation state. Player-vs-player de-penetration lives in
//! [`crate::collision_rig`] (real rigid-body contact); other contact outcomes
//! (blocks, tackles, falls) live in [`super::contact`].

use axiom::prelude::Vec3;

use crate::ai::{steering, PlayerIntent};
use crate::data::BehaviorTuning;
use crate::field::OffenseFrame;
use crate::identity::TeamId;

use super::{AnimState, PlayerSim};

/// Speed threshold between idle and jog, and jog and sprint (yd/s).
const IDLE_SPEED: f32 = 0.35;
const SPRINT_SPEED: f32 = 5.4;

/// Integrate every standing player's movement from their intent, in id order.
pub fn integrate_movement(
    players: &mut [PlayerSim],
    intents: &[PlayerIntent],
    live: bool,
    tuning: &BehaviorTuning,
    dt: f32,
) {
    let snapshot: Vec<(Vec3, f32, bool, TeamId)> = players
        .iter()
        .map(|p| (p.pos, p.archetype.body_radius, p.anim.can_act(), p.team))
        .collect();

    for index in 0..players.len() {
        let player = &mut players[index];
        if !player.anim.can_act() {
            continue;
        }
        let intent = &intents[index];

        let desired = match intent.movement() {
            Some((point, sprint)) => {
                let top = if sprint {
                    player.archetype.max_speed
                } else {
                    player.archetype.max_speed * 0.62
                };
                // A committed chaser (pursuit / tackle) runs flat out into
                // contact; everyone else eases into their target.
                let mut v = if intent.closes_hard() {
                    steering::seek(player.pos, point, top)
                } else {
                    steering::arrive(player.pos, point, top, tuning.arrival_radius)
                };
                // Separation applies to TEAMMATES only — closing on an
                // opponent (pursuit, tackling, blocking) must never be
                // steered away; opponent overlap is contact, not spacing.
                let team = player.team;
                let neighbors: Vec<(Vec3, f32)> = snapshot
                    .iter()
                    .enumerate()
                    .filter(|(other, (_, _, standing, other_team))| {
                        *other != index && *standing && *other_team == team
                    })
                    .map(|(_, &(pos, radius, _, _))| (pos, radius))
                    .collect();
                v = v.add(steering::separation(
                    player.pos,
                    player.archetype.body_radius,
                    &neighbors,
                    tuning,
                ));
                v
            }
            None => Vec3::ZERO,
        };

        player.vel = steering::limited_velocity_update(player.vel, desired, &player.archetype, dt);
        let step = player.vel.mul_scalar(dt);
        player.pos = OffenseFrame::clamp_in_bounds(
            Vec3::new(player.pos.x + step.x, player.pos.y, player.pos.z + step.z),
            tuning.bounds_margin * 0.5,
        );
        let speed = player.speed();

        // Facing: explicit request, else movement direction.
        let face = match *intent {
            PlayerIntent::Face { direction } => Some(direction),
            _ => (speed > IDLE_SPEED).then_some(player.vel),
        };
        if let Some(direction) = face {
            player.facing = steering::yaw_of(direction, player.facing);
        }

        set_locomotion_anim(player, intent, live, speed);
        player.balance = (player.balance + 0.15 * dt).min(1.0);
    }
}

/// Locomotion animation from intent + speed (special states are set by the
/// simulation at their events: Catch on completion, falls by the contact
/// framework).
fn set_locomotion_anim(player: &mut PlayerSim, intent: &PlayerIntent, live: bool, speed: f32) {
    let anim = match *intent {
        PlayerIntent::Throw => AnimState::Throw,
        PlayerIntent::Block { .. } if speed < 2.0 => AnimState::Block,
        PlayerIntent::Tackle { .. } => AnimState::Tackle,
        PlayerIntent::PrepareCatch { .. } if speed < 2.0 => AnimState::Catch,
        _ if speed <= IDLE_SPEED => {
            if live {
                AnimState::Idle
            } else {
                AnimState::ReadyStance
            }
        }
        _ if speed < SPRINT_SPEED => AnimState::Jog,
        _ => AnimState::Sprint,
    };
    // The QB keeps the drop-back backpedal while moving against their facing.
    let backpedal = player.vel.dot(player.facing_dir()) < -0.5 && speed > IDLE_SPEED;
    if backpedal && anim == AnimState::Jog {
        player.set_anim(AnimState::DropBack);
    } else {
        player.set_anim(anim);
    }
}
