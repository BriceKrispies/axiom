//! Personnel matching for the overseer: given a chosen tactical mode, pick which
//! defenders fill its assignment overrides (spy / blitz / bracket / contain) by
//! suitability — position and distance to the target. Deterministic, with
//! ascending-id tie-breaks so a replay stamps the same personnel.

use axiom::prelude::Vec3;

use crate::identity::PlayerId;
use crate::player::PlayerSim;

use super::directive::{AssignmentOverride, DefensiveDirective, SecondaryAdjustment, TacticalMode};
use super::perception::PlayPerception;

/// Fill `d.overrides` from the directive's mode + secondary adjustment.
pub fn assign_overrides(d: &mut DefensiveDirective, per: &PlayPerception, players: &[PlayerSim]) {
    let defenders: Vec<PlayerId> = players
        .iter()
        .filter(|p| p.team != per.offense_team && p.anim.can_act())
        .map(|p| p.id)
        .collect();
    let pos = |id: PlayerId| players[id.index()].pos;

    match d.mode {
        TacticalMode::IncreasePressure => {
            // The second-nearest defender to the QB comes on a delayed blitz
            // (the nearest is already the primary rusher).
            let mut by_qb = defenders.clone();
            by_qb.sort_by(|a, b| dist(pos(*a), per.qb_pos).total_cmp(&dist(pos(*b), per.qb_pos)));
            if let Some(&id) = by_qb.get(1) {
                d.overrides[id.index()] = AssignmentOverride::Blitz;
            }
        }
        TacticalMode::ContainQb => {
            assign_spy(d, &defenders, per, players);
            for id in widest_two(&defenders, players) {
                d.overrides[id.index()] = AssignmentOverride::ContainEdge;
            }
        }
        TacticalMode::BracketReceiver => {
            if let Some(target) = d.primary_threat {
                let tpos = pos(target);
                let mut by_target = defenders.clone();
                by_target.sort_by(|a, b| dist(pos(*a), tpos).total_cmp(&dist(pos(*b), tpos)));
                if let Some(&primary) = by_target.first() {
                    d.overrides[primary.index()] = AssignmentOverride::BracketPrimary;
                }
                if let Some(&help) = by_target.get(1) {
                    d.overrides[help.index()] = AssignmentOverride::BracketHelp;
                }
            }
        }
        _ => {}
    }
    if d.secondary == SecondaryAdjustment::SpyQb
        && !d.overrides.iter().any(|o| *o == AssignmentOverride::Spy)
    {
        assign_spy(d, &defenders, per, players);
    }
}

/// Spy the quarterback with the interior defender nearest his throwing lane.
fn assign_spy(
    d: &mut DefensiveDirective,
    defenders: &[PlayerId],
    per: &PlayPerception,
    players: &[PlayerSim],
) {
    let spy = defenders
        .iter()
        .filter(|id| d.overrides[id.index()] == AssignmentOverride::None)
        .min_by(|a, b| {
            spy_key(players[a.index()].pos, per).total_cmp(&spy_key(players[b.index()].pos, per))
        });
    if let Some(&id) = spy {
        d.overrides[id.index()] = AssignmentOverride::Spy;
    }
}

/// Prefer an interior (small |x|) defender in front of the quarterback.
fn spy_key(p: Vec3, per: &PlayPerception) -> f32 {
    p.x.abs() + dist(p, per.qb_pos) * 0.3
}

/// The two defenders furthest to each sideline (the edge-contain pair).
fn widest_two(defenders: &[PlayerId], players: &[PlayerSim]) -> Vec<PlayerId> {
    let mut left: Option<PlayerId> = None;
    let mut right: Option<PlayerId> = None;
    for &id in defenders {
        let x = players[id.index()].pos.x;
        if x < 0.0 && left.map(|l| x < players[l.index()].pos.x).unwrap_or(true) {
            left = Some(id);
        }
        if x >= 0.0 && right.map(|r| x > players[r.index()].pos.x).unwrap_or(true) {
            right = Some(id);
        }
    }
    left.into_iter().chain(right).collect()
}

fn dist(a: Vec3, b: Vec3) -> f32 {
    Vec3::new(a.x - b.x, 0.0, a.z - b.z).length()
}
