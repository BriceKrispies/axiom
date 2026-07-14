//! Defensive role machines: man cover, zone cover, quarterback rush, edge
//! contain, pursuit, tackle. Defenders read the DELAYED perception view (their
//! archetype's reaction delay) and lead their pursuit with a bounded,
//! aggressiveness-scaled prediction — no perfect mirroring.

use axiom::prelude::Vec3;

use crate::identity::PlayerId;
use crate::player::PlayerSim;

use super::assignment::{AssignmentKind, ResolvedAssignment};
use super::brain::{BrainCtx, PerceptionFrame, RoleState};
use super::{steering, PlayerIntent};

/// Decide a defensive player's intent.
pub fn decide(
    player: &PlayerSim,
    assignment: &ResolvedAssignment,
    role: &mut RoleState,
    ctx: &BrainCtx<'_>,
) -> PlayerIntent {
    if !ctx.live {
        *role = RoleState::Waiting;
        return PlayerIntent::Hold;
    }
    *role = RoleState::Defending;
    let delay = player.archetype.reaction_delay_ticks;
    let seen = ctx.perception.sample(delay);

    // A perceived carrier who is NOT the passer (i.e. a runner after the
    // catch) flips coverage into pursuit; the rush roles chase whoever holds
    // the ball, the quarterback included.
    let open_runner = seen.carrier.filter(|&carrier| carrier != ctx.quarterback);

    match assignment.kind {
        AssignmentKind::QuarterbackRush { quarterback } => {
            *role = RoleState::Pursuing;
            let chase = seen.carrier.unwrap_or(quarterback);
            pursue_or_tackle(player, chase, seen, ctx)
        }
        AssignmentKind::EdgeContain { post, .. } => match seen.carrier {
            Some(carrier) => {
                *role = RoleState::Pursuing;
                pursue_or_tackle(player, carrier, seen, ctx)
            }
            None => PlayerIntent::MoveToward {
                point: post,
                sprint: false,
            },
        },
        AssignmentKind::ManCover { target } => match open_runner {
            Some(carrier) => {
                *role = RoleState::Pursuing;
                pursue_or_tackle(player, carrier, seen, ctx)
            }
            None => man_cover(player, target, seen),
        },
        AssignmentKind::ZoneCover { center, radius } => match open_runner {
            Some(carrier) => {
                *role = RoleState::Pursuing;
                pursue_or_tackle(player, carrier, seen, ctx)
            }
            None => {
                if seen.ball_airborne && flat_distance(center, seen.ball_target) < radius {
                    PlayerIntent::PrepareCatch {
                        point: seen.ball_target,
                    }
                } else {
                    PlayerIntent::MoveToward {
                        point: center,
                        sprint: false,
                    }
                }
            }
        },
        AssignmentKind::Pursuit | AssignmentKind::TackleTarget => match open_runner {
            Some(carrier) => {
                *role = RoleState::Pursuing;
                pursue_or_tackle(player, carrier, seen, ctx)
            }
            None => {
                if seen.ball_airborne {
                    // Rally goal-side of the perceived landing point (a deep
                    // defender keeps the catch in front, never camps it).
                    let to_goal = ctx.end_zone_target.subtract(seen.ball_target);
                    let cushion = to_goal
                        .normalize()
                        .unwrap_or(Vec3::UNIT_Z)
                        .mul_scalar(ctx.tuning.pursuit_cushion);
                    PlayerIntent::MoveToward {
                        point: seen.ball_target.add(cushion),
                        sprint: true,
                    }
                } else {
                    // Deep patrol: mirror the perceived ball laterally, hold depth.
                    PlayerIntent::MoveToward {
                        point: Vec3::new(seen.ball_pos.x * 0.5, 0.0, player.pos.z),
                        sprint: false,
                    }
                }
            }
        },
        // Offensive kinds never reach here (dispatched in `brain`).
        _ => PlayerIntent::Hold,
    }
}

/// Trail the assigned receiver: aim at their perceived position plus a
/// bounded, aggressiveness-scaled lead; break on a perceived pass landing
/// near them.
fn man_cover(player: &PlayerSim, target: PlayerId, seen: &PerceptionFrame) -> PlayerIntent {
    let man_pos = seen.positions[target.index()];
    if seen.ball_airborne && flat_distance(man_pos, seen.ball_target) < 8.0 {
        return PlayerIntent::PrepareCatch {
            point: seen.ball_target,
        };
    }
    let man_vel = seen.velocities[target.index()];
    let lead =
        steering::pursuit_lead_seconds(flat_distance(player.pos, man_pos), &player.archetype);
    PlayerIntent::MoveToward {
        point: steering::predict(man_pos, man_vel, lead),
        sprint: true,
    }
}

/// Close on a carrier: tackle inside range, otherwise pursue a predicted
/// interception point (perceived position + bounded lead).
fn pursue_or_tackle(
    player: &PlayerSim,
    carrier: PlayerId,
    seen: &PerceptionFrame,
    ctx: &BrainCtx<'_>,
) -> PlayerIntent {
    let seen_pos = seen.positions[carrier.index()];
    let seen_vel = seen.velocities[carrier.index()];
    let distance = flat_distance(player.pos, seen_pos);
    if distance <= ctx.tuning.tackle_range * 2.0 {
        PlayerIntent::Tackle {
            target: carrier,
            point: seen_pos,
        }
    } else {
        let lead = steering::pursuit_lead_seconds(distance, &player.archetype);
        PlayerIntent::Pursue {
            target: carrier,
            point: steering::predict(seen_pos, seen_vel, lead),
        }
    }
}

fn flat_distance(a: Vec3, b: Vec3) -> f32 {
    Vec3::new(a.x - b.x, 0.0, a.z - b.z).length()
}
