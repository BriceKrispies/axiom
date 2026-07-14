//! Offensive role machines: quarterback, route runner, blocker, ball carrier.
//! Generic over the data — no player- or team-special cases.

use axiom::prelude::Vec3;

use crate::field::OffenseFrame;
use crate::football::BallState;
use crate::player::PlayerSim;

use super::assignment::{AssignmentKind, ResolvedAssignment};
use super::brain::{BrainCtx, RoleState};
use super::PlayerIntent;

/// A route waypoint is "reached" inside this range, yards.
const WAYPOINT_RANGE: f32 = 0.9;

/// Decide an offensive player's intent.
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
    match assignment.kind {
        AssignmentKind::Quarterback { drop_to, .. } => quarterback(player, drop_to, role, ctx),
        AssignmentKind::Snapper | AssignmentKind::PassBlock => pass_block(player, role, ctx),
        AssignmentKind::LeadBlock => lead_block(player, role, ctx),
        AssignmentKind::Route { .. } => route_runner(player, assignment, role, ctx),
        AssignmentKind::BallCarry => {
            *role = RoleState::Carrying;
            carry(player, ctx)
        }
        // Defensive kinds never reach here (dispatched in `brain`).
        _ => PlayerIntent::Hold,
    }
}

/// The quarterback: drop back, scan, wind up on command. The simulation owns
/// the actual release.
fn quarterback(
    player: &PlayerSim,
    drop_to: Vec3,
    role: &mut RoleState,
    ctx: &BrainCtx<'_>,
) -> PlayerIntent {
    let holds_ball = ctx.possession == Some(player.id);
    match *role {
        RoleState::QbWindup { .. } => PlayerIntent::Throw,
        RoleState::QbDone => PlayerIntent::Hold,
        _ => {
            if holds_ball && ctx.throw_commanded {
                *role = RoleState::QbWindup { since: ctx.tick };
                return PlayerIntent::Throw;
            }
            let to_drop = drop_to.subtract(player.pos);
            let far = Vec3::new(to_drop.x, 0.0, to_drop.z).length() > 0.5;
            if far && holds_ball {
                *role = RoleState::QbDrop;
                PlayerIntent::MoveToward {
                    point: drop_to,
                    sprint: false,
                }
            } else if holds_ball {
                *role = RoleState::QbScan;
                PlayerIntent::Face {
                    direction: ctx.end_zone_target.subtract(player.pos),
                }
            } else {
                // Waiting on the snap flight.
                PlayerIntent::Hold
            }
        }
    }
}

/// Route runner: chase waypoints; break to the ball when it is thrown to us;
/// (the carry promotion happens in `brain::decide`).
fn route_runner(
    player: &PlayerSim,
    assignment: &ResolvedAssignment,
    role: &mut RoleState,
    ctx: &BrainCtx<'_>,
) -> PlayerIntent {
    // A live pass intended for us overrides the route.
    if let BallState::Airborne { flight } = ctx.ball.state {
        if flight.intended == player.id {
            *role = RoleState::CatchWork;
            return PlayerIntent::PrepareCatch {
                point: flight.target,
            };
        }
    }
    let index = match *role {
        RoleState::Route { index } => index,
        RoleState::CatchWork => {
            // The pass is gone (caught elsewhere or dead): hold position.
            return PlayerIntent::Hold;
        }
        RoleState::RouteDone => {
            return PlayerIntent::Face {
                direction: ctx.end_zone_target.subtract(player.pos).mul_scalar(-1.0),
            };
        }
        _ => {
            *role = RoleState::Route { index: 0 };
            0
        }
    };
    match assignment.route.get(index) {
        None => {
            *role = RoleState::RouteDone;
            PlayerIntent::Hold
        }
        Some(&waypoint) => {
            let flat = Vec3::new(waypoint.x - player.pos.x, 0.0, waypoint.z - player.pos.z);
            if flat.length() < WAYPOINT_RANGE {
                *role = RoleState::Route { index: index + 1 };
            }
            PlayerIntent::MoveToward {
                point: waypoint,
                sprint: true,
            }
        }
    }
}

/// Pass protection: pick the nearest threat closing on the passer (or the
/// carrier) and wall them off at the midpoint.
fn pass_block(player: &PlayerSim, role: &mut RoleState, ctx: &BrainCtx<'_>) -> PlayerIntent {
    *role = RoleState::Blocking;
    let protect = ctx
        .possession
        .map(|id| ctx.players[id.index()].pos)
        .unwrap_or(player.pos);
    match nearest_opponent(player, ctx) {
        Some((threat, threat_pos)) => {
            let point = Vec3::new(
                (threat_pos.x + protect.x) * 0.5,
                0.0,
                (threat_pos.z + protect.z) * 0.5,
            );
            PlayerIntent::Block {
                target: threat,
                point,
            }
        }
        None => PlayerIntent::Hold,
    }
}

/// Lead blocking: work downfield of the carrier and wall the nearest threat.
fn lead_block(player: &PlayerSim, role: &mut RoleState, ctx: &BrainCtx<'_>) -> PlayerIntent {
    *role = RoleState::Blocking;
    match nearest_opponent(player, ctx) {
        Some((threat, threat_pos)) => PlayerIntent::Block {
            target: threat,
            point: threat_pos,
        },
        None => PlayerIntent::MoveToward {
            point: ctx.end_zone_target,
            sprint: false,
        },
    }
}

/// Carry the ball: run for the end zone, drifting toward the middle third.
pub fn carry(player: &PlayerSim, ctx: &BrainCtx<'_>) -> PlayerIntent {
    let target = Vec3::new(
        player.pos.x * 0.6 + ctx.end_zone_target.x * 0.4,
        0.0,
        ctx.end_zone_target.z,
    );
    PlayerIntent::Carry {
        point: OffenseFrame::clamp_in_bounds(target, ctx.tuning.bounds_margin),
    }
}

/// The nearest standing opponent (true positions — offense reads the field
/// honestly; only defenders are perception-delayed).
fn nearest_opponent(
    player: &PlayerSim,
    ctx: &BrainCtx<'_>,
) -> Option<(crate::identity::PlayerId, Vec3)> {
    let mut best: Option<(crate::identity::PlayerId, Vec3, f32)> = None;
    for other in ctx.players {
        if other.team == player.team || !other.anim.can_act() {
            continue;
        }
        let d = Vec3::new(other.pos.x - player.pos.x, 0.0, other.pos.z - player.pos.z).length();
        let closer = best.map(|(_, _, bd)| d < bd).unwrap_or(true);
        if closer {
            best = Some((other.id, other.pos, d));
        }
    }
    best.map(|(id, pos, _)| (id, pos))
}
