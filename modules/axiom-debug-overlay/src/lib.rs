//! # Axiom Debug Overlay — platform-facing engine module
//!
//! A developer debug overlay for the live browser/WASM engine surface, plus a
//! tiny in-overlay command console. Toggled by the physical Backquote (`` ` ``)
//! key; it shows live frame/fps/backend read-outs and routes typed commands
//! through a real registry.
//!
//! ## What this module is
//! - A pure, native-testable, **branchless** overlay *state machine* composed on
//!   [`axiom_interface`]: the generic windowing (visibility / pin / focus / drag,
//!   the console model, the command-dispatch shape, the neutral draw list) lives
//!   in that layer; this module adds the debug-specific density, the diagnostics
//!   read-out, the debug command set, and the `` ` `` Backquote binding. It takes
//!   host-supplied facts (as primitives) and produces what to render.
//! - A thin `wasm32` arm ([`dom_binding`]) that renders the layer's
//!   `InterfaceDrawList` onto real DOM nodes and wires the browser keyboard/pointer
//!   events back into it.
//!
//! ## What this module is not
//! Not engine state. The overlay only ever *reads* diagnostics handed in through
//! the facade — it is a read-out, never a source of deterministic engine state,
//! and it knows nothing about scenes, the run loop, or any rendering backend by
//! name. The host (an app) aggregates real engine facts and pushes them in.
//!
//! ## Diagnostics cross the facade as primitives
//! Two modules can never share a Rust type, so the facade takes diagnostics as
//! plain values, never a shared contract type. Timing crosses as **integers**
//! (`fps_milli`, `frame_time_micros`) — there are no naked floats in the public
//! API, and the overlay formats the integers for display.
//!
//! This is a sanctioned platform-facing module (Module Law #9): its real
//! `web-sys` arm is compiled only for `wasm32`, behind the native-clean facade,
//! and never enters the native build or the coverage gate.
//!
//! ## Public surface
//! `lib.rs` exposes **exactly one** facade: [`DebugOverlayApi`].

mod backquote;
mod diagnostics;
mod overlay_commands;
mod overlay_density;
mod overlay_state;

mod overlay_api;

#[cfg(target_arch = "wasm32")]
mod dom_binding;

pub use overlay_api::DebugOverlayApi;
