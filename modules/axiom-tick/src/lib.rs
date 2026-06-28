//! # Axiom Tick — Engine Module (timers + tick-driven state machines)
//!
//! The deterministic, **wall-clock-free** half of "time & state": tick-scheduled
//! `after` / `every` / `cancel` timers and author-defined state machines. Both are
//! `sim`-class — a cooldown, a spawn cadence, and a round-phase clock all decide
//! gameplay and must replay byte-identically.
//!
//! ```text
//! kernel Tick / TickDelta time
//!   + kernel TickSchedule  ((tick, id)-ordered wake schedule)
//!     -> Timers  (after / every / cancel, ascending TimerId)
//!     -> StateMachines  ((current, entered) records, derived Enter/Update/Exit)
//! ```
//!
//! ## What this module is
//! - An *isolated* engine module depending only on the kernel. It reads **no
//!   clock**: the current [`axiom_kernel::Tick`] is supplied on every call,
//!   exactly as sim-core supplies its scheduler tick.
//! - The single owner of [`TickApi`]: timers and tick-driven state machines as
//!   pure data. The schedule and each machine's `(current, entered)` are the only
//!   state; the per-tick fired-id and event lists are derived values, not stored.
//!
//! ## What this module is not
//! It owns no domain meaning — no scene, render, asset, input, physics, audio,
//! animation, or gameplay concept — and stores **no closures**: a callback is
//! opaque code, not serializable bytes, so it cannot live in a snapshot or on the
//! wire. [`TickApi::due`] and [`TickApi::drain_events`] return data; the runtime
//! app binds that data to the author's timer / `onEnter` / `onUpdate` / `onExit`
//! closures TS-side.
//!
//! ## Public surface
//! `lib.rs` exposes **exactly one** behavioral facade — [`TickApi`] — plus its
//! **identity vocabulary**: the [`TimerId`] and [`StateMachineId`] handles the
//! facade hands out (Module Law #8). Every other type (the timer payloads, the
//! state events, the per-machine record) stays reachable only through the facade.

mod ids;
mod machines;
mod state_event;
mod state_machine;
mod tick_api;
mod timers;

pub use ids::{StateMachineId, TimerId};
pub use tick_api::TickApi;
