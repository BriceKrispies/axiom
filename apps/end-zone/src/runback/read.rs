//! **The encounter read** — the one place the game answers "who is in front of
//! me, and what would happen if I hit him?"
//!
//! It has four consumers that must never disagree: the on-field tell, the HUD
//! chip, the headless policy, and the agent's observation. So it is computed
//! once, here, and published on [`super::RunbackStatus`]; nobody recomputes it.
//! An indicator drawn from one prediction while the simulation resolves another
//! is worse than no indicator at all.

use axiom::prelude::Vec3;

use crate::config::DT;
use crate::identity::PlayerId;
use crate::player::PlayerSim;
use crate::state::SimState;

use super::charge::{self, ChargeResolution};

/// The encounter in front of the runner right now: who, how far, how fast the
/// two are coming together, how set he is to meet it, and — the expensive part —
/// what the shoulder charge would actually do about it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Encounter {
    pub defender: PlayerId,
    /// Ground gap between the two bodies, yd.
    pub gap: f32,
    /// Closing speed along the contact normal, yd/s.
    pub closing: f32,
    /// How squarely the defender is set to meet a hit, `0..=1`.
    pub brace: f32,
    /// Whether he is coming from the offense's right hand. A cut goes the other
    /// way.
    pub from_right: bool,
    /// **When the collision is**, in ticks from now, if the two are actually on
    /// a course to meet. `None` when they are not closing — a defender running
    /// parallel or away is not an encounter to answer, however near he is.
    ///
    /// This is the number every decision should be made against, and the reason
    /// it exists. A move takes time to happen: a cut plays out over a quarter of
    /// a second, a leap reaches its apex in getting on for half of one, and a
    /// lowered shoulder may not touch anybody for two thirds. Judging any of
    /// them against where a defender is *now* asks the wrong question, and with
    /// a human's reaction time in the loop it asks it about a picture that is
    /// already out of date. Judging them against where he *will be* asks the
    /// right one — and it is stable, because the collision is a fixed future
    /// event rather than a geometry that changes every tick.
    pub contact_in_ticks: Option<u32>,
    /// **What the shoulder charge would do**, resolved through the same
    /// [`charge::resolve`] the simulation will run.
    ///
    /// This is a *prediction that cannot drift*, because the simulation resolves
    /// the charge on the geometry as it was when the player pressed — which is
    /// this geometry. Predicting with a threshold instead, as an earlier version
    /// did, was a guess at the arithmetic rather than the arithmetic, and
    /// measured against real play it never once fired.
    pub predicted_charge: ChargeResolution,
}

/// The nearest defender **in front of** the runner who can still make a play,
/// with the ground gap to him.
///
/// "In front" is measured along the drive, not around him: a defender level with
/// or behind the runner is not an encounter, and answering one is how you waste
/// the move you need two ticks later.
pub fn nearest_threat(sim: &SimState, runner: &PlayerSim) -> Option<(PlayerId, f32)> {
    let forward = sim.frame.forward();
    sim.players
        .iter()
        .filter(|p| p.team != runner.team && p.anim.can_act())
        .filter_map(|p| {
            let to = p.pos.subtract(runner.pos);
            let flat = Vec3::new(to.x, 0.0, to.z);
            (flat.dot(forward) > -0.5).then(|| (p.id, flat.length()))
        })
        .fold(None::<(PlayerId, f32)>, |best, (id, gap)| {
            match best.map(|(_, b)| gap < b).unwrap_or(true) {
                true => Some((id, gap)),
                false => best,
            }
        })
}

/// How far ahead a collision is looked for, in ticks (~1.3 s).
///
/// Longer than the slowest move takes to happen, so nothing is ever invisible
/// because it was still too far away to see coming — and long enough to absorb a
/// human's reaction time on top.
pub const CONTACT_HORIZON_TICKS: u32 = 80;

/// Where a body will be in `ticks`, carried forward at its current velocity.
///
/// Constant velocity, deliberately. It is what a person extrapolates, it is what
/// makes the answer stable across an approach (a fancier model would revise
/// itself every tick and reintroduce the flicker this exists to remove), and it
/// is honest about being an estimate rather than a promise about the future.
fn projected(player: &PlayerSim, ticks: u32) -> PlayerSim {
    let travel = ticks as f32 * DT;
    let mut ahead = *player;
    ahead.pos = Vec3::new(
        player.pos.x + player.vel.x * travel,
        player.pos.y,
        player.pos.z + player.vel.z * travel,
    );
    ahead
}

/// When these two will collide, in ticks, if neither changes anything.
///
/// Sampled per tick rather than solved, so the answer lives in the same
/// fixed-step arithmetic as the rest of the simulation and is exactly
/// reproducible. `None` if they never come inside contact range within the
/// horizon — which includes every case where they are not really closing at all.
pub fn contact_in_ticks(
    runner: &PlayerSim,
    defender: &PlayerSim,
    reach: f32,
    horizon: u32,
) -> Option<u32> {
    let relative_pos = Vec3::new(
        defender.pos.x - runner.pos.x,
        0.0,
        defender.pos.z - runner.pos.z,
    );
    let relative_vel = Vec3::new(
        defender.vel.x - runner.vel.x,
        0.0,
        defender.vel.z - runner.vel.z,
    );
    (0..=horizon).find(|tick| {
        relative_pos
            .add(relative_vel.mul_scalar(*tick as f32 * DT))
            .length()
            <= reach
    })
}

/// The reach at which the shoulder finds contact.
fn contact_reach(runner: &PlayerSim, defender: &PlayerSim, sim: &SimState) -> f32 {
    runner.archetype.body_radius
        + defender.archetype.body_radius
        + sim.runback_tuning.shoulder_reach
}

