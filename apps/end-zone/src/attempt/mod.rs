//! The **decision-window attempt loop** — the prototype's gameplay layer.
//!
//! It replaces the old down-and-distance drive loop with a much tighter idea:
//! the football play *simulates itself*, and the player intervenes only at the
//! moment the read is worth making. One attempt is ~8–12 seconds end to end and
//! resets straight into the next one, so ten attempts fit in a couple of
//! minutes.
//!
//! ```text
//! PlayCall ──call 1|2|3──▶ Shifting ──offense set──▶ Developing ──trigger──▶ DecisionWindow
//!                                                        ▲                        │
//!                                                        └──── no choice ─────────┤
//!                                                                                 │
//!                                   ┌── throw 1|2|3 ────────────────────────────  ┤
//!                                   │                             └── scramble ──┐
//!                                   ▼                                            ▼
//!                              PassInFlight ───┐                            Scrambling
//!                                              ├──▶ Resolving ──▶ Result ──▶ Resetting
//!                                              └──────────────────────┬───────────┘
//!                                                                     ▼
//!                                                                  PlayCall
//! ```
//!
//! Both ends of the pre-snap belong to the player. `PlayCall` has **no clock**:
//! it waits for a play to be called, however long that takes. `Shifting` then
//! ends on a *fact about the field* — every offensive player standing on his
//! spot — rather than on a timer, so the snap is always the consequence of the
//! offense being ready.
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
pub mod in_flight;
pub mod ledger;
pub mod phase;
mod pre_snap;
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

/// The stall guard on the shift, in ticks (~2.5 s).
///
/// **Not a deadline.** The ball is meant to snap once the offense is set (see
/// `setup::offense_is_set`), and in practice always does: the offense sprints
/// into its new alignment and the longest walk on the field finishes well
/// inside this. It exists purely so a player wedged against a body — or a
/// formation change nobody can complete — can never hang an attempt that is now
/// otherwise unbounded on both ends (the call has no clock either). If this
/// fires, someone is late, which is exactly what happens on a real field.
pub const SHIFT_STALL_TICKS: u64 = 150;

/// The earliest a decision window may open after the snap (~1.1 s). Before
/// this, nothing has developed and there is nothing to read.
pub const DEVELOP_MIN_TICKS: u64 = 66;

/// The snap-relative deadline at which the first window opens regardless of what
/// the read looks like (~2.6 s). This is what makes the window **reliable**: no
/// attempt can ever run without offering at least one decision.
pub const DEVELOP_MAX_TICKS: u64 = 156;

/// How long a window stays open, in simulation ticks. Time is NOT dilated (see
/// [`DECISION_TIME_SCALE`]), so a tick is 1/60 s of real time: 90 → 1.5 s for
/// the first look, then 1.1 s, then 0.8 s.
///
/// Shorter than the dilated version was, and necessarily so: at full speed the
/// window costs the offense real GAME time, so a long one just hands the pass
/// rush a free sack. The window is now a prompt, not a pause.
pub const WINDOW_TICKS: u64 = 90;

/// Every window after the first is this many ticks shorter — declining a read
/// costs time as well as field position, so the third look is a snap judgement.
pub const WINDOW_DECAY_TICKS: u64 = 24;

/// The fewest ticks any window stays open, however late it is (~0.8 s). The
/// last look is meant to be rushed, not impossible.
pub const WINDOW_MIN_TICKS: u64 = 48;

/// Windows one attempt may offer before the quarterback is on his own. After
/// the last one closes the play still runs — the rush simply gets home.
pub const MAX_WINDOWS: u32 = 3;

/// Full-speed ticks between a window closing and the next one arming. The play
/// visibly runs on at normal speed in between, which is what makes declining a
/// read feel like a decision rather than a menu dismissal.
pub const WINDOW_COOLDOWN_TICKS: u64 = 20;

/// How long after a window closes the next one opens no matter what.
pub const REARM_DEADLINE_TICKS: u64 = 48;

/// Time dilation while a decision window is open. **1.0 — off.**
///
/// The slow-motion beat was built, shipped, and then deliberately switched off:
/// the design moved away from slowing the game down. The machinery is intact
/// and still compiled — the fractional tick-credit stepping in
/// [`crate::app::EndZoneApp::advance`] and the render interpolation in
/// [`crate::presentation::interpolate`] are both keyed on `time_scale < 1.0`,
/// so setting this back to `0.13` wakes the whole path up in one edit.
///
/// If you do, re-tune [`WINDOW_TICKS`] and friends: they are in TICKS, so their
/// real duration is `ticks / (60 * scale)`, and dilating without re-tuning
/// shortens every window by the dilation factor.
///
/// The technique, and the four rules that stop it looking like frame stutter,
/// are written up in `docs/time-dilation-and-render-interpolation.md`.
pub const DECISION_TIME_SCALE: f32 = 1.0;

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
