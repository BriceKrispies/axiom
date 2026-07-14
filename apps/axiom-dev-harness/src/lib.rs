//! # Axiom — Browser Debug Overlay developer harness
//!
//! A thin host for the `axiom_debug_overlay` module: it mounts the overlay +
//! command console over a bare canvas. The overlay's whole state machine
//! (density, command registry, console history, keyboard classification), its
//! DOM binding, **and** the measured-diagnostics driver (fps / frame time from
//! `requestAnimationFrame` deltas, the RAF frame counter, live document
//! visibility, and the real `navigator.gpu` backend probe) live in the module,
//! behind the `DebugOverlayApi` facade — this app only mounts it via
//! `DebugOverlayApi::mount_with_measured_diagnostics`.
//!
//! All of that is the nondeterministic browser edge, so it is confined to the
//! `wasm32` [`web`] module; native `cargo test` compiles nothing here.

#[cfg(target_arch = "wasm32")]
mod web;
