//! The contact framework: blocking engagements, tackle evaluation, and the
//! controlled fall (stumble → airborne → ground impact → recovery). Outcomes
//! are deterministic and authoritative; there is no ragdoll — falls are
//! procedural pose states driven by the fixed tick.

use axiom::prelude::Vec3;

use crate::ai::PlayerIntent;
use crate::collision_rig::CollisionRig;
use crate::data::BehaviorTuning;
use crate::identity::PlayerId;

use super::tackle::{self, TackleContest};
use super::{AnimState, PlayerSim};

/// Ticks a stumble lasts before the trip completes.
// Dive commitment and the fall progression moved to `falls`; callers still
// reach them through `contact::`.
pub use super::falls::{advance_falls, commit_dives};

pub(super) const STUMBLE_TICKS: u32 = 10;
/// Ticks the ground-impact pose holds before recovery starts.
pub(super) const GROUND_TICKS: u32 = 16;

/// What one tick's tackling produced: the hit that brought the carrier down (if
/// any), and every attempt he shed getting there.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TackleResolution {
    /// The tackle that landed, with the contest that decided it.
    pub landed: Option<TackleOutcome>,
    /// Attempts the carrier survived, in tackler id order.
    pub shed: Vec<TackleContest>,
}

impl TackleResolution {
    /// Whether anybody got a hand on the carrier at all this tick.
    pub fn any_contact(&self) -> bool {
        self.landed.is_some() || !self.shed.is_empty()
    }
}

/// A tackle that landed this tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TackleOutcome {
    pub tackler: PlayerId,
    pub target: PlayerId,
    pub contact_point: Vec3,
    pub contact_direction: Vec3,
    pub relative_speed: f32,
    pub strength: f32,
    pub target_airborne: bool,
    /// The contest itself, so the reason is inspectable downstream.
    pub contest: TackleContest,
}

/// Tackle evaluation: every defender who reaches the carrier this tick gets a
/// **contest**, in id order, until one of them wins it.
///
/// It used to be that reaching him *was* winning: the first man in range with
/// any closing speed above a floor put him on the turf, every time. Now reaching
/// him only earns the attempt (see [`super::tackle`]), and a man who has run the
/// carrier down but has no closing speed left to hit him with gets shed. The
/// hits and the sheds are both authoritative and deterministic — impulse against
/// resistance, no ragdoll and no roll.
pub fn resolve_tackle(
    players: &mut [PlayerSim],
    intents: &[PlayerIntent],
    carrier: Option<PlayerId>,
    carrier_charging: bool,
    tuning: &BehaviorTuning,
    collision: &CollisionRig,
) -> TackleResolution {
    let mut resolution = TackleResolution::default();
    let Some(carrier) = carrier else {
        return resolution;
    };
    // A back mid-charge is running THROUGH people: nobody brings him down for
    // the length of the window. The defenders he meets are dealt with by
    // `runback::stage::carry_charge`, which knocks them aside — so this is not
    // "nothing happens", it is "the collision goes the other way".
    if carrier_charging || !players[carrier.index()].anim.can_act() {
        return resolution;
    }
    for index in 0..players.len() {
        // Either a standing chaser holding a `Tackle` intent, or a committed
        // diver mid-lunge (whose intent has already lapsed — the dive is the
        // commitment) can attempt the hit here.
        let diving = players[index].anim == AnimState::Dive;
        let standing = matches!(
            intents[index],
            PlayerIntent::Tackle { target, .. } if target == carrier
        ) && players[index].anim.can_act();
        if !(diving || standing) {
            continue;
        }
        let tackler_pos = players[index].pos;
        let carrier_sim = &players[carrier.index()];
        let to_carrier = Vec3::new(
            carrier_sim.pos.x - tackler_pos.x,
            0.0,
            carrier_sim.pos.z - tackler_pos.z,
        );
        let distance = to_carrier.length();
        // A dive reaches only on real body contact from the collision world (arc
        // height included); a standing tackle keeps its horizontal arm-reach —
        // but only up to the height a man on his feet can actually reach.
        //
        // That height gate is the fix for a defect the horizontal-only test had
        // from the start: a whiffed dive sailing clean over the carrier still
        // measured "in range" and landed a phantom tackle, which ended the play
        // and shook the camera for a hit that never happened. It is also what
        // makes the running back's leap a real answer rather than an animation
        // — a carrier genuinely above a defender's arms cannot be brought down
        // by them, and that is one rule, written once, for both cases.
        let within_reach = carrier_sim.pos.y <= tuning.tackle_reach_height;
        let reached = match diving {
            true => collision.in_contact(players[index].id, carrier),
            false => distance <= tuning.tackle_range && within_reach,
        };
        if !reached {
            continue;
        }
        let relative = players[index].vel.subtract(carrier_sim.vel);
        let relative_speed = relative.length() + players[index].speed() * 0.25;
        // A diver is already airborne and past the point of no return — no
        // minimum-closing-speed gate; a standing attempt still needs pop to be
        // worth making at all.
        if !diving && relative_speed < tuning.tackle_min_closing_speed {
            continue;
        }

        let contest = tackle::contest(&players[index], &players[carrier.index()], diving, tuning);
        if !contest.landed {
            tackle::apply_shed(players, &contest, tuning);
            resolution.shed.push(contest);
            // The next man in id order still gets his attempt this tick, against
            // a carrier who is now slower and less balanced — which is exactly
            // how a swarm is supposed to work.
            continue;
        }
        let contact_point = players[carrier.index()].pos.add(Vec3::new(0.0, 1.0, 0.0));
        let strength = contest.strength(tuning);
        let target_airborne = tackle::apply_landed(players, &contest, tuning);
        resolution.landed = Some(TackleOutcome {
            tackler: contest.tackler,
            target: carrier,
            contact_point,
            contact_direction: contest.direction,
            relative_speed,
            strength,
            target_airborne,
            contest,
        });
        return resolution;
    }
    resolution
}
