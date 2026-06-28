//! # Axiom Input — Engine Module
//!
//! Owns the engine's **input contract**: the per-tick *intent snapshot* the
//! simulation reads. Raw device events are impure and arrive at presentation
//! rate; this module's sampling boundary turns them into a deterministic,
//! tick-indexed snapshot of author-defined *actions* — so gameplay reads only
//! action names against a fixed [`Tick`], never a physical key and never a
//! wall-clock event (SPEC-05 §17.3).
//!
//! ## What it folds in
//! - A guard-free **action-binding table**: [`InputState::bind_action`] maps
//!   neutral [`KeyToken`]s to an [`ActionId`]; the simulation queries the action,
//!   never the key. (The interface layer's keymap is *UI hotkey* routing with
//!   text-field/console guards — a different home; this module owns its own
//!   sim-class table and never depends on that layer.)
//! - **Keyboard edge detection** as pure tick arithmetic over down-sets:
//!   [`InputState::pressed`]/[`InputState::released`] are the set-difference of
//!   this tick's down-set against the previous tick's. Auto-repeat is suppressed
//!   structurally — an edge is a transition, not a level.
//! - **Pointer/click** ([`InputState::pointer`], [`InputState::pointer_pressed`])
//!   and the directional **swipe** ([`InputState::swipe`]) synthesized from
//!   neutral `(position, is_down)` samples.
//! - Tick-stamped press ([`InputState::pressed_at_tick`]) for rhythm/reaction
//!   timing windows.
//!
//! ## What it is / is not
//! - It **is** a pure, deterministic sampling core: the same [`DeviceFrame`]
//!   sequence reproduces byte-identical snapshots and reads, fully testable on
//!   native exactly as it drives the web.
//! - It is **not** a browser event source. It never references `web_sys` /
//!   `KeyboardEvent` / `PointerEvent` / the DOM (Module Law #9). The capture that
//!   decodes raw events into neutral [`DeviceFrame`]s lives in the
//!   platform-facing `windowing` module / host.
//!
//! ## Public surface
//! `lib.rs` exposes **exactly one** behavioural facade — [`InputState`] — plus
//! the pure `ids` value-type vocabulary that facade traffics in. Every contract
//! is reached through the facade.

mod action_id;
mod device_frame;
mod ids;
mod input_state;
mod key_token;
mod swipe_dir;
mod swipe_synth;

pub use ids::{ActionId, DeviceFrame, KeyToken, Pointer, SwipeDir, Tick};
pub use input_state::InputState;
