//! # Axiom Frame — Layer 04
//!
//! The canonical engine frame boundary. Adapts host frame reports
//! (Layer 03), runtime step records (Layer 01), viewport facts
//! (Layer 03), and math primitives (Layer 02) into a stable per-frame
//! contract — the [`EngineFrame`] and [`FrameContext`] every future
//! engine system reads from.
//!
//! ## What this layer is
//! - The single place where a [`axiom_host::HostFrameReport`] is turned
//!   into an authoritative immutable per-frame value.
//! - The single place where viewport / lifecycle / timing / diagnostics
//!   are snapped onto the engine frame.
//! - The single place where frame-local commands are queued
//!   deterministically.
//!
//! ## What this layer is not
//! Not a browser adapter, not a renderer, not a render graph, not
//! WebGPU/WebGL, not an ECS/world, not scenes, not assets, not physics,
//! not input mapping, not animation, not audio, not plugins, not
//! editor/tooling UI, not the real browser game loop. It also does not
//! call browser/DOM, `requestAnimationFrame`, `performance.now`,
//! `std::time`, randomness, or any other ambient API — every value
//! enters as explicit data through the host boundary above it.
//!
//! ## Public surface
//! `lib.rs` exposes the [`FrameApi`] facade plus the curated set of
//! frame-boundary data types future engine systems must be able to
//! *name*. The curated set is locked down by
//! `tests/architecture.rs::lib_exports_are_curated_set` — any
//! accidental widening fails the build.

mod engine_frame;
mod frame_api;
mod frame_builder;
mod frame_command;
mod frame_command_queue;
mod frame_context;
mod frame_diagnostics;
mod frame_error;
mod frame_error_code;
mod frame_lifecycle_state;
mod frame_result;
mod frame_step_summary;
mod frame_timing;
mod frame_viewport;

// --- Curated public surface ---

// Primary facade.
pub use frame_api::FrameApi;

// Frame boundary data types future engine systems must be able to name.
pub use engine_frame::EngineFrame;
pub use frame_builder::FrameBuilder;
pub use frame_command::FrameCommand;
pub use frame_command_queue::FrameCommandQueue;
pub use frame_context::FrameContext;
pub use frame_diagnostics::FrameDiagnostics;
pub use frame_error::FrameError;
pub use frame_error_code::FrameErrorCode;
pub use frame_lifecycle_state::FrameLifecycleState;
pub use frame_result::FrameResult;
pub use frame_step_summary::FrameStepSummary;
pub use frame_timing::FrameTiming;
pub use frame_viewport::FrameViewport;
