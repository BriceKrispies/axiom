//! The headless **autopilot**: the deterministic policy that plays the run game
//! when nobody is holding a phone.
//!
//! It answers exactly the questions a player answers, and nothing else — which
//! play to call at the line ([`call_play`]), and what to do about the defender
//! in front of you right now ([`decide_move`]). *Where* to run is not one of
//! them: the ball carrier's heading is the AI's, for everybody, and lives in
//! [`crate::ai::carry`]. That split is the point — this file used to also
//! produce a movement stick, which meant the headless game and the played game
//! were being driven by two different pieces of code that could disagree.
//!
//! It is a pure function of the simulation: no I/O, no wall clock, no
//! randomness, so an autopiloted run replays bit-for-bit like any other run.
//!
//! It is also the game's **tuning instrument**. Running the same carry under
//! different [`Aggression`] profiles is how we check that the three moves are
//! genuinely different tools rather than three spellings of one: if a policy
//! that only ever jukes posts the same numbers as one that reads the geometry,
//! the design has failed its own claim.

use crate::attempt::{AttemptPhase, AttemptStep};
use crate::identity::PlayerId;
use crate::player::PlayerSim;
use crate::runback::{charge, ChargeResolution, RunbackMove};
use crate::state::SimState;

/// The concept the autopilot calls, unless told otherwise.
///
/// Fixed by default, and deliberately: [`decide_move`] is the instrument, and a
/// policy that also shopped the playbook would confound what it measures — two
/// profiles could post different numbers because they ran different plays rather
/// than because they answered defenders differently.
pub const AUTOPILOT_CONCEPT: usize = 0;

/// The play to call, while the offense is waiting on one (`None` otherwise).
///
/// An attempt does not start until a play is called, so a headless session has
/// to answer this or it stands at the line forever.
pub fn call_play(step: &AttemptStep) -> Option<usize> {
    matches!(step.phase, AttemptPhase::PlayCall).then_some(AUTOPILOT_CONCEPT)
}

/// How a simulated back answers an encounter — the knob the harness sweeps.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aggression {
    /// How near a defender must be before the policy does anything at all, yd.
    /// Its whole skill is here: react too early and the move is spent before the
    /// defender commits, too late and he has already made the tackle.
    pub react_range: f32,
    /// How decisively the predicted charge must be won before the policy will
    /// take contact rather than avoid it. `1.0` is a dead heat; above it is
    /// margin, because the prediction is made a few ticks before the collision
    /// and the geometry moves in between.
    pub charge_margin: f32,
    /// Whether the leap is on the table at all.
    pub will_jump: bool,
}

impl Aggression {
    /// Reads the geometry and picks the move that fits it — the profile the
    /// agent and the balance harness both run.
    pub const BALANCED: Aggression = Aggression {
        react_range: 3.4,
        charge_margin: 1.05,
        will_jump: true,
    };
    /// Identical to [`Self::BALANCED`] in every respect except that it never
    /// presses the down button. The B arm of the shoulder-charge A/B: with one
    /// control removed and nothing else changed, any difference in the numbers
    /// is that control's doing.
    pub const NO_SHOULDER: Aggression = Aggression {
        charge_margin: f32::INFINITY,
        ..Aggression::BALANCED
    };
    /// Never takes contact: everything is a cut.
    pub const EVASIVE: Aggression = Aggression {
        react_range: 3.4,
        charge_margin: f32::INFINITY,
        will_jump: false,
    };
    /// Runs at everything it could win.
    pub const BRUISING: Aggression = Aggression {
        react_range: 3.0,
        charge_margin: 1.0,
        will_jump: false,
    };
}

/// The nearest defender in front of the runner who can still make a play, with
/// the ground gap to him.
///
/// "In front" is measured along the drive, not around him: a defender level with
/// or behind the runner is not an encounter, and answering one is how a policy
/// wastes the move it needs two ticks later.
pub fn nearest_threat(sim: &SimState, runner: &PlayerSim) -> Option<(PlayerId, f32)> {
    let forward = sim.frame.forward();
    sim.players
        .iter()
        .filter(|p| p.team != runner.team && p.anim.can_act())
        .filter_map(|p| {
            let to = p.pos.subtract(runner.pos);
            let flat = axiom::prelude::Vec3::new(to.x, 0.0, to.z);
            (flat.dot(forward) > -0.5).then(|| (p.id, flat.length()))
        })
        .fold(None::<(PlayerId, f32)>, |best, (id, gap)| {
            match best.map(|(_, b)| gap < b).unwrap_or(true) {
                true => Some((id, gap)),
                false => best,
            }
        })
}

