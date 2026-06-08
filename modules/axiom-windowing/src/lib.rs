//! # Axiom Windowing — Engine Module (deterministic presentation driver)
//!
//! The deterministic half of presentation: the part that owns *what* a window
//! presents and *when*, with no browser or GPU object in sight. It assembles a
//! validated [`axiom_host::HostPresentationRequest`] from plain viewport
//! dimensions and drives the fixed per-frame tick loop. A future, compiled-out
//! `wasm32` arm binds a real surface and issues the GPU work *behind* this core;
//! every decision the loop makes stays here, on the native-testable side.
//!
//! ## What this module is
//! - The single owner of presentation-request assembly (host surface/adapter/
//!   device boundary -> one validated, replayable request).
//! - The fixed-step run-loop driver (monotonic tick + frame counters) that
//!   `App::run` will pump, identically on native (finite/headless drive) and on
//!   the web (one tick per animation frame).
//!
//! ## What this module is not
//! Not a GPU backend, not a renderer, not a scene/world, and — in this rlib —
//! not a browser binding. It composes no other module: the per-frame draw work
//! reaches a backend as plain data, never as another module's (un-nameable)
//! contract type. The real `wgpu`/`web-sys` arm is a later, platform-gated
//! addition documented in `ARCHITECTURE.md`.
//!
//! ## Public surface
//! `lib.rs` exposes **exactly one** facade: [`WindowingApi`]. The presentation
//! request it assembles and the loop state it drives are reached only through it.

mod windowing_api;

pub use windowing_api::WindowingApi;
