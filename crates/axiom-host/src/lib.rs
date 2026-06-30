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
//! construct, match on). This includes the backend-neutral 2D draw contract
//! ([`Draw2dList`] and the value vocabulary it carries), relocated here from
//! the `axiom-draw2d` module so the render backends that depend on host can
//! name and rasterize it — the same role [`FramePacket`] already plays. Host
//! owns the *contract*, not a renderer: it rasterizes nothing. The curated set
//! is locked down by `tests/architecture.rs::lib_exports_are_curated_set` —
//! any accidental widening fails the build.

mod camera2d;
mod common2d;
mod draw2d_command;
mod draw2d_list;
mod fill2d;
mod frame_packet;
mod frame_raster_stats;
mod frame_submission_report;
mod handles;
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
mod host_metrics;
mod host_orientation;
mod host_outcome;
mod host_outcome_set;
mod host_param_value;
mod host_power_preference;
mod host_present_mode;
mod host_presentation_report;
mod host_presentation_request;
mod host_presentation_status;
mod host_presentation_target;
mod host_result;
mod host_safe_area_insets;
mod host_session_config;
mod host_session_params;
mod host_skip_reason;
mod host_step_driver;
mod host_step_plan;
mod host_surface_descriptor;
mod host_surface_handle;
mod host_viewport;
mod paint;
mod pixels;
mod player_id;
mod rect;
mod rgba;
mod score;
mod sdf_scene;
mod sprite_draw2d;
mod text2d;

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
pub use host_orientation::Orientation;
pub use host_result::HostResult;
pub use host_safe_area_insets::HostSafeAreaInsets;
pub use host_skip_reason::HostSkipReason;
pub use host_step_driver::HostStepDriver;
pub use host_step_plan::HostStepPlan;
pub use host_viewport::HostViewport;
pub use pixels::Pixels;

// Embed-seam boundary data types (SPEC-12): the inbound session identity and
// the outbound terminal outcome the platform arm decodes/forwards. Primitive-
// only, browser-free — the same discipline as every other host boundary type.
// `Score` is the single sanctioned f64 boundary (a quantity newtype, like
// `Pixels`); no naked float appears elsewhere on this surface.
pub use host_metrics::HostMetrics;
pub use host_outcome::HostOutcome;
pub use host_outcome_set::HostOutcomeSet;
pub use host_param_value::HostParamValue;
pub use host_session_config::HostSessionConfig;
pub use host_session_params::HostSessionParams;
pub use player_id::PlayerId;
pub use score::Score;

// Presentation-boundary data types future browser/WASM adapters and a future
// axiom-webgpu live mode must be able to name. None of these contain
// browser/DOM/WebGPU objects — they are stable kernel identities and
// validated host-owned data.
pub use host_adapter_request::HostAdapterRequest;
pub use host_alpha_mode::HostAlphaMode;
pub use host_color_format::HostColorFormat;
pub use host_device_profile::HostDeviceProfile;
pub use host_device_request::HostDeviceRequest;
pub use host_power_preference::HostPowerPreference;
pub use host_present_mode::HostPresentMode;
pub use host_presentation_report::HostPresentationReport;
pub use host_presentation_request::HostPresentationRequest;
pub use host_presentation_status::HostPresentationStatus;
pub use host_presentation_target::HostPresentationTarget;
pub use host_surface_descriptor::HostSurfaceDescriptor;
pub use host_surface_handle::HostSurfaceHandle;

// Backend-neutral frame presentation packet + uniform submission report. The
// single artifact every render backend (GPU now, Canvas 2D later) consumes /
// returns. Primitive-only, browser/GPU-free — derived from a render command
// list by axiom-render. See frame_packet.rs / frame_submission_report.rs.
pub use frame_packet::FrameCamera;
pub use frame_packet::FrameDrawItem;
pub use frame_packet::FrameFeatureSet;
pub use frame_packet::FrameLight;
pub use frame_packet::FramePacket;
pub use frame_packet::FrameViewport;
pub use frame_raster_stats::FrameDepthCueStats;
pub use frame_raster_stats::FrameRasterStats;
pub use frame_submission_report::BackendKind;
pub use frame_submission_report::FrameFeature;
pub use frame_submission_report::FrameSubmissionReport;

// Backend-neutral SDF raymarch contract: the raymarch peer of FramePacket's
// triangle draws, carried as an optional FramePacket arm. Both render backends
// (GPU now, Canvas 2D) march the same primitive-only data. See sdf_scene.rs.
pub use sdf_scene::SdfPrimitive;
pub use sdf_scene::SdfScene;

// Backend-neutral 2D draw contract (SPEC-04), relocated here from the
// axiom-draw2d module so both render backends (Canvas 2D, GPU) — which already
// depend on host — can name and rasterize it, exactly as they name FramePacket.
// axiom-draw2d keeps only the Draw2dApi *builder*, which assembles these
// host-owned types through their producer constructors. Primitive-only — no
// GPU/DOM/font/scene types.
pub use camera2d::Camera2d;
pub use common2d::Common2d;
pub use common2d::Shadow2d;
pub use draw2d_command::Draw2dCommand;
pub use draw2d_list::Draw2dList;
pub use fill2d::Fill2d;
pub use fill2d::Stroke2d;
pub use handles::FontHandle;
pub use handles::PaintId;
pub use handles::RenderTargetId;
pub use handles::TextureId;
pub use handles::TransformDepth;
pub use paint::GradientStop;
pub use rect::Rect;
pub use rgba::Rgba;
pub use sprite_draw2d::SpriteDraw2d;
pub use text2d::Glyph2d;
pub use text2d::GlyphRun;
pub use text2d::TextAlign;
pub use text2d::TextDraw2d;
pub use text2d::TextMetrics;