/// Read the encounter in front of `back`, if there is one.
///
/// The charge is predicted **at the projected collision**, not here and now.
/// That is the difference between "would this hit work if it happened this
/// instant" — which it never does — and "will the hit I am about to have work",
/// which is the only question worth asking, and the only one whose answer is
/// still true a reaction time later.
pub fn encounter(sim: &SimState, back: PlayerId) -> Option<Encounter> {
    let runner = &sim.players[back.index()];
    let (defender_id, gap) = nearest_threat(sim, runner)?;
    let defender = &sim.players[defender_id.index()];
    let to = defender.pos.subtract(runner.pos);
    let flat = Vec3::new(to.x, 0.0, to.z);
    let direction = flat.mul_scalar(1.0 / flat.length().max(1.0e-4));
    let reach = contact_reach(runner, defender, sim);
    let meeting = contact_in_ticks(runner, defender, reach, CONTACT_HORIZON_TICKS);
    Some(Encounter {
        defender: defender_id,
        gap,
        closing: Vec3::new(
            runner.vel.x - defender.vel.x,
            0.0,
            runner.vel.z - defender.vel.z,
        )
        .dot(direction),
        brace: defender.facing_dir().dot(direction.mul_scalar(-1.0)).max(0.0),
        from_right: direction.dot(sim.frame.right()) >= 0.0,
        contact_in_ticks: meeting,
        predicted_charge: predict_charge(runner, defender, meeting, &sim.runback_tuning),
    })
}

/// Resolve the charge the runner would get **at the collision**.
///
/// Everything is evaluated where the two bodies will actually meet, so the
/// alignment term reads the angle of the real hit rather than the angle of an
/// approach that has not happened yet — and the timing term reads the LEAD, how
/// long after the press the collision lands, which is the thing the player is
/// actually judging.
pub fn predict_charge(
    runner: &PlayerSim,
    defender: &PlayerSim,
    meeting: Option<u32>,
    tuning: &crate::data::RunbackTuning,
) -> ChargeResolution {
    let ticks = meeting.unwrap_or(0);
    charge::resolve(
        &projected(runner, ticks),
        &projected(defender, ticks),
        ticks,
        tuning,
    )
}

/// An open **charge window**: a specific defender the back could run through
/// right now, and by how much.
///
/// This is the value behind the tell. It is deliberately a *fact about the
/// simulation* rather than a presentation hint — it is published on
/// `RunbackStatus`, the same struct the HUD, the on-field marker, the headless
/// policy and the agent all read, so what the player is shown and what the agent
/// perceives are the same number, and both are what the game will actually do.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChargeWindow {
    /// The man who can be run through.
    pub defender: PlayerId,
    /// The gap to him, yd.
    pub gap: f32,
    /// `impulse / resistance` — how decisively the charge would be won. Drives
    /// how strongly the tell reads, so a marginal window looks marginal.
    pub overload: f32,
    /// How long until the collision, in ticks — how much time you have to
    /// answer it.
    pub contact_in_ticks: u32,
}

/// How far ahead the tell will warn about a collision, in ticks (~0.8 s).
///
/// A **time**, not a distance, and that swap is what finally made the cue
/// usable. A distance threshold lights when a man is close, which at 10 yd/s of
/// closing is a tenth of a second before he arrives — measured in the browser,
/// 14 lit frames in 386 of live carry, a flicker nobody can answer. A time
/// threshold lights a fixed lead before the *collision*, so the window is as
/// long as the number says regardless of how fast the two are travelling, and it
/// is stable across the approach because it is about one fixed future event.
///
/// Sized as a human reaction time (500 ms, 30 ticks) PLUS the lead the move
/// itself needs, so that by the time a person has seen it and pressed, there is
/// still enough of the approach left for the move to happen. A cue that arrives
/// with less warning than that is information you cannot use.
pub const CHARGE_TELL_HORIZON_TICKS: u32 = 78;

/// How decisively the charge must be won before the tell appears.
///
/// Above `1.0` on purpose. The tell is a **promise**, and a promise made on a
/// dead heat is a promise broken half the time: the defender keeps moving in the
/// ticks between the press and the collision, and a window that was exactly
/// break-even can be lost by nothing the player did.
///
/// Only a little above, though. Every extra point of margin is a window that
/// closes sooner, and a cue nobody has time to answer is worth less than a cue
/// that is occasionally optimistic.
pub const CHARGE_TELL_MARGIN: f32 = 1.06;

/// Advance the charge window one tick.
///
/// Now that the charge is a couple of seconds of immunity rather than a single
/// contested collision, the tell has a much simpler and much more honest job:
/// **there is somebody in front of you and you have the charge available.** No
/// prediction, no threshold, no hysteresis to stop it strobing — it cannot
/// strobe, because it is not tracking a quantity that drifts. The old version
/// needed all three because it was forecasting the outcome of a knife-edge
/// contest, and the reason it needed them was the reason the contest had to go.
///
/// While it is lit, pressing runs you through that man. That is still a promise;
/// it is just one the game can now keep trivially.
pub fn advance_charge_window(
    sim: &SimState,
    back: PlayerId,
    can_charge: bool,
    _open: Option<ChargeWindow>,
) -> Option<ChargeWindow> {
    can_charge
        .then(|| encounter(sim, back))
        .flatten()
        .filter(|seen| {
            seen.contact_in_ticks
                .is_some_and(|ticks| ticks <= CHARGE_TELL_HORIZON_TICKS)
        })
        .map(|seen| ChargeWindow {
            defender: seen.defender,
            gap: seen.gap,
            overload: seen.predicted_charge.overload,
            contact_in_ticks: seen.contact_in_ticks.unwrap_or(0),
        })
}
