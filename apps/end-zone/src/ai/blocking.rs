//! **Blocking assignment**: which blocker has which rusher.
//!
//! The offensive counterpart of [`super::coordination`], and it existed on the
//! defensive side from the beginning while the offense went without. Every
//! blocker independently picked *the nearest opponent*, which is a rule with an
//! obvious failure mode and a measured cost: three linemen converge on the one
//! man who happens to be closest to all of them, the other rushers walk into the
//! backfield untouched, and the back is tackled before he has taken a step.
//! Over 200 benched carries that was **45% of them lost yardage**, which is not
//! a difficulty setting, it is a defense playing four-on-one.
//!
//! The fix is the same shape as the defensive one: a stateless geometric pass
//! that hands out distinct duties. Nobody gets double-teamed while somebody else
//! runs free.
//!
//! ## The order matters
//!
//! Threats are claimed **most dangerous first** — nearest to the man being
//! protected — and each claims the nearest blocker still free. Doing it the
//! other way round (each blocker takes his nearest threat) is what produced the
//! pile-up: a blocker's own nearest man is a fact about the blocker, not about
//! the play, and several blockers can share one. Starting from the *threat* side
//! means the rusher who is about to make the tackle is the first to be answered,
//! and the man nobody can reach is the one left over — which is the right thing
//! to leave over.

use axiom::prelude::Vec3;

use crate::config::PLAYER_COUNT;
use crate::identity::PlayerId;
use crate::player::PlayerSim;

/// One blocker's assigned man, indexed by [`PlayerId`]. `None` for anyone who is
/// not blocking, and for a blocker with nobody left to take.
pub type BlockAssignments = [Option<PlayerId>; PLAYER_COUNT];

fn flat_distance(a: Vec3, b: Vec3) -> f32 {
    Vec3::new(a.x - b.x, 0.0, a.z - b.z).length()
}

/// Hand each blocker a distinct rusher.
///
/// `blockers` and `threats` are already filtered to the players who can do each
/// job; `protect` is the point being defended (the ball carrier, or the ball).
/// Deterministic throughout: both lists are consumed in ascending id order after
/// a stable sort, so the same field always produces the same pairing.
pub fn assign_blocks(
    blockers: &[PlayerId],
    threats: &[PlayerId],
    players: &[PlayerSim],
    protect: Vec3,
) -> BlockAssignments {
    let mut assigned: BlockAssignments = [None; PLAYER_COUNT];

    // Most dangerous first: the rusher nearest the man we are protecting.
    let mut ranked: Vec<(PlayerId, f32)> = threats
        .iter()
        .map(|id| (*id, flat_distance(players[id.index()].pos, protect)))
        .collect();
    ranked.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0 .0.cmp(&b.0 .0)));

    let mut free: Vec<PlayerId> = blockers.to_vec();
    for (threat, _) in ranked {
        let threat_pos = players[threat.index()].pos;
        let pick = free
            .iter()
            .enumerate()
            .map(|(index, id)| {
                (
                    index,
                    *id,
                    flat_distance(players[id.index()].pos, threat_pos),
                )
            })
            .fold(None::<(usize, PlayerId, f32)>, |best, candidate| {
                let better = best
                    .map(|(_, id, d)| {
                        candidate.2 < d || (candidate.2 == d && candidate.1 .0 < id.0)
                    })
                    .unwrap_or(true);
                match better {
                    true => Some(candidate),
                    false => best,
                }
            });
        let Some((index, blocker, _)) = pick else {
            // Out of blockers: every remaining rusher is unblocked, which is the
            // honest outcome of being outnumbered rather than a bug to paper
            // over. It is also what the back's own moves exist to answer.
            break;
        };
        assigned[blocker.index()] = Some(threat);
        free.remove(index);
    }
    assigned
}
