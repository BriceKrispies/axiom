//! Offensive-line protection candidates: an offensive blocker walls the nearest
//! rusher off the pocket by planting a leverage point protect-side of him and
//! squaring his body to him. This is the *decision* half of a block; the
//! physical contest it sets up is resolved in [`super::engagement`].

use axiom::prelude::Vec3;

use crate::identity::PlayerId;
use crate::player::PlayerSim;

use super::action::{Priority, ScoredAction};
use super::brain::{BrainCtx, RoleState};
use super::PlayerIntent;

/// How far protect-side of the rusher a blocker plants his leverage point.
const BLOCK_STAND: f32 = 0.7;

/// Pass protection: wall the nearest threat off the pocket, squared to him.
pub fn pass_block(
    player: &PlayerSim,
    role: &mut RoleState,
    ctx: &BrainCtx<'_>,
    out: &mut Vec<ScoredAction>,
) {
    *role = RoleState::Blocking;
    let protect = ctx
        .possession
        .map(|id| ctx.players[id.index()].pos)
        .unwrap_or(player.pos);
    if let Some((threat, threat_pos)) = nearest_opponent(player, ctx) {
        out.push(block_action(player, threat, threat_pos, protect, "pass-block"));
    } else {
        out.push(ScoredAction::new(
            PlayerIntent::Hold,
            Priority::Assignment,
            0.2,
            "no-threat",
            2,
        ));
    }
}

/// Lead blocking: wall the nearest threat downfield of the carrier.
pub fn lead_block(
    player: &PlayerSim,
    role: &mut RoleState,
    ctx: &BrainCtx<'_>,
    out: &mut Vec<ScoredAction>,
) {
    *role = RoleState::Blocking;
    let protect = ctx
        .possession
        .map(|id| ctx.players[id.index()].pos)
        .unwrap_or(ctx.end_zone_target);
    match nearest_opponent(player, ctx) {
        Some((threat, threat_pos)) => {
            out.push(block_action(player, threat, threat_pos, protect, "lead-block"))
        }
        None => out.push(ScoredAction::new(
            PlayerIntent::MoveToward {
                point: ctx.end_zone_target,
                sprint: false,
            },
            Priority::Assignment,
            0.4,
            "lead-up",
            3,
        )),
    }
}

/// A squared block: plant a leverage point protect-side of the rusher and face
/// him, so the blocker walls and anchors instead of chasing a midpoint.
fn block_action(
    player: &PlayerSim,
    threat: PlayerId,
    threat_pos: Vec3,
    protect: Vec3,
    reason: &'static str,
) -> ScoredAction {
    let to_protect = flat(protect.subtract(threat_pos))
        .normalize()
        .unwrap_or_else(|_| flat(protect.subtract(player.pos)).normalize().unwrap_or(Vec3::UNIT_Z));
    let point = threat_pos.add(to_protect.mul_scalar(BLOCK_STAND));
    let face = flat(threat_pos.subtract(player.pos));
    ScoredAction::new(
        PlayerIntent::Block {
            target: threat,
            point,
            face,
        },
        Priority::Assignment,
        0.7,
        reason,
        5,
    )
}

/// The nearest standing opponent (true positions — offense reads the field
/// honestly; only defenders are perception-delayed).
fn nearest_opponent(player: &PlayerSim, ctx: &BrainCtx<'_>) -> Option<(PlayerId, Vec3)> {
    let mut best: Option<(PlayerId, Vec3, f32)> = None;
    for other in ctx.players {
        if other.team == player.team || !other.anim.can_act() {
            continue;
        }
        let d = flat(other.pos.subtract(player.pos)).length();
        let closer = best.map(|(_, _, bd)| d < bd).unwrap_or(true);
        if closer {
            best = Some((other.id, other.pos, d));
        }
    }
    best.map(|(id, pos, _)| (id, pos))
}

fn flat(v: Vec3) -> Vec3 {
    Vec3::new(v.x, 0.0, v.z)
}
