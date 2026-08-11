//! **Playing End Zone with the reusable `axiom-agent` module.**
//!
//! The agent is not a simulation of a player; it *is* a player. Every tick it
//! runs the module's real loop and the intents that come out are lowered into
//! the same [`crate::showcase::ShowcaseRun`] entry points a keyboard reaches:
//!
//! ```text
//! sim state --perceive--> Observation (integer facts)
//!            --axiom-agent decide--> press_control intents
//!            --lower--> select_concept / RunbackMove --> the real game
//! ```
//!
//! # What is the agent's, and what is the app's
//!
//! **The app owns perception.** Which defender is the encounter, how fast the
//! two are closing, whether he is squared up, whether the leap is off cooldown —
//! that is the runner's *eyes*, and every part of it names an End Zone noun
//! `axiom-agent` must never learn. It is the same read the headless policy in
//! [`crate::autopilot`] uses, through the same [`crate::autopilot::encounter`]
//! function, so the agent and the game can never be judging different fields.
//!
//! **The agent owns the decision.** Which of the four verbs answers this
//! geometry is a *priority ordering over present facts*, and that is exactly
//! what the module's scripted brain is: an ordered rule table where the first
//! rule whose fact is present wins. The ordering — go through him if you can,
//! else over him, else round him — is written as data in [`decide_one_step`] and
//! evaluated by the module. There is no hand-rolled decision anywhere in this
//! file: cut the agent out and nothing is ever pressed.
//!
//! # Why the scripted brain and not the axis-map brain
//!
//! Burnt Rubber's driver steers a wheel, so it needs the analogue brain. A
//! running back has four buttons and presses at most one of them, so the
//! discrete brain is the right shape — and `press_control` is precisely the
//! neutral intent for "the player pushed the thing", which is what makes the
//! lowering below a translation rather than a special case.

use axiom_agent::AgentApi;
use axiom_kernel::{FrameIndex, Tick};
use axiom_runtime::RuntimeStep;

use crate::showcase::ShowcaseRun;

use crate::attempt::AttemptStep;
use crate::autopilot::{self, Aggression};
use crate::identity::PlayerId;
use crate::runback::RunbackMove;
use crate::state::SimState;

/// The app's control vocabulary: the meaning this app assigns to a neutral
/// `press_control` code. `axiom-agent` carries the `u32` opaquely.
///
/// They are **distinct bit flags**, not 1/2/3/4/5, because
/// `ActionQueue::combined_control_code` folds a tick's presses with a bitwise
/// OR — the queue models "which controls are held", which is a set. With
/// sequential integers the codes alias (`3` is `1|2`), so a single juke-right
/// press reads back as "call the play AND juke left" and the game does neither
/// thing you asked for. Getting this wrong is silent: the agent decides
/// correctly, emits correctly, and the lowering hands the game nonsense.
pub const CONTROL_CALL_PLAY: u32 = 1 << 0;
pub const CONTROL_JUKE_LEFT: u32 = 1 << 1;
pub const CONTROL_JUKE_RIGHT: u32 = 1 << 2;
pub const CONTROL_SHOULDER: u32 = 1 << 3;
pub const CONTROL_JUMP: u32 = 1 << 4;

/// The app's observation-fact vocabulary: what the back can *see*. A fact is
/// present only when there is something to perceive, which is what lets the
/// priority live in the rule ORDER rather than in a comparison — "there is a man
/// I can run through" and "there is a man I must go round" are different
/// sightings, not one number with a threshold.
pub const FACT_AWAITING_CALL: u16 = 1;
pub const FACT_RUN_THROUGH: u16 = 2;
pub const FACT_GO_OVER: u16 = 3;
pub const FACT_CUT_LEFT: u16 = 4;
pub const FACT_CUT_RIGHT: u16 = 5;

/// Fixed-point scale: one whole unit (a yard, a yard per second) is a million.
// The drive cycle and its report moved to siblings; callers still reach them
// through `agent::`.
pub use super::agent_drive::{drive, TickReport, DEFAULT_REACTION_MILLIS};
pub use super::agent_report::{observe, AgentDecision, AgentObservation};

pub(crate) const MICRO: f32 = 1_000_000.0;