/// The encounter in front of the runner right now: who, how far, how fast the
/// two are coming together, and how set he is to meet it.
///
/// This is the **eyes**, and it is deliberately separate from every policy that
/// reads it. [`decide_move`] and [`crate::agent`] both look at exactly this, so
/// the headless game and the agent can never end up judging different fields —
/// only deciding differently about the same one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Encounter {
    pub defender: PlayerId,
    /// Ground gap between the two bodies, yd.
    pub gap: f32,
    /// Closing speed along the contact normal, yd/s — the charge's own input.
    pub closing: f32,
    /// How squarely the defender is set to meet a hit, `0..=1`.
    pub brace: f32,
    /// Whether he is coming from the offense's right hand. A cut goes the other
    /// way.
    pub from_right: bool,
    /// **What the shoulder charge would actually do**, resolved right now
    /// through the same [`crate::runback::charge::resolve`] the simulation will
    /// run at contact.
    ///
    /// Predicting the real contest replaces what used to be here — a pair of
    /// hand-picked thresholds on closing speed and brace, which were a guess at
    /// the arithmetic rather than the arithmetic. The guess was wrong in the
    /// only way that mattered: measured against real play it never once fired,
    /// because it demanded a closing speed that only occurs in a head-on
    /// collision. Asking the resolution directly cannot drift from it.
    pub predicted_charge: ChargeResolution,
}

/// Read the encounter in front of the runner, if there is one.
pub fn encounter(sim: &SimState, step: &AttemptStep) -> Option<Encounter> {
    let back = step.runback.back?;
    let runner = &sim.players[back.index()];
    let (defender_id, gap) = nearest_threat(sim, runner)?;
    let defender = &sim.players[defender_id.index()];
    let to = defender.pos.subtract(runner.pos);
    let flat = axiom::prelude::Vec3::new(to.x, 0.0, to.z);
    let direction = flat.mul_scalar(1.0 / flat.length().max(1.0e-4));
    Some(Encounter {
        defender: defender_id,
        gap,
        predicted_charge: charge::resolve(runner, defender, gap, &sim.runback_tuning),
        closing: axiom::prelude::Vec3::new(
            runner.vel.x - defender.vel.x,
            0.0,
            runner.vel.z - defender.vel.z,
        )
        .dot(direction),
        brace: defender.facing_dir().dot(direction.mul_scalar(-1.0)).max(0.0),
        from_right: direction.dot(sim.frame.right()) >= 0.0,
    })
}

/// What the policy does about the man in front of it this tick, or `None` to
/// keep running.
///
/// The decision is the geometry, in the order a person would read it:
///
/// 1. **Is anyone actually there?** Nothing inside `react_range` means nothing
///    to answer, and a move spent on empty grass is a move you do not have when
///    somebody arrives.
/// 2. **Can I go through him?** Enough closing speed, and he is not squared up
///    to meet it. This is the [`crate::runback::charge`] contest's own inputs,
///    read the same way the resolution will read them, so the policy is
///    *predicting* the collision rather than guessing at it.
/// 3. **Can I go over him?** Only if the leap is ready — otherwise this is a
///    free way to be tackled while airborne and out of options.
/// 4. **Otherwise, go round him** — cut away from the side he is coming from.
pub fn decide_move(sim: &SimState, step: &AttemptStep, policy: Aggression) -> Option<RunbackMove> {
    if !step.phase.controllable() || !step.runback.move_ready {
        return None;
    }
    let seen = encounter(sim, step)?;
    if seen.gap > policy.react_range {
        return None;
    }
    let can_charge =
        seen.predicted_charge.won && seen.predicted_charge.overload >= policy.charge_margin;
    let can_jump = policy.will_jump && step.runback.jump_available;
    match (can_charge, can_jump) {
        (true, _) => Some(RunbackMove::Shoulder),
        // Over the top of a man who is squared up and waiting: exactly the
        // encounter the charge would lose.
        (false, true) => Some(RunbackMove::Jump),
        // Cut AWAY from him.
        (false, false) => match seen.from_right {
            true => Some(RunbackMove::JukeLeft),
            false => Some(RunbackMove::JukeRight),
        },
    }
}
