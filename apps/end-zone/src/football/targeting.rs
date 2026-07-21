//! Throw targeting: the cone in front of the quarterback, who is eligible to
//! receive inside it, and which of them the pass goes to.
//!
//! This replaces the old hardcoded `throw_to` roster slot baked into the play
//! definition — the reason every snap threw to the same receiver. Targeting is
//! now resolved from the live field: the quarterback's facing defines a cone,
//! every upright route-runner inside it is eligible, and the throw goes to
//! whichever eligible receiver sits closest to the cone's centre line. Because
//! the player steers the quarterback, **the stick aims the pass** — turning to
//! face a different receiver changes who the ball goes to, with no extra input.
//!
//! Everything here is a pure function of simulation state: same field, same
//! answer. Candidates are produced in a deterministic order (by angle, then by
//! player id) so a replay can never pick a different receiver.

use axiom::prelude::Vec3;

use crate::ai::{AssignmentKind, ResolvedAssignment};
use crate::data::BehaviorTuning;
use crate::identity::PlayerId;
use crate::player::PlayerSim;

/// One receiver the quarterback may legally throw to this tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThrowCandidate {
    pub id: PlayerId,
    /// Absolute angle off the quarterback's facing, radians. Smaller is more
    /// directly in front — this is what the pick minimizes.
    pub angle: f32,
    /// Planar distance from the quarterback, yards.
    pub distance: f32,
}

/// Whether an assignment makes a player an eligible receiver. Only route
/// runners are — the snapper, the pass blockers and the quarterback himself
/// are ineligible, exactly as in the real game. A decoy route still counts:
/// he is running a route, so he can be thrown to.
fn is_receiver(assignment: &ResolvedAssignment) -> bool {
    matches!(assignment.kind, AssignmentKind::Route { .. })
}

/// The absolute angle between `facing` and the direction from `from` to `to`,
/// in radians. `None` when the two points are (near) coincident, which has no
/// meaningful direction.
fn angle_off_facing(from: Vec3, facing: f32, to: Vec3) -> Option<f32> {
    let forward = Vec3::new(facing.sin(), 0.0, facing.cos());
    let offset = Vec3::new(to.x - from.x, 0.0, to.z - from.z);
    offset
        .normalize()
        .ok()
        .map(|dir| forward.dot(dir).clamp(-1.0, 1.0).acos())
}

/// Every receiver inside the quarterback's throwing cone this tick, ordered by
/// how directly in front of him they are (nearest the centre line first, ties
/// broken by player id so the order is total and replay-stable).
pub fn candidates(
    quarterback: &PlayerSim,
    players: &[PlayerSim],
    assignments: &[ResolvedAssignment],
    tuning: &BehaviorTuning,
) -> Vec<ThrowCandidate> {
    let mut out: Vec<ThrowCandidate> = players
        .iter()
        .enumerate()
        .filter(|(index, p)| {
            p.id != quarterback.id
                && p.team == quarterback.team
                && !p.anim.is_down()
                && assignments.get(*index).is_some_and(is_receiver)
        })
        .filter_map(|(_, p)| {
            let angle = angle_off_facing(quarterback.pos, quarterback.facing, p.pos)?;
            let distance = Vec3::new(p.pos.x - quarterback.pos.x, 0.0, p.pos.z - quarterback.pos.z)
                .length();
            let in_cone = angle <= tuning.throw_cone_half_angle;
            let in_range = distance >= tuning.throw_min_range && distance <= tuning.throw_max_range;
            (in_cone && in_range).then_some(ThrowCandidate {
                id: p.id,
                angle,
                distance,
            })
        })
        .collect();
    out.sort_by(|a, b| a.angle.total_cmp(&b.angle).then(a.id.0.cmp(&b.id.0)));
    out
}

/// The receiver the pass goes to: the one closest to the cone's centre line.
/// `None` when nobody is open, in which case the quarterback must not throw.
pub fn best(candidates: &[ThrowCandidate]) -> Option<PlayerId> {
    candidates.first().map(|c| c.id)
}
