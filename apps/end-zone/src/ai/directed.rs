//! How an individual defender executes the overseer's per-player assignment
//! overrides. The overseer only *names* the responsibility (spy / blitz /
//! bracket / contain); here that becomes scored candidate actions the defender's
//! own arbitration weighs — never a steering command. Coverage overrides sit in
//! the Assignment band, so a thrown ball's ball-threat reactions still take over.

use axiom::prelude::Vec3;

use crate::identity::PlayerId;
use crate::player::PlayerSim;

use super::action::{Priority, ScoredAction};
use super::brain::{BrainCtx, PerceptionFrame};
use super::defense::{flat_dist, pursue_or_tackle};
use super::directive::AssignmentOverride;
use super::{steering, PlayerIntent};

/// Push the candidate actions implied by this defender's directive override.
pub fn override_candidates(
    player: &PlayerSim,
    ov: AssignmentOverride,
    ctx: &BrainCtx<'_>,
    seen: &PerceptionFrame,
    out: &mut Vec<ScoredAction>,
) {
    let per = ctx.per;
    let qb = per.quarterback;
    match ov {
        AssignmentOverride::None => {}
        AssignmentOverride::Blitz => {
            let qbp = seen.positions[qb.index()];
            let qbv = seen.velocities[qb.index()];
            let urgency = (0.72 + per.directive.rush_emphasis * 0.2).clamp(0.4, 0.95);
            rush(player, qbp, qbv, Priority::Assignment, urgency, "blitz", ctx, out);
        }
        AssignmentOverride::Spy => {
            let qbp = seen.positions[qb.index()];
            if per.qb_in_pocket {
                // Mirror the quarterback at the line; attack only once he breaks
                // the pocket.
                let mirror = Vec3::new(qbp.x * 0.6, 0.0, per.pocket.los_z);
                out.push(ScoredAction::new(
                    PlayerIntent::MoveToward { point: mirror, sprint: false },
                    Priority::Assignment,
                    0.5,
                    "spy",
                    6,
                ));
            } else {
                let qbv = seen.velocities[qb.index()];
                rush(player, qbp, qbv, Priority::BallThreat, 0.8, "spy-attack", ctx, out);
            }
        }
        AssignmentOverride::BracketPrimary => {
            cover(player, per.directive.primary_threat, seen, 0.0, "bracket", out)
        }
        AssignmentOverride::BracketHelp => cover(
            player,
            per.directive.primary_threat,
            seen,
            per.drive_sign * 2.5,
            "help",
            out,
        ),
        AssignmentOverride::ContainEdge => {
            let side = if player.pos.x.abs() < 0.5 {
                per.directive.shade_side
            } else {
                player.pos.x.signum()
            };
            let post = Vec3::new(
                side * (per.pocket.half_width + 1.5),
                0.0,
                per.pocket.los_z + per.drive_sign * 1.0,
            );
            out.push(ScoredAction::new(
                PlayerIntent::MoveToward { point: post, sprint: false },
                Priority::Assignment,
                0.5,
                "contain",
                8,
            ));
        }
    }
}

/// Rush the quarterback (delegates to the shared close-or-tackle helper).
fn rush(
    player: &PlayerSim,
    qbp: Vec3,
    qbv: Vec3,
    priority: Priority,
    urgency: f32,
    reason: &'static str,
    ctx: &BrainCtx<'_>,
    out: &mut Vec<ScoredAction>,
) {
    let lead = steering::pursuit_lead_seconds(flat_dist(player.pos, qbp), &player.archetype);
    let point = steering::predict(qbp, qbv, lead);
    pursue_or_tackle(player, ctx.quarterback, qbp, point, priority, urgency, reason, ctx, out);
}

/// Man-cover a bracketed receiver, optionally sitting `depth_bias` over the top.
fn cover(
    player: &PlayerSim,
    target: Option<PlayerId>,
    seen: &PerceptionFrame,
    depth_bias: f32,
    reason: &'static str,
    out: &mut Vec<ScoredAction>,
) {
    let Some(t) = target else {
        return;
    };
    let tp = seen.positions[t.index()];
    let tv = seen.velocities[t.index()];
    let lead = steering::pursuit_lead_seconds(flat_dist(player.pos, tp), &player.archetype);
    let mut point = steering::predict(tp, tv, lead);
    point.z += depth_bias;
    out.push(ScoredAction::new(
        PlayerIntent::MoveToward { point, sprint: true },
        Priority::Assignment,
        0.6,
        reason,
        4,
    ));
}
