//! Defensive candidate generators. A defender weighs four kinds of action on
//! one shared scale: a loose ball, a ball in flight (predictive — intercept /
//! contest / tackle-angle by coordinated responsibility), a live ground runner
//! (rallying through his responsibility so nobody duplicates an angle), and his
//! base coverage/rush assignment as the fallback. Opponent geometry is read
//! from the DELAYED perception ring at his own reaction delay; the shared
//! situation + responsibilities come from [`crate::ai::PlayPerception`].

use axiom::prelude::Vec3;

use crate::identity::PlayerId;
use crate::player::PlayerSim;

use super::action::{Priority, ScoredAction};
use super::assignment::{AssignmentKind, ResolvedAssignment};
use super::brain::{BrainCtx, PerceptionFrame, RoleState};
use super::engagement;
use super::perception::Responsibility;
use super::{steering, PlayerIntent};

/// A loose ball inside this range is worth chasing, yards.
const LOOSE_ALERT: f32 = 16.0;

/// Push a defensive player's candidate actions.
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
    *role = RoleState::Defending;
    let seen = ctx.perception.sample(player.archetype.reaction_delay_ticks);
    let resp = ctx.per.responsibility(player.id);

    loose_ball(player, ctx, out);
    if ctx.per.situation.ball_in_air() {
        airborne(resp, ctx, seen, out);
    }
    runner(player, resp, ctx, seen, out);
    base(player, assignment, seen, ctx, out);
}

/// React to a live ground carrier through this defender's responsibility. Gated
/// by the DELAYED ring: a defender only rallies once his own perception shows
/// the runner holding the ball — so reaction delay is preserved.
fn runner(
    player: &PlayerSim,
    resp: Responsibility,
    ctx: &BrainCtx<'_>,
    seen: &PerceptionFrame,
    out: &mut Vec<ScoredAction>,
) {
    let Some(target) = ctx.per.ground_threat else {
        return;
    };
    if seen.carrier != Some(target) {
        return;
    }
    let seen_pos = seen.positions[target.index()];
    let seen_vel = seen.velocities[target.index()];
    let side = escape_side(seen_pos, seen_vel);
    let dist = flat_dist(player.pos, seen_pos);
    let lead = steering::pursuit_lead_seconds(dist, &player.archetype);
    let aim = steering::predict(seen_pos, seen_vel, lead);
    // Every defender pursues the carrier, but his responsibility biases the
    // ANGLE he takes so the team converges from different leverages instead of
    // stacking one pursuit path (spec §4, §8) — nobody hangs back off the ball.
    let (point, urgency, reason) = match resp {
        Responsibility::PrimaryTackler => (aim, primary_urgency(player, seen_pos, ctx), "attack-runner"),
        Responsibility::OutsideContain => (
            Vec3::new(aim.x + side * 2.0, 0.0, aim.z),
            0.85,
            "contain",
        ),
        Responsibility::Cutback => (
            Vec3::new(aim.x - side * 2.0, 0.0, aim.z),
            0.8,
            "cutback",
        ),
        // The last line of defense meets the carrier further downfield (a longer
        // lead) so he is goal-side, then attacks.
        Responsibility::DeepHelp => (
            steering::predict(seen_pos, seen_vel, lead * 1.6),
            0.8,
            "deep-help",
        ),
        _ => (aim, 0.6, "backside"),
    };
    pursue_or_tackle(player, target, seen_pos, point, Priority::BallThreat, urgency, reason, ctx, out);
}

/// React to a ball in flight predictively (spec §7), by responsibility.
fn airborne(
    resp: Responsibility,
    ctx: &BrainCtx<'_>,
    seen: &PerceptionFrame,
    out: &mut Vec<ScoredAction>,
) {
    let Some(catch_point) = ctx.per.catch_point else {
        return;
    };
    match resp {
        Responsibility::Intercept => out.push(ScoredAction::new(
            PlayerIntent::PrepareCatch { point: catch_point },
            Priority::BallThreat,
            1.0,
            "intercept",
            5,
        )),
        Responsibility::ContestCatch => out.push(ScoredAction::new(
            PlayerIntent::PrepareCatch { point: catch_point },
            Priority::BallThreat,
            0.75,
            "contest",
            5,
        )),
        Responsibility::TackleAngle => {
            if let Some(receiver) = ctx.per.intended_receiver {
                let rp = seen.positions[receiver.index()];
                out.push(move_to(
                    Vec3::new(rp.x, 0.0, rp.z + ctx.per.drive_sign * 3.0),
                    Priority::PreventScore,
                    0.65,
                    "tackle-angle",
                ));
            }
        }
        // Not on this ball: rally goal-side and keep the catch in front.
        _ => {
            let to_goal = flat(ctx.per.end_zone.subtract(catch_point))
                .normalize()
                .unwrap_or(Vec3::UNIT_Z)
                .mul_scalar(ctx.tuning.pursuit_cushion);
            out.push(move_to(catch_point.add(to_goal), Priority::Leverage, 0.4, "over-top"));
        }
    }
}

