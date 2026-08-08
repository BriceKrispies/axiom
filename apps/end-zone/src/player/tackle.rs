//! **The tackle contest.** Contact is a question, not an answer.
//!
//! A tackle used to be a *touch*: get within `tackle_range` of the carrier with
//! any closing speed above a floor and he went down, every time, without fail.
//! Every other physical interaction in this game is a contest with terms you can
//! read — the blocking engagement, the shoulder charge — and the most important
//! one in the sport was the one place a number never got compared to another
//! number.
//!
//! So it is a contest, on the same shape as the charge in
//! [`crate::runback::charge`], deliberately: one impulse against one resistance,
//! every term derived from state on the screen, no roll anywhere.
//!
//! ```text
//!   hit        = closing_speed · squareness
//!   wrap       = grip · tackle_strength
//!   impulse    = mass · (hit + wrap) · wrap_power  (· dive bonus)
//!   resistance = mass · break_speed  · leg_drive · drive · footing
//!   the tackle lands  ⇔  impulse >= resistance
//! ```
//!
//! A tackle is **two things at once**, and getting that wrong was the first
//! version of this file. Modelling only the hit made chase-down tackles
//! impossible: the pursuit AI runs a carrier down and then *matches his pace*,
//! so closing speed at contact is near zero, and a measured run had six carries
//! in nine walking into the end zone untouched. So there is a hit and there is a
//! wrap, and either can bring a man down.
//!
//! Reading the terms:
//!
//! * **closing_speed** — the hit, and the answer to the complaint this file was
//!   written for. An arm thrown at a man running the same speed as you is not a
//!   takedown; hitting him head-on is.
//! * **wrap** — the grip. Speed-independent, so a defender who has caught up and
//!   is running alongside can still get hold of him and drag him down. It is
//!   sized *just short* of a fresh carrier at full stride, which is the whole
//!   balance of the mechanic: a clean wrap on a man at full speed is not quite
//!   enough on its own, and needs either some closing speed behind it or a
//!   carrier who has already been knocked about.
//! * **squareness** — how much of the tackler's *own* travel is into the
//!   carrier. A hit from the side or from behind carries a fraction of the same
//!   speed's worth. Floored at `0.35` rather than zero: a stationary defender the
//!   runner runs straight into still gets a piece of him.
//! * **wrap_power** — the archetype's existing `tackle_strength`.
//! * **leg_drive** — a carrier at speed is *harder* to bring down, not easier.
//!   This is why breaking into the open field matters: the same defender who
//!   would have stopped you at the line cannot stop you at full stride.
//! * **drive** — the carrier's `block_strength`, the same "how well does this
//!   body deal with contact" number the shoulder charge reads as power.
//! * **footing** — his `balance`, the existing `0..=1` the contact framework
//!   already depletes. A shed costs a chunk of it, so sheds are **cumulative**:
//!   surviving one hit leaves you easier to bring down, and the second or third
//!   man through finishes the play. That is what stops "some tackles get shed"
//!   from becoming "the back is unstoppable".
//!
//! A failed tackle is not nothing: the tackler bounces off into a
//! [`AnimState::HitReaction`] and is out of the play for a beat, and the carrier
//! is slowed and knocked off balance. Both sides pay for it, which is what makes
//! a shed feel like something survived rather than something skipped.

use axiom::prelude::Vec3;

use crate::data::BehaviorTuning;
use crate::identity::PlayerId;

use super::{AnimState, PlayerSim};

/// One resolved tackle attempt — the verdict plus every term that produced it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TackleContest {
    pub tackler: PlayerId,
    pub target: PlayerId,
    /// Closing speed along the contact normal, yd/s.
    pub closing_speed: f32,
    /// How much of the tackler's travel is into the carrier, `0..=1`.
    pub squareness: f32,
    /// The hit delivered.
    pub impulse: f32,
    /// The carrier's resistance to it.
    pub resistance: f32,
    /// `impulse / resistance` — how decisively it went, either way.
    pub overload: f32,
    /// Whether the carrier went down.
    pub landed: bool,
    /// Whether the tackler had left his feet.
    pub diving: bool,
    /// Unit contact direction, tackler → carrier.
    pub direction: Vec3,
}

impl TackleContest {
    /// Normalized hit strength `0..=1` — what the camera shake, the dust and the
    /// knockdown arc are scaled by. Derived from the contest rather than
    /// recomputed, so what you feel is what was decided.
    pub fn strength(&self, tuning: &BehaviorTuning) -> f32 {
        ((self.closing_speed / tuning.tackle_full_strength_speed) * self.overload.min(1.6))
            .clamp(0.05, 1.0)
    }

    /// A one-line explanation for the debug overlay and the agent trace.
    pub fn describe(&self) -> &'static str {
        match (self.landed, self.squareness < 0.55, self.closing_speed < 3.0) {
            (true, _, _) => "wrapped",
            (false, _, true) => "shed: ran alongside, no hit behind it",
            (false, true, _) => "shed: off-angle arm tackle",
            (false, false, false) => "shed: outmuscled",
        }
    }
}

/// How much a carrier's own speed adds to his resistance, as a fraction of the
/// full-strength speed.
///
/// Small on purpose. Momentum SHOULD make a man harder to bring down — that is
/// why breaking into the open field matters — but at a slope of 1.0 it made a
/// back at full stride effectively untacklable by anyone who had merely caught
/// up with him: a measured run posted ten touchdowns in thirteen carries. At
/// 0.4 a full-speed back is about a quarter harder to tackle, which is an edge
/// rather than immunity.
const LEG_DRIVE_SLOPE: f32 = 0.4;

