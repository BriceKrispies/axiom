//! **The encounter read** — the one place the game answers "who is in front of
//! me, and what would happen if I hit him?"
//!
//! It has four consumers that must never disagree: the on-field tell, the HUD
//! chip, the headless policy, and the agent's observation. So it is computed
//! once, here, and published on [`super::RunbackStatus`]; nobody recomputes it.
//! An indicator drawn from one prediction while the simulation resolves another
//! is worse than no indicator at all.

use axiom::prelude::Vec3;

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

/// Read the encounter in front of `back`, if there is one.
pub fn encounter(sim: &SimState, back: PlayerId) -> Option<Encounter> {
    let runner = &sim.players[back.index()];
    let (defender_id, gap) = nearest_threat(sim, runner)?;
    let defender = &sim.players[defender_id.index()];
    let to = defender.pos.subtract(runner.pos);
    let flat = Vec3::new(to.x, 0.0, to.z);
    let direction = flat.mul_scalar(1.0 / flat.length().max(1.0e-4));
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
        predicted_charge: charge::resolve(runner, defender, gap, &sim.runback_tuning),
    })
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
}

/// How far ahead a defender may be and still open a charge window, yd.
///
/// Matched to how long the shoulder stays armed: telling the player about a man
/// they cannot reach before the move expires is telling them to waste it.
///
/// It is also what makes the tell long enough to *act on*. Measured in the
/// browser at 4.2 yd the marker was lit for 14 frames in 386 of live carry —
/// about a tenth of a second at a time, which is a flicker rather than a cue.
/// The window is short because the encounter is: two bodies closing at 8 yd/s
/// do not stay in any one geometry for long. Reaching further up the field is
/// the honest way to lengthen it — the prediction stays exactly as true, the
/// player just gets to see it coming.
pub const CHARGE_TELL_RANGE: f32 = 7.0;

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

/// Advance the charge window one tick, **with hysteresis**.
///
/// Opening and closing are deliberately different tests, and that asymmetry is
/// the whole of what makes the tell readable:
///
/// * it **opens** only on a comfortable win ([`CHARGE_TELL_MARGIN`]), so a
///   window that appears is worth acting on;
/// * it **stays open** for as long as pressing would still win at all — a bare
///   `overload >= 1.0` — against the same man.
///
/// Without the asymmetry the marker chased the threshold: `overload` drifts as
/// two bodies converge, so a single approach crossed 1.06 several times and the
/// tell strobed on and off in tenth-of-a-second flashes. Hysteresis turns that
/// into one window that opens once and lasts until the chance is genuinely
/// gone.
///
/// It is **not** a latch, and the difference matters. A latch would hold the
/// marker lit for a fixed time after the chance had passed, which is exactly the
/// bait-and-switch the charge itself was just fixed to stop doing: while this is
/// lit, pressing wins. When it stops being true it goes out on that tick.
pub fn advance_charge_window(
    sim: &SimState,
    back: PlayerId,
    can_move: bool,
    open: Option<ChargeWindow>,
) -> Option<ChargeWindow> {
    let held_defender = open.map(|window| window.defender);
    can_move
        .then(|| encounter(sim, back))
        .flatten()
        .filter(|seen| seen.gap <= CHARGE_TELL_RANGE)
        .filter(|seen| seen.predicted_charge.won)
        .filter(|seen| {
            // Already open on this man: hold it while it is still a win.
            // Otherwise it has to clear the higher bar to open at all.
            let holding = held_defender == Some(seen.defender);
            let bar = [CHARGE_TELL_MARGIN, 1.0][usize::from(holding)];
            seen.predicted_charge.overload >= bar
        })
        .map(|seen| ChargeWindow {
            defender: seen.defender,
            gap: seen.gap,
            overload: seen.predicted_charge.overload,
        })
}