/// The base coverage / rush assignment — the fallback the shared reactions
/// outrank when the ball is live in the air or on the ground.
fn base(
    player: &PlayerSim,
    assignment: &ResolvedAssignment,
    seen: &PerceptionFrame,
    ctx: &BrainCtx<'_>,
    out: &mut Vec<ScoredAction>,
) {
    match assignment.kind {
        AssignmentKind::ManCover { target } => {
            let mp = seen.positions[target.index()];
            let mv = seen.velocities[target.index()];
            let lead = steering::pursuit_lead_seconds(flat_dist(player.pos, mp), &player.archetype);
            out.push(ScoredAction::new(
                PlayerIntent::MoveToward { point: steering::predict(mp, mv, lead), sprint: true },
                Priority::Assignment,
                0.5,
                "man-cover",
                3,
            ));
        }
        AssignmentKind::ZoneCover { center, .. } => {
            out.push(move_to(center, Priority::Assignment, 0.4, "zone"));
        }
        AssignmentKind::QuarterbackRush { quarterback } => {
            let qbp = seen.positions[quarterback.index()];
            let qbv = seen.velocities[quarterback.index()];
            let lead = steering::pursuit_lead_seconds(flat_dist(player.pos, qbp), &player.archetype);
            let free = engagement::rusher_is_free(ctx.engagements, player.id);
            let urgency = if free { 0.8 } else { 0.55 };
            let reason = if free { "pressure" } else { "rush" };
            pursue_or_tackle(
                player,
                quarterback,
                qbp,
                steering::predict(qbp, qbv, lead),
                Priority::Assignment,
                urgency,
                reason,
                ctx,
                out,
            );
        }
        AssignmentKind::EdgeContain { post, .. } => {
            out.push(move_to(post, Priority::Assignment, 0.4, "edge"));
        }
        AssignmentKind::Pursuit | AssignmentKind::TackleTarget => {
            out.push(move_to(
                Vec3::new(seen.ball_pos.x * 0.5, 0.0, player.pos.z),
                Priority::Leverage,
                0.3,
                "patrol",
            ));
        }
        _ => {}
    }
}

/// Scramble for a loose ball this defender is close to.
fn loose_ball(player: &PlayerSim, ctx: &BrainCtx<'_>, out: &mut Vec<ScoredAction>) {
    if !ctx.per.situation.is_loose() {
        return;
    }
    let distance = flat_dist(player.pos, ctx.per.ball_pos);
    if distance < LOOSE_ALERT {
        out.push(ScoredAction::new(
            PlayerIntent::MoveToward { point: ctx.per.ball_pos, sprint: true },
            Priority::BallThreat,
            (1.0 - distance / LOOSE_ALERT).clamp(0.2, 1.0),
            "loose-ball",
            4,
        ));
    }
}

/// Close on a carrier: tackle when inside range of him, else pursue toward the
/// given (leverage-biased) point.
fn pursue_or_tackle(
    player: &PlayerSim,
    target: PlayerId,
    runner_pos: Vec3,
    pursue_point: Vec3,
    priority: Priority,
    urgency: f32,
    reason: &'static str,
    ctx: &BrainCtx<'_>,
    out: &mut Vec<ScoredAction>,
) {
    if flat_dist(player.pos, runner_pos) <= ctx.tuning.tackle_range * 2.0 {
        out.push(ScoredAction::new(
            PlayerIntent::Tackle { target, point: runner_pos },
            priority,
            urgency.max(0.9),
            reason,
            3,
        ));
    } else {
        out.push(ScoredAction::new(
            PlayerIntent::Pursue { target, point: pursue_point },
            priority,
            urgency,
            reason,
            4,
        ));
    }
}

/// Urgency to attack a runner: proximity plus how near the end zone he is.
fn primary_urgency(player: &PlayerSim, runner_pos: Vec3, ctx: &BrainCtx<'_>) -> f32 {
    let prox = (1.0 - flat_dist(player.pos, runner_pos) / 30.0).clamp(0.0, 1.0);
    let goal = (1.0 - (ctx.per.end_zone.z - runner_pos.z).abs() / 60.0).clamp(0.0, 1.0);
    (0.6 + 0.25 * prox + 0.15 * goal).clamp(0.0, 1.0)
}

fn escape_side(pos: Vec3, vel: Vec3) -> f32 {
    if vel.x.abs() > 0.5 {
        vel.x.signum()
    } else {
        pos.x.signum()
    }
}

fn move_to(point: Vec3, priority: Priority, urgency: f32, reason: &'static str) -> ScoredAction {
    ScoredAction::new(PlayerIntent::MoveToward { point, sprint: true }, priority, urgency, reason, 3)
}

fn flat(v: Vec3) -> Vec3 {
    Vec3::new(v.x, 0.0, v.z)
}

fn flat_dist(a: Vec3, b: Vec3) -> f32 {
    flat(a.subtract(b)).length()
}