fn flat(v: Vec3) -> Vec3 {
    Vec3::new(v.x, 0.0, v.z)
}

/// Resolve one tackle attempt. Pure: same inputs, same verdict, every time.
pub fn contest(
    tackler: &PlayerSim,
    carrier: &PlayerSim,
    diving: bool,
    tuning: &BehaviorTuning,
) -> TackleContest {
    let to_carrier = flat(carrier.pos.subtract(tackler.pos));
    let distance = to_carrier.length();
    let direction = match distance > 1.0e-4 {
        true => to_carrier.mul_scalar(1.0 / distance),
        false => tackler.facing_dir(),
    };

    // The hit. Closing speed is measured along the contact normal from BOTH
    // bodies' velocities, so running the carrier down and matching his pace
    // leaves nothing to hit him with — which is the whole point.
    let closing_speed = flat(tackler.vel.subtract(carrier.vel)).dot(direction).max(0.0);
    let tackler_speed = flat(tackler.vel).length();
    let squareness = match tackler_speed > 1.0e-4 {
        true => 0.35 + 0.65 * flat(tackler.vel).mul_scalar(1.0 / tackler_speed).dot(direction).max(0.0),
        // A planted defender the runner arrives at still gets the floor.
        false => 0.35,
    };
    let wrap_power = 0.55 + 0.45 * tackler.archetype.tackle_strength;
    let dive_bonus = match diving {
        true => tuning.tackle_dive_bonus,
        false => 1.0,
    };
    let hit = closing_speed * squareness;
    let wrap = tuning.tackle_grip * tackler.archetype.tackle_strength;
    let impulse = tackler.archetype.mass * (hit + wrap) * wrap_power * dive_bonus;

    // The resistance.
    let carrier_speed = flat(carrier.vel).length();
    let leg_drive =
        1.0 + LEG_DRIVE_SLOPE * carrier_speed / tuning.tackle_full_strength_speed.max(1.0e-3);
    let drive = 0.5 + 0.5 * carrier.archetype.block_strength;
    // Footing never reaches zero: a spent runner resists a little, so a man on
    // his last legs is easy to bring down rather than mathematically certain to
    // be, and the arithmetic can never divide the game by zero.
    let footing = 0.3 + 0.7 * carrier.balance.clamp(0.0, 1.0);
    let resistance =
        (carrier.archetype.mass * tuning.tackle_break_speed * leg_drive * drive * footing)
            .max(1.0e-3);

    TackleContest {
        tackler: tackler.id,
        target: carrier.id,
        closing_speed,
        squareness,
        impulse,
        resistance,
        overload: impulse / resistance,
        landed: impulse >= resistance,
        diving,
        direction,
    }
}

/// Apply a **landed** tackle to both bodies: the carrier takes the hit and goes
/// down, the tackler commits to the wrap.
pub fn apply_landed(
    players: &mut [PlayerSim],
    contest: &TackleContest,
    tuning: &BehaviorTuning,
) -> bool {
    let strength = contest.strength(tuning);
    let airborne = strength >= tuning.airborne_threshold;
    let hit = &mut players[contest.target.index()];
    hit.balance = 0.0;
    hit.impact_strength = strength;
    hit.vel = contest.direction.mul_scalar(contest.closing_speed * 0.35);
    match airborne {
        true => {
            hit.vertical_vel = tuning.launch_up_speed * strength;
            hit.set_anim(AnimState::AirborneFall);
        }
        false => hit.set_anim(AnimState::Stumble),
    }
    let tackler = &mut players[contest.tackler.index()];
    match contest.diving {
        true => {
            tackler.pos = Vec3::new(tackler.pos.x, 0.0, tackler.pos.z);
            tackler.vertical_vel = 0.0;
            tackler.vel = tackler.vel.mul_scalar(0.2);
            tackler.set_anim(AnimState::GroundImpact);
        }
        false => {
            tackler.vel = tackler.vel.mul_scalar(0.25);
            tackler.set_anim(AnimState::Tackle);
        }
    }
    airborne
}

/// Apply a **shed** tackle: the carrier stays up but pays for it, and the
/// tackler bounces off out of the play.
///
/// Both halves matter. Without the carrier's cost, a good back would run through
/// the whole defense; without the tackler's, he would simply re-attempt on the
/// next tick and the shed would be a one-frame flicker nobody could see.
pub fn apply_shed(players: &mut [PlayerSim], contest: &TackleContest, tuning: &BehaviorTuning) {
    // How completely he won it — a shed that was nearly a tackle costs more.
    let marginal = (1.0 - contest.overload.clamp(0.0, 1.0)).clamp(0.15, 1.0);
    let carrier = &mut players[contest.target.index()];
    carrier.balance = (carrier.balance - tuning.tackle_shed_balance_cost).max(0.0);
    // Contact always scrubs speed; a glancing one scrubs less.
    carrier.vel = carrier.vel.mul_scalar(0.72 + 0.2 * marginal);
    carrier.impact_strength = contest.strength(tuning);

    let tackler = &mut players[contest.tackler.index()];
    match contest.diving {
        // A diver who whiffed is on the turf either way — he left his feet.
        true => {
            tackler.pos = Vec3::new(tackler.pos.x, 0.0, tackler.pos.z);
            tackler.vertical_vel = 0.0;
            tackler.vel = tackler.vel.mul_scalar(0.15);
            tackler.impact_strength = tuning.dive_whiff_impact;
            tackler.set_anim(AnimState::GroundImpact);
        }
        // A standing tackler is bounced off, on his feet but out of it.
        false => {
            tackler.vel = contest.direction.mul_scalar(-1.5);
            tackler.set_anim(AnimState::HitReaction);
        }
    }
}
