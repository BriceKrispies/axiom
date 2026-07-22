//! Offensive candidate generators: quarterback, route runner, pass/lead
//! blocker, ball carrier. Each pushes a few [`ScoredAction`]s onto the shared
//! candidate buffer; the arbiter picks one. Generic over the data — no player-
//! or team-special cases. Positional identity lives in *which* actions a role
//! offers and their weights (spec §9); the machinery is one scored contest.

use axiom::prelude::Vec3;

use crate::field::OffenseFrame;
use crate::football::BallState;
use crate::player::PlayerSim;

use super::action::{Priority, ScoredAction};
use super::assignment::{AssignmentKind, ResolvedAssignment};
use super::brain::{BrainCtx, RoleState};
use super::PlayerIntent;

/// A route waypoint is "reached" inside this range, yards.
const WAYPOINT_RANGE: f32 = 0.9;
/// A loose ball inside this range is worth chasing, yards.
const LOOSE_ALERT: f32 = 14.0;

/// Push an offensive player's candidate actions.
pub fn candidates(
    player: &PlayerSim,
    assignment: &ResolvedAssignment,
    role: &mut RoleState,
    ctx: &BrainCtx<'_>,
    out: &mut Vec<ScoredAction>,
) {
    if !ctx.live {
        *role = RoleState::Waiting;
        out.push(ScoredAction::new(
            PlayerIntent::Hold,
            Priority::Assignment,
            0.0,
            "set",
            1,
        ));
        return;
    }
    loose_ball_candidate(player, ctx, out);
    match assignment.kind {
        AssignmentKind::Quarterback { drop_to } => quarterback(player, drop_to, role, ctx, out),
        AssignmentKind::Snapper | AssignmentKind::PassBlock => {
            super::protection::pass_block(player, role, ctx, out)
        }
        AssignmentKind::LeadBlock => super::protection::lead_block(player, role, ctx, out),
        AssignmentKind::Route { .. } => route_runner(player, assignment, role, ctx, out),
        AssignmentKind::BallCarry => {
            *role = RoleState::Carrying;
            carry_candidates(player, ctx, out);
        }
        _ => {}
    }
}

/// The quarterback: drop back, scan, wind up on command, or take off if he has
/// committed to running. The simulation owns the actual release.
fn quarterback(
    player: &PlayerSim,
    drop_to: Vec3,
    role: &mut RoleState,
    ctx: &BrainCtx<'_>,
    out: &mut Vec<ScoredAction>,
) {
    let holds_ball = ctx.possession == Some(player.id);
    match *role {
        RoleState::QbWindup { .. } => {
            out.push(ScoredAction::new(
                PlayerIntent::Throw,
                Priority::BallThreat,
                1.0,
                "throw",
                6,
            ));
        }
        RoleState::QbDone => out.push(ScoredAction::new(
            PlayerIntent::Hold,
            Priority::Assignment,
            0.0,
            "thrown",
            1,
        )),
        _ => {
            if holds_ball && ctx.throw_commanded {
                *role = RoleState::QbWindup { since: ctx.tick };
                out.push(ScoredAction::new(
                    PlayerIntent::Throw,
                    Priority::BallThreat,
                    1.0,
                    "windup",
                    6,
                ));
                return;
            }
            if ctx.per.qb_committed_to_run && holds_ball {
                out.push(ScoredAction::new(
                    PlayerIntent::Carry {
                        point: OffenseFrame::clamp_in_bounds(ctx.end_zone_target, ctx.tuning.bounds_margin),
                    },
                    Priority::BallThreat,
                    0.9,
                    "scramble",
                    8,
                ));
            }
            let to_drop = drop_to.subtract(player.pos);
            let far = Vec3::new(to_drop.x, 0.0, to_drop.z).length() > 0.5;
            if far && holds_ball {
                *role = RoleState::QbDrop;
                out.push(ScoredAction::new(
                    PlayerIntent::DropBack {
                        point: drop_to,
                        face: ctx.end_zone_target.subtract(player.pos),
                        sprint: false,
                    },
                    Priority::Assignment,
                    0.6,
                    "drop",
                    4,
                ));
            } else if holds_ball {
                *role = RoleState::QbScan;
                out.push(ScoredAction::new(
                    PlayerIntent::Face {
                        direction: ctx.end_zone_target.subtract(player.pos),
                    },
                    Priority::Assignment,
                    0.4,
                    "scan",
                    2,
                ));
            } else {
                out.push(ScoredAction::new(
                    PlayerIntent::Hold,
                    Priority::Assignment,
                    0.0,
                    "await-snap",
                    1,
                ));
            }
        }
    }
}

