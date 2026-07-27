//! The **decision-window attempt loop** — the prototype's gameplay layer.
//!
//! It replaces the old down-and-distance drive loop with a much tighter idea:
//! the football play *simulates itself*, and the player intervenes only at the
//! moment the read is worth making. One attempt is ~8–12 seconds end to end and
//! resets straight into the next one, so ten attempts fit in a couple of
//! minutes.
//!
//! ```text
//! PreSnap ──auto snap──▶ Developing ──trigger──▶ DecisionWindow
//!                            ▲                        │
//!                            └──── no choice ─────────┤ (slow-motion closes,
//!                                                     │  the rush keeps coming)
//!                       ┌── throw 1|2|3 ──────────────┤
//!                       │                             └── scramble ──┐
//!                       ▼                                            ▼
//!                  PassInFlight ───┐                            Scrambling
//!                                  ├──▶ Resolving ──▶ Result ──▶ Resetting ──▶ PreSnap
//!                                  └────────────────────────────────────┘
//! ```
//!
//! Everything football-specific stays here in the app: what the reads are, when
//! a window is worth opening, how a choice becomes a throw. The simulation, the
//! AI, the ball state machine, the contact framework and the presentation stack
//! underneath are all the app's existing systems, untouched.
//!
//! Four owners: [`AttemptPhase`] (the explicit state, in [`phase`]),
//! [`PlayRead`] (what the player is being asked to judge, in [`read`]),
//! [`AttemptController`] (the loop that drives the simulation, in
//! [`controller`]), and [`AttemptLedger`] (what happened, in [`ledger`]). The
//! `SimState` mutators the loop needs live in [`sim_support`].

pub mod call;
pub mod controller;
pub mod ledger;
pub mod phase;
pub mod read;
mod setup;
mod sim_support;
pub mod view;

pub use controller::AttemptController;
pub use ledger::{AttemptLedger, AttemptOutcome, AttemptRecord, SessionSummary};
pub use phase::{window_length, AttemptPhase, PlayerChoice, WindowTrigger};
pub use read::{read_play, window_trigger, PlayRead, ReadState, WindowGate};
pub use view::AttemptStep;

// --- attempt timing (all in 60 Hz simulation ticks) ---------------------------

/// How long the offense stands set before the ball snaps itself (~0.8 s). Long
/// enough to see the formation and the coverage; short enough that a reset is
/// not a wait.
pub const SET_TICKS: u64 = 50;

/// The earliest a decision window may open after the snap (~1.1 s). Before
/// this, nothing has developed and there is nothing to read.
pub const DEVELOP_MIN_TICKS: u64 = 66;

/// The snap-relative deadline at which the first window opens regardless of what
/// the read looks like (~2.6 s). This is what makes the window **reliable**: no
/// attempt can ever run without offering at least one decision.
pub const DEVELOP_MAX_TICKS: u64 = 156;

/// How long a window stays open, in simulation ticks. At
/// [`DECISION_TIME_SCALE`] this is ~3.6 real seconds for the first window —
/// enough to actually look at the coverage, pick a receiver and press a key,
/// including on a touch screen where the "key" is a thumb travelling across
/// the display. The first pass at 20 ticks / 0.16× (~2.1 s) read as a reflex
/// test rather than a decision.
pub const WINDOW_TICKS: u64 = 28;

/// Every window after the first is this many ticks shorter — declining a read
/// costs time as well as field position, so the third look is a snap judgement.
pub const WINDOW_DECAY_TICKS: u64 = 6;

/// The fewest ticks any window stays open, however late it is (~2.0 s). The
/// last look is meant to be rushed, not impossible.
pub const WINDOW_MIN_TICKS: u64 = 16;

/// Windows one attempt may offer before the quarterback is on his own. After
/// the last one closes the play still runs — the rush simply gets home.
pub const MAX_WINDOWS: u32 = 3;

/// Full-speed ticks between a window closing and the next one arming. The play
/// visibly runs on at normal speed in between, which is what makes declining a
/// read feel like a decision rather than a menu dismissal.
pub const WINDOW_COOLDOWN_TICKS: u64 = 20;

/// How long after a window closes the next one opens no matter what.
pub const REARM_DEADLINE_TICKS: u64 = 48;

/// Time dilation while a decision window is open. Not a pause: the rush keeps
/// closing, the routes keep running and the coverage keeps rotating — just
/// slowly enough to read.
///
/// This buys reading time far more cheaply than a longer window does: dilation
/// costs the player only real seconds, while extra `WINDOW_TICKS` also let the
/// rush get closer. The two are tuned together — the window durations quoted
/// above are `ticks / (60 * DECISION_TIME_SCALE)` seconds:
/// **3.6 s → 2.8 s → 2.1 s** across the three looks.
pub const DECISION_TIME_SCALE: f32 = 0.13;

/// How long the result card holds before the next attempt (~0.9 s).
pub const RESULT_TICKS: u64 = 54;

/// Hard cap on one attempt's live ticks (~9 s). A play that somehow never
/// resolves is blown dead, so the loop can never stall.
pub const MAX_LIVE_TICKS: u64 = 540;

/// The fixed defensive aggression the prototype runs at. It never escalates —
/// this prototype tests a decision, not a difficulty curve.
///
/// It is 4 rather than a middling 2 because `launch::heat_profile` is a
/// *difficulty* curve, not a linear one: at heat 2 the reaction-delay scale is
/// **1.32**, i.e. the coverage reacts a third SLOWER than the archetype
/// baseline, which is why receivers used to be run down by nobody. Heat 4 puts
/// reaction at ~0.96 and pursuit at ~1.06 — a defense that plays at its listed
/// ability rather than one handicapped by the dial's midpoint.
pub const PROTOTYPE_HEAT: u8 = 4;

/// The distance one attempt is played to, yards downfield of the line of
/// scrimmage.
///
/// One value, two consumers: the defensive selector is called against it (see
/// `setup`), and the field paints its line to gain there (see
/// [`crate::presentation::snapshot`]). Keeping them the same constant is the
/// point — a line drawn at a distance the defense was not called against would
/// be lying to the player.
pub const ATTEMPT_DISTANCE: f32 = 10.0;
