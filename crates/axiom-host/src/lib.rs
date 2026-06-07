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
//! - The boundary that types host scale factors as the kernel
//!   [`axiom_kernel::Ratio`] quantity, which guarantees finiteness at
//!   construction.
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

mod host_adapter_request;
mod host_alpha_mode;
mod host_api;
mod host_boundary_config;
mod host_color_format;
mod host_device_profile;
mod host_device_request;
mod host_error;
mod host_error_code;
mod host_frame_input;
mod host_frame_report;
mod host_lifecycle_signal;
mod host_lifecycle_state;
mod host_present_mode;
mod host_presentation_report;
mod host_presentation_request;
mod host_presentation_status;
mod host_presentation_target;
mod host_power_preference;
mod host_result;
mod pixels;
mod host_skip_reason;
mod host_step_driver;
mod host_step_plan;
mod host_surface_descriptor;
mod host_surface_handle;
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
pub use pixels::Pixels;
pub use host_skip_reason::HostSkipReason;
pub use host_step_driver::HostStepDriver;
pub use host_step_plan::HostStepPlan;
pub use host_viewport::HostViewport;

// Presentation-boundary data types future browser/WASM adapters and a future
// axiom-webgpu live mode must be able to name. None of these contain
// browser/DOM/WebGPU objects — they are stable kernel identities and
// validated host-owned data.
pub use host_adapter_request::HostAdapterRequest;
pub use host_alpha_mode::HostAlphaMode;
pub use host_color_format::HostColorFormat;
pub use host_device_profile::HostDeviceProfile;
pub use host_device_request::HostDeviceRequest;
pub use host_present_mode::HostPresentMode;
pub use host_presentation_report::HostPresentationReport;
pub use host_presentation_request::HostPresentationRequest;
pub use host_presentation_status::HostPresentationStatus;
pub use host_presentation_target::HostPresentationTarget;
pub use host_power_preference::HostPowerPreference;
pub use host_surface_descriptor::HostSurfaceDescriptor;
pub use host_surface_handle::HostSurfaceHandle;