/// Route runner: break to a live pass thrown to us, otherwise run the route.
fn route_runner(
    player: &PlayerSim,
    assignment: &ResolvedAssignment,
    role: &mut RoleState,
    ctx: &BrainCtx<'_>,
    out: &mut Vec<ScoredAction>,
) {
    // A live pass intended for us is the priority — adjust to the catch point.
    if let BallState::Airborne { flight } = ctx.ball.state {
        if flight.intended == player.id {
            *role = RoleState::CatchWork;
            out.push(ScoredAction::new(
                PlayerIntent::PrepareCatch { point: flight.target },
                Priority::BallThreat,
                1.0,
                "adjust-catch",
                4,
            ));
            return;
        }
    }
    let index = match *role {
        RoleState::Route { index } => index,
        RoleState::CatchWork => {
            out.push(ScoredAction::new(
                PlayerIntent::Hold,
                Priority::Leverage,
                0.1,
                "pass-gone",
                2,
            ));
            return;
        }
        RoleState::RouteDone => {
            out.push(ScoredAction::new(
                PlayerIntent::Face {
                    direction: ctx.end_zone_target.subtract(player.pos).mul_scalar(-1.0),
                },
                Priority::Leverage,
                0.2,
                "work-back",
                2,
            ));
            return;
        }
        _ => {
            *role = RoleState::Route { index: 0 };
            0
        }
    };
    match assignment.route.get(index) {
        None => {
            *role = RoleState::RouteDone;
            out.push(ScoredAction::new(
                PlayerIntent::Hold,
                Priority::Leverage,
                0.1,
                "route-done",
                2,
            ));
        }
        Some(&waypoint) => {
            let flat = Vec3::new(waypoint.x - player.pos.x, 0.0, waypoint.z - player.pos.z);
            if flat.length() < WAYPOINT_RANGE {
                *role = RoleState::Route { index: index + 1 };
            }
            out.push(ScoredAction::new(
                PlayerIntent::MoveToward { point: waypoint, sprint: true },
                Priority::Assignment,
                0.6,
                "route",
                3,
            ));
        }
    }
}

/// Carry the ball: run for the end zone, drifting toward the middle third.
pub fn carry_candidates(player: &PlayerSim, ctx: &BrainCtx<'_>, out: &mut Vec<ScoredAction>) {
    let target = Vec3::new(
        player.pos.x * 0.6 + ctx.end_zone_target.x * 0.4,
        0.0,
        ctx.end_zone_target.z,
    );
    out.push(ScoredAction::new(
        PlayerIntent::Carry {
            point: OffenseFrame::clamp_in_bounds(target, ctx.tuning.bounds_margin),
        },
        Priority::BallThreat,
        0.9,
        "carry",
        6,
    ));
}

/// Every player scrambles for a loose ball they are close to.
fn loose_ball_candidate(player: &PlayerSim, ctx: &BrainCtx<'_>, out: &mut Vec<ScoredAction>) {
    if !ctx.per.situation.is_loose() {
        return;
    }
    let distance = flat(ctx.per.ball_pos.subtract(player.pos)).length();
    if distance < LOOSE_ALERT {
        let urgency = (1.0 - distance / LOOSE_ALERT).clamp(0.2, 1.0);
        out.push(ScoredAction::new(
            PlayerIntent::MoveToward { point: ctx.per.ball_pos, sprint: true },
            Priority::BallThreat,
            urgency,
            "loose-ball",
            4,
        ));
    }
}

fn flat(v: Vec3) -> Vec3 {
    Vec3::new(v.x, 0.0, v.z)
}
