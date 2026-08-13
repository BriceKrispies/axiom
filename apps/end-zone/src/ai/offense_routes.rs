//! Route running, carry candidates and the loose-ball scramble.
//!
//! Split out of [`super::offense`], which owns the quarterback and hand-off
//! reads, so each file stays narrowly owned. Pure relocation.

use axiom::prelude::Vec3;
use super::offense::{LOOSE_ALERT, WAYPOINT_RANGE};
use crate::field::OffenseFrame;
use crate::football::BallState;
use crate::player::PlayerSim;
use super::action::{Priority, ScoredAction};
use super::assignment::{AssignmentKind, ResolvedAssignment};
use super::brain::{BrainCtx, RoleState};
use super::PlayerIntent;

/// Route runner: break to a live pass thrown to us, otherwise run the route.
pub(super) fn route_runner(
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

/// Carry the ball: run downfield through the best lane the field offers,
/// attacking the play's designed hole while it is still ahead.
///
/// This is the **automatic forward run**. It is the same candidate the AI has
/// always produced for a carrier — the player's three moves modify what it does,
/// they never replace it, which is why nothing here knows the runner is human.
/// The heading itself is [`super::carry::carry_point`].
pub fn carry_candidates(
    player: &PlayerSim,
    assignment: &ResolvedAssignment,
    ctx: &BrainCtx<'_>,
    out: &mut Vec<ScoredAction>,
) {
    let aim = match assignment.kind {
        AssignmentKind::RunBack { aim, .. } => Some(aim),
        _ => None,
    };
    out.push(ScoredAction::new(
        PlayerIntent::Carry {
            point: super::carry::carry_point(player, aim, ctx),
        },
        Priority::BallThreat,
        0.9,
        "carry",
        6,
    ));
}

/// Every player scrambles for a loose ball they are close to.
pub(super) fn loose_ball_candidate(player: &PlayerSim, ctx: &BrainCtx<'_>, out: &mut Vec<ScoredAction>) {
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

pub(super) fn flat(v: Vec3) -> Vec3 {
    Vec3::new(v.x, 0.0, v.z)
}
