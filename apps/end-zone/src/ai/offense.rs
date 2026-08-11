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
// Route running and the carry reads moved to `offense_routes`; callers still
// reach them through `offense::`.
pub use super::offense_routes::carry_candidates;
// The route read, the scramble read and the planar helper moved with them.
use super::offense_routes::{flat, loose_ball_candidate, route_runner};

pub(super) const WAYPOINT_RANGE: f32 = 0.9;
/// A loose ball inside this range is worth chasing, yards.
pub(super) const LOOSE_ALERT: f32 = 14.0;
/// A player this far off his alignment is considered SET, yards. Loose enough
/// that ordinary settling never reads as a shift.
pub const SET_RANGE: f32 = 0.6;

/// The not-live candidate: get to the spot this play wants, or stand still.
///
/// This is what makes the pre-snap picker legible. Choosing a concept swaps the
/// formation under the offense, and rather than teleporting everyone into the
/// new alignment they RUN there — the shift is the feedback that the call took,
/// and finishing it is what triggers the snap.
///
/// `pre_snap` is not `!live`: the whistle is also not-live, and a player who
/// went looking for his alignment on a dead play would drag the ball back
/// upfield after every tackle. After the snap this always holds.
pub(super) fn shift_or_set(
    player: &PlayerSim,
    assignment: &ResolvedAssignment,
    pre_snap: bool,
) -> ScoredAction {
    let flat = Vec3::new(player.pos.x, assignment.align.y, player.pos.z);
    match pre_snap && flat.distance(assignment.align) > SET_RANGE {
        true => ScoredAction::new(
            // At a hustle. The shift is what stands between pressing a play and
            // the snap, so a leisurely walk would turn a responsive call into a
            // two-second wait.
            PlayerIntent::MoveToward {
                point: assignment.align,
                sprint: true,
            },
            Priority::Assignment,
            0.0,
            "shift",
            1,
        ),
        false => ScoredAction::new(PlayerIntent::Hold, Priority::Assignment, 0.0, "set", 1),
    }
}

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
        out.push(shift_or_set(player, assignment, ctx.pre_snap));
        return;
    }
    loose_ball_candidate(player, ctx, out);
    match assignment.kind {
        AssignmentKind::Quarterback { drop_to } => quarterback(player, drop_to, role, ctx, out),
        AssignmentKind::HandOff { back, mesh } => {
            handoff_quarterback(player, back, mesh, role, ctx, out)
        }
        AssignmentKind::RunBack { mesh, .. } => run_back(mesh, role, out),
        AssignmentKind::Snapper | AssignmentKind::PassBlock => {
            super::protection::pass_block(player, role, ctx, out)
        }
        AssignmentKind::LeadBlock => super::protection::lead_block(player, role, ctx, out),
        AssignmentKind::Route { .. } => route_runner(player, assignment, role, ctx, out),
        AssignmentKind::BallCarry => {
            *role = RoleState::Carrying;
            carry_candidates(player, assignment, ctx, out);
        }
        _ => {}
    }
}

/// The run game's quarterback: take the snap, open to the mesh, and give it up.
///
/// He never decides to hand off — that is a *fact about the field* the attempt
/// loop reads (are the two of them actually together yet), exactly as the snap
/// is a fact about the offense being set. All he does here is get to the meeting
/// point and keep his eyes on the back so the exchange is something the player
/// can watch happen.
fn handoff_quarterback(
    player: &PlayerSim,
    back: crate::identity::PlayerId,
    mesh: Vec3,
    role: &mut RoleState,
    ctx: &BrainCtx<'_>,
    out: &mut Vec<ScoredAction>,
) {
    let holds_ball = ctx.possession == Some(player.id);
    // Once the ball is gone he is a bystander who must not wander back into the
    // hole his own back is running through.
    if !holds_ball {
        *role = RoleState::QbDone;
        out.push(ScoredAction::new(
            PlayerIntent::Face {
                direction: ctx.players[back.index()].pos.subtract(player.pos),
            },
            Priority::Assignment,
            0.2,
            "handed-off",
            2,
        ));
        return;
    }
    *role = RoleState::QbDrop;
    let to_mesh = flat(mesh.subtract(player.pos));
    // Turn and carry the ball to the meeting point, facing the man taking it.
    // `DropBack` rather than `MoveToward` because the whole point of the open
    // step is that he does NOT turn to face where he is going — his body opens
    // to the back while his feet take him to the spot.
    out.push(ScoredAction::new(
        PlayerIntent::DropBack {
            point: match to_mesh.length() > 0.35 {
                true => mesh,
                false => player.pos,
            },
            face: ctx.players[back.index()].pos.subtract(player.pos),
            sprint: false,
        },
        Priority::Assignment,
        0.7,
        "mesh",
        4,
    ));
}

/// The running back before the exchange: get to the mesh point, on time.
///
/// After it he is the carrier, and [`crate::ai::brain::decide`] routes every
/// carrier — this one included — through [`carry_candidates`]. So this function
/// is only ever the *approach*, which is why it is three lines and not a state
/// machine.
fn run_back(mesh: Vec3, role: &mut RoleState, out: &mut Vec<ScoredAction>) {
    *role = RoleState::Route { index: 0 };
    out.push(ScoredAction::new(
        PlayerIntent::MoveToward {
            point: mesh,
            // Not a sprint: he has three yards to cover and has to arrive under
            // control, or he runs through the exchange and the mesh gate refuses
            // a handoff that never happened.
            sprint: false,
        },
        Priority::Assignment,
        0.8,
        "mesh",
        4,
    ));
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
