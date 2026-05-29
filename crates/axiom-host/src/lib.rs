//! # Axiom Host — Layer 03
//!
//! The deterministic platform/host boundary. Validates externally supplied
//! host facts (viewport metadata, frame pulses, lifecycle signals) and
//! adapts them into deterministic runtime stepping plans and reports.
//!
//! ## What this layer is
//! - The boundary between deterministic engine code and future, possibly
//!   nondeterministic host integrations (browser, native window, headless).
//! - The single place that calls [`axiom_runtime::Runtime::step`] in response
//!   to a host frame pulse.
//! - The single place that consumes [`axiom_math::MathApi`] to validate
//!   finite host scalar inputs.
//!
//! ## What this layer is not
//! Not a browser adapter, not a renderer, not a renderer abstraction, not
//! WebGPU/WebGL, not an ECS/world, not scenes, not assets, not physics, not
//! input mapping, not animation, not audio, not plugins, not editor/tooling
//! UI, not the real browser game loop. It also does not call browser/DOM,
//! `requestAnimationFrame`, `performance.now`, `std::time`, randomness, or
//! any other ambient API — every nondeterministic value enters as explicit
//! data on a [`HostFrameInput`].
//!
//! ## Public surface
//! `lib.rs` exposes the [`HostApi`] facade plus the curated set of host
//! boundary data types future adapters must be able to *name* (store,
//! construct, match on). The curated set is locked down by
//! `tests/architecture.rs::lib_exports_are_curated_set` — any accidental
//! widening fails the build.

mod host_api;
mod host_boundary_config;
mod host_error;
mod host_error_code;
mod host_frame_input;
mod host_frame_report;
mod host_lifecycle_signal;
mod host_lifecycle_state;
mod host_result;
mod host_skip_reason;
mod host_step_driver;
mod host_step_plan;
mod host_viewport;

// --- Curated public surface ---

// Primary facade.
pub use host_api::HostApi;

// Host boundary data types future adapters must be able to name.
pub use host_boundary_config::HostBoundaryConfig;
pub use host_error::HostError;
pub use host_error_code::HostErrorCode;
pub use host_frame_input::HostFrameInput;
pub use host_frame_report::HostFrameReport;
pub use host_lifecycle_signal::HostLifecycleSignal;
pub use host_lifecycle_state::HostLifecycleState;
pub use host_result::HostResult;
pub use host_skip_reason::HostSkipReason;
pub use host_step_driver::HostStepDriver;
pub use host_step_plan::HostStepPlan;
pub use host_viewport::HostViewport;