/// The stable agent id this single-runner session uses.
pub(crate) const AGENT_RAW_ID: u64 = 0x454e_445a; // "ENDZ"

/// The engine's fixed 60 Hz step delta in integer nanoseconds.
pub(crate) const FIXED_DELTA_NANOS: u64 = crate::config::FIXED_STEP_NANOS;

/// What the agent perceived this tick, before it is encoded as integer facts.
///
/// Split out so perception is testable on its own: it is a pure function of the
/// simulation and the attempt view, with no agent machinery in it.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Perception {
    /// The offense is at the line waiting for a play to be called.
    pub awaiting_call: bool,
    /// There is a man in range with enough momentum behind the runner, and he is
    /// not squared up to meet it — the charge's own conditions, read the way the
    /// resolution will read them.
    pub run_through: Option<f32>,
    /// There is a man in range the runner can go over, and the leap is ready.
    pub go_over: Option<f32>,
    /// There is a man in range coming from the runner's right (cut left) or his
    /// left (cut right).
    pub cut_left: Option<f32>,
    pub cut_right: Option<f32>,
}

/// Which move answers an encounter is decided in **time**, not distance.
///
/// The scripted brain evaluates rules in a fixed order, so if every fact were
/// present at every moment the first rule would win every time — measured, an
/// agent with the leap always available leapt at everything and threw not one
/// charge in twelve carries. Ordering cannot express "it depends when he gets
/// here", so the *facts* do: a defender is a leaping problem only once the
/// collision is close enough that the apex will coincide with it, and a charging
/// problem only when the predicted collision is winnable. Everything else is a
/// cut, which is the cheap answer you should be making most of the time.

/// Look at the field and measure what a back would need to know.
///
/// The three "can I" questions are asked against the same [`Aggression`] profile
/// the headless policy uses, so the agent's *standards* are tunable data rather
/// than a constant buried in a decision.
pub fn perceive(
    sim: &SimState,
    step: &AttemptStep,
    policy: Aggression,
    latency_ticks: u32,
) -> Perception {
    let awaiting_call = step.phase.accepts_call();
    // **Lead the encounter by your own reaction time.** This observation will be
    // acted on `latency_ticks` from now, so the collision it should be judged
    // against is that much nearer than it looks. Subtracting the latency is not
    // the agent cheating — it is the thing a person does without noticing, and
    // it is the whole difference between a decision that is right when it is
    // made and one that is still right when it lands.
    let Some(seen) = autopilot::encounter(sim, step)
        .filter(|_| step.phase.controllable() && step.runback.move_ready)
        .filter(|seen| {
            seen.contact_in_ticks
                .is_some_and(|ticks| ticks.saturating_sub(latency_ticks) <= policy.react_ticks)
        })
    else {
        return Perception {
            awaiting_call,
            ..Perception::default()
        };
    };
    // "Can I run through him?" is not asked here at all. It is answered once,
    // by the simulation, and published as `charge_window` — the SAME value that
    // lights the marker under the defender and warms the shoulder chip. The
    // agent perceives the tell the player sees, rather than a private
    // re-derivation of it that could quietly disagree.
    // How long after the press the collision actually arrives.
    let lead = seen
        .contact_in_ticks
        .unwrap_or(u32::MAX)
        .saturating_sub(latency_ticks);
    // The charge is now a window you spend, so the question is simply "is there
    // traffic worth spending it on, and do I have it?" — not "would this exact
    // collision be won", which is a question no human could answer in time.
    let run_through = step
        .runback
        .charge_window
        .filter(|_| step.runback.charge_available)
        .filter(|_| (policy.charge_lead.0..=policy.charge_lead.1).contains(&lead))
        .map(|window| window.overload);
    let go_over = (run_through.is_none()
        && policy.will_jump
        && step.runback.jump_available
        && (policy.leap_lead.0..=policy.leap_lead.1).contains(&lead))
        .then_some(seen.gap);
    let cut = (run_through.is_none() && go_over.is_none()).then_some(seen.gap);
    Perception {
        awaiting_call,
        run_through,
        go_over,
        cut_left: cut.filter(|_| seen.from_right),
        cut_right: cut.filter(|_| !seen.from_right),
    }
}
