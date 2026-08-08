//! **The shoulder charge: one contest, decided by arithmetic.**
//!
//! Running *through* a man is the only one of the three moves that is a
//! collision rather than an avoidance, so it is the only one that needs a
//! collision *resolution*. There is no roll, no chance, and no hidden number:
//! the outcome is a comparison of two quantities, each built from state the
//! player can see on the screen and a test can construct by hand.
//!
//! ```text
//!   impulse    = mass · closing_speed · alignment · timing · power · balance
//!   resistance = mass · anchor       · brace                · resist_speed
//!   the runner goes through  ⇔  impulse > resistance
//! ```
//!
//! Reading each term, and what the player is doing when they move it:
//!
//! * **closing_speed** — how fast the runner is actually going *at* the
//!   defender, measured along the contact normal, and using both bodies'
//!   velocities. Charging a man running away from you is worth far less than
//!   charging one running at you, which is exactly right: the second is a
//!   collision and the first is a chase.
//! * **alignment** — how squarely the runner's own velocity points at the
//!   defender, `0..1`. A charge thrown at someone off to the side glances.
//! * **timing** — how close the gap was, when the shoulder went down, to the
//!   ideal one. Dropping it on top of a defender means never getting low;
//!   dropping it from too far out means leaning through most of the approach.
//!   This is the term the *skill* lives in, and it is the only one derived from
//!   the moment of the button press rather than from the moment of contact.
//! * **power** — the archetype's `block_strength`, the existing number for "how
//!   well this body delivers a hit". The running back has a high one; a receiver
//!   does not, which is why this is the back's move.
//! * **balance** — the existing `0..=1` the contact framework already depletes.
//!   A runner who has just been hit cannot then run someone over.
//! * **anchor** — the defender's existing `tackle_strength`.
//! * **brace** — whether the defender is *squared up*. Reads his facing against
//!   the contact normal: a defender looking straight at the runner braces fully;
//!   one caught turned is worth [`RunbackTuning::charge_brace_floor`] of that.
//!   This is why beating a defender to a spot and then charging works, and why
//!   charging the man who is already square is the bad idea it should be.
//!
//! Every one of those is a number the game already had (mass, block strength,
//! tackle strength, balance, velocity) — nothing parallel was invented for this.
//!
//! The whole resolution is returned as a [`ChargeResolution`] with every term on
//! it, so "why did I bounce off him" has a real answer in a test, in the debug
//! overlay, and in the agent's observation.

use axiom::prelude::Vec3;

use crate::data::RunbackTuning;
use crate::identity::PlayerId;
use crate::player::PlayerSim;

/// One resolved shoulder charge — the verdict plus every term that produced it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChargeResolution {
    pub runner: PlayerId,
    pub defender: PlayerId,
    /// Ground gap between the two bodies' centres at contact, yd.
    pub contact_gap: f32,
    /// The gap at the moment the shoulder went down, yd — the timing input.
    pub commit_gap: f32,
    /// Closing speed along the contact normal, yd/s.
    pub closing_speed: f32,
    /// How squarely the runner is travelling at the defender, `0..=1`.
    pub alignment: f32,
    /// The timing factor derived from `commit_gap`, `0..=1`.
    pub timing: f32,
    /// The runner's delivered momentum.
    pub impulse: f32,
    /// The defender's braced anchor.
    pub resistance: f32,
    /// How squared-up the defender was, `0..=1`.
    pub brace: f32,
    /// `impulse / resistance` — how decisively it went, either way.
    pub overload: f32,
    /// Whether the runner went through.
    pub won: bool,
    /// The unit contact direction, runner → defender.
    pub direction: Vec3,
}

impl ChargeResolution {
    /// A one-line explanation, for the debug overlay and the agent trace.
    pub fn describe(&self) -> &'static str {
        match (self.won, self.timing < 0.55, self.alignment < 0.6) {
            (true, _, _) => "through",
            (false, true, _) => "stuffed: mistimed",
            (false, false, true) => "stuffed: off-angle",
            (false, false, false) => "stuffed: outmuscled",
        }
    }
}

/// The timing factor for a shoulder dropped with `commit_gap` yards to go.
///
/// Peaks at [`RunbackTuning::charge_ideal_gap`] and falls off linearly in both
/// directions to a floor of `1 - charge_timing_penalty`. A floor rather than
/// zero because a badly-timed charge should be *weak*, not inert — a
/// fast-enough runner can still bulldoze a light defender on poor timing, and
/// that is a fair outcome rather than a bug.
pub fn timing_factor(commit_gap: f32, tuning: &RunbackTuning) -> f32 {
    let span = tuning.charge_timing_span.max(1.0e-3);
    let off = ((commit_gap - tuning.charge_ideal_gap).abs() / span).clamp(0.0, 1.0);
    1.0 - off * tuning.charge_timing_penalty
}

fn flat(v: Vec3) -> Vec3 {
    Vec3::new(v.x, 0.0, v.z)
}

/// Resolve one shoulder charge. Pure: same inputs, same verdict, every time.
///
/// `commit_gap` is the ground distance between the two when the player pressed,
/// which the stage captured then and carries here — the resolution cannot
/// recover it from the contact tick, and it is the term the skill lives in.
pub fn resolve(
    runner: &PlayerSim,
    defender: &PlayerSim,
    commit_gap: f32,
    tuning: &RunbackTuning,
) -> ChargeResolution {
    let to_defender = flat(defender.pos.subtract(runner.pos));
    let contact_gap = to_defender.length();
    // A degenerate zero-gap contact takes the runner's facing as the normal, so
    // the resolution is still total rather than dividing by zero.
    let direction = match contact_gap > 1.0e-4 {
        true => to_defender.mul_scalar(1.0 / contact_gap),
        false => runner.facing_dir(),
    };

    let closing_speed = flat(runner.vel.subtract(defender.vel)).dot(direction).max(0.0);
    let runner_speed = flat(runner.vel).length();
    let alignment = match runner_speed > 1.0e-4 {
        true => flat(runner.vel).mul_scalar(1.0 / runner_speed).dot(direction).max(0.0),
        // A standing start has no direction of travel to be aligned with, so it
        // delivers nothing — which is the correct reading of "charging" from a
        // dead stop.
        false => 0.0,
    };
    let timing = timing_factor(commit_gap, tuning);
    let power = 0.5 + 0.5 * runner.archetype.block_strength;
    let impulse = runner.archetype.mass
        * closing_speed
        * alignment
        * timing
        * power
        * runner.balance.clamp(0.0, 1.0);

    // How squarely the defender meets it: his facing against the incoming
    // runner. `-direction` is the bearing from the defender back to the runner.
    let squared = defender.facing_dir().dot(direction.mul_scalar(-1.0)).max(0.0);
    let brace = tuning.charge_brace_floor + (1.0 - tuning.charge_brace_floor) * squared;
    let anchor = 0.5 + 0.5 * defender.archetype.tackle_strength;
    let resistance = (defender.archetype.mass * anchor * brace * tuning.charge_resist_speed)
        .max(1.0e-3);

    ChargeResolution {
        runner: runner.id,
        defender: defender.id,
        contact_gap,
        commit_gap,
        closing_speed,
        alignment,
        timing,
        impulse,
        resistance,
        brace,
        overload: impulse / resistance,
        won: impulse > resistance,
        direction,
    }
}
