//! The **attempt loop** — the game layer on top of the football simulation.
//!
//! One attempt is one carry: call it, watch it come to you, take the exchange,
//! and survive the run. It resets straight into the next one, so ten attempts
//! fit in a couple of minutes.
//!
//! ```text
//! PlayCall ──call 1|2|3──▶ Shifting ──offense set──▶ Mesh ──they meet──▶ Exchange
//!                                                                          │
//!                                       ball lands in the back's hands ────┤
//!                                                                          ▼
//!    PlayCall ◀── Resetting ◀── Result ◀── Resolving ◀── tackle / TD ── Carrying
//! ```
//!
//! Both ends of the pre-snap belong to the player. `PlayCall` has **no clock**:
//! it waits for a play to be called, however long that takes. `Shifting` then
//! ends on a *fact about the field* — every offensive player standing on his
//! spot — rather than on a timer, so the snap is always the consequence of the
//! offense being ready. The **exchange** follows the same rule: it happens when
//! the quarterback and the back are genuinely together, not when a counter says
//! so, which is why it always looks like two people meeting.
//!
//! Everything football-specific stays here in the app: what the concepts are,
//! when a handoff is legal, how a carry is measured. The simulation, the AI, the
//! ball state machine, the contact framework and the presentation stack
//! underneath are all the app's existing systems.
//!
//! Four owners: [`AttemptPhase`] (the explicit state, in [`phase`]),
//! [`AttemptController`] (the loop that drives the simulation, in
//! [`controller`]), [`AttemptLedger`] (what happened, in [`ledger`]), and
//! [`AttemptStep`] (what presentation may see, in [`view`]). The `SimState`
//! mutators the loop needs live in [`sim_support`].

pub mod call;
pub mod controller;
pub mod ledger;
pub mod phase;
mod pre_snap;
mod setup;
mod sim_support;
pub mod view;

pub use controller::AttemptController;
pub use ledger::{AttemptLedger, AttemptOutcome, AttemptRecord, SessionSummary};
pub use phase::AttemptPhase;
pub use view::AttemptStep;

// --- attempt timing (all in 60 Hz simulation ticks) ---------------------------

/// The stall guard on the shift, in ticks (~2.5 s).
///
/// **Not a deadline.** The ball is meant to snap once the offense is set (see
/// `setup::offense_is_set`), and in practice always does. It exists purely so a
/// player wedged against a body — or a formation change nobody can complete —
/// can never hang an attempt that is otherwise unbounded on both ends.
pub const SHIFT_STALL_TICKS: u64 = 150;

/// The earliest the exchange may happen after the snap (~0.2 s).
///
/// It is a floor on the *snap*, not on the handoff: the ball takes
/// `snap_ticks` to reach the quarterback's hands, and a handoff ordered before
/// it lands would be asking him to give away something he does not have. A few
/// ticks past that so the open step is visible.
pub const HANDOFF_EARLIEST_TICKS: u64 = 14;

/// How long the mesh may go unresolved before the loop stops asking (~2 s).
///
/// Past it the quarterback simply keeps the ball, and the play resolves however
/// the field decides — almost always a sack, which is the honest outcome of a
/// muffed exchange. The loop never forces a handoff that the field refused.
pub const MESH_DEADLINE_TICKS: u64 = 120;

/// How long the result card holds before the next attempt (~0.9 s).
pub const RESULT_TICKS: u64 = 54;

/// Hard cap on one attempt's live ticks (~12 s). A play that somehow never
/// resolves is blown dead, so the loop can never stall. Longer than the pass
/// game's cap because a broken run genuinely can take that long to finish.
pub const MAX_LIVE_TICKS: u64 = 720;

/// Time dilation the run game runs at. **1.0 — the game never slows down.**
///
/// See [`AttemptPhase::time_scale`] for why, and for what setting it below 1.0
/// would still wake up.
pub const RUN_TIME_SCALE: f32 = 1.0;

/// The fixed defensive aggression the run game runs at. It never escalates —
/// this is a game about executing three moves well, not a difficulty curve.
///
/// It is 4 rather than a middling 2 because `launch::heat_profile` is a
/// *difficulty* curve, not a linear one: at heat 2 the reaction-delay scale is
/// **1.32**, i.e. the defense reacts a third SLOWER than the archetype
/// baseline. Heat 4 puts reaction at ~0.96 and pursuit at ~1.06 — a defense
/// that plays at its listed ability rather than one handicapped by the dial's
/// midpoint.
pub const RUN_HEAT: u8 = 4;

/// The distance one attempt is played to, yards downfield of the line of
/// scrimmage.
///
/// One value, two consumers: the defensive selector is called against it (see
/// `setup`), and the field paints its line to gain there. Keeping them the same
/// constant is the point — a line drawn at a distance the defense was not
/// called against would be lying to the player.
pub const ATTEMPT_DISTANCE: f32 = 10.0;
