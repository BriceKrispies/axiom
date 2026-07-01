//! The Layer-03 host boundary facade.

use axiom_kernel::KernelApi;
use axiom_kernel::Ratio;

use crate::host_adapter_request::HostAdapterRequest;
use crate::host_alpha_mode::HostAlphaMode;
use crate::host_boundary_config::HostBoundaryConfig;
use crate::host_color_format::HostColorFormat;
use crate::host_device_profile::HostDeviceProfile;
use crate::host_device_request::HostDeviceRequest;
use crate::host_frame_input::HostFrameInput;
use crate::host_frame_report::HostFrameReport;
use crate::host_lifecycle_signal::HostLifecycleSignal;
use crate::host_lifecycle_state::HostLifecycleState;
use crate::host_metrics::HostMetrics;
use crate::host_outcome::HostOutcome;
use crate::host_outcome_set::HostOutcomeSet;
use crate::host_power_preference::HostPowerPreference;
use crate::host_present_mode::HostPresentMode;
use crate::host_presentation_report::HostPresentationReport;
use crate::host_presentation_request::HostPresentationRequest;
use crate::host_presentation_target::HostPresentationTarget;
use crate::host_result::HostResult;
use crate::host_session_config::HostSessionConfig;
use crate::host_session_params::HostSessionParams;
use crate::host_step_driver::HostStepDriver;
use crate::host_step_plan::HostStepPlan;
use crate::host_surface_descriptor::HostSurfaceDescriptor;
use crate::host_surface_handle::HostSurfaceHandle;
use crate::host_viewport::HostViewport;
use crate::player_id::PlayerId;
use crate::score::Score;

/// The primary entry point to the Axiom host boundary.
///
/// `HostApi` is a zero-sized facade. It exposes the constructors a future
/// browser/native adapter will use to build validated host boundary data —
/// viewports, frame inputs, lifecycle state, boundary configs, step plans,
/// and step drivers — and offers a deterministic "report a skipped frame"
/// helper used by adapters that need to surface a non-stepping plan without
/// touching the runtime.
///
/// Viewport scale factors arrive as the kernel [`Ratio`] quantity type, which
/// already guarantees finiteness at its boundary; the facade only enforces the
/// host's positivity and dimension invariants on top.
#[derive(Debug, Clone, Copy, Default)]
pub struct HostApi {
    _sealed: (),
}

impl HostApi {
    /// Construct the facade.
    pub const fn new() -> Self {
        HostApi { _sealed: () }
    }

    /// Construct a validated viewport from a logical size and a scale
    /// factor. The scale factor is a finite kernel [`Ratio`].
    pub fn viewport(
        &self,
        logical_width: u32,
        logical_height: u32,
        scale_factor: Ratio,
    ) -> HostResult<HostViewport> {
        HostViewport::new(logical_width, logical_height, scale_factor)
    }

    /// Construct a validated viewport from a physical size and a scale
    /// factor. The scale factor is a finite kernel [`Ratio`].
    pub fn viewport_from_physical(
        &self,
        physical_width: u32,
        physical_height: u32,
        scale_factor: Ratio,
    ) -> HostResult<HostViewport> {
        HostViewport::from_physical(physical_width, physical_height, scale_factor)
    }

    /// Construct a host frame input from explicit integer timing values.
    /// The host supplies every timestamp; nothing is read from a clock.
    pub fn frame_input(
        &self,
        sequence: u64,
        elapsed_nanos: u64,
        viewport: HostViewport,
    ) -> HostFrameInput {
        HostFrameInput::new(sequence, elapsed_nanos, viewport)
    }

    /// The initial host lifecycle state (nothing observed yet).
    pub const fn lifecycle_initial(&self) -> HostLifecycleState {
        HostLifecycleState::initial()
    }

    /// Apply one signal to a lifecycle state.
    pub const fn apply_lifecycle_signal(
        &self,
        state: HostLifecycleState,
        signal: HostLifecycleSignal,
    ) -> HostLifecycleState {
        state.apply(signal)
    }

    /// Construct a host boundary config. Rejects zero `max_steps_per_frame`.
    pub const fn boundary_config(
        &self,
        fixed_step_nanos: u64,
        max_steps_per_frame: u32,
    ) -> HostResult<HostBoundaryConfig> {
        HostBoundaryConfig::new(fixed_step_nanos, max_steps_per_frame)
    }

    /// Validate a host boundary config against a kernel facade. Returns
    /// `InvalidBoundaryConfig` if the kernel rejects the fixed step.
    pub fn validate_boundary_config(
        &self,
        config: &HostBoundaryConfig,
        kernel: &KernelApi,
    ) -> HostResult<()> {
        config.validate(kernel)
    }

    /// Construct a step driver around a validated boundary config.
    pub fn step_driver(&self, config: HostBoundaryConfig) -> HostStepDriver {
        HostStepDriver::new(config)
    }

    /// Compute a step plan for the given inputs, without touching a
    /// runtime. Pure and deterministic.
    pub fn plan_frame(
        &self,
        input: &HostFrameInput,
        config: &HostBoundaryConfig,
        lifecycle: &HostLifecycleState,
        accumulator_nanos: u64,
    ) -> HostStepPlan {
        HostStepPlan::build(input, config, lifecycle, accumulator_nanos)
    }

    /// Produce a frame report for a host frame that did not require any
    /// runtime stepping (e.g. a skipped lifecycle frame). The report
    /// contains zero step records.
    pub fn report_no_step_frame(
        &self,
        input: &HostFrameInput,
        plan: HostStepPlan,
        lifecycle_after: HostLifecycleState,
    ) -> HostFrameReport {
        HostFrameReport::new(
            input.sequence(),
            plan,
            0,
            Vec::new(),
            *input.viewport(),
            lifecycle_after,
        )
    }

    // Deterministic surface/adapter/device/presentation boundary: handles are
    // stable kernel identities, and nothing here touches a real GPU, window, or
    // DOM object.

    /// Mint a validated [`HostPresentationTarget`]. The handle id is built
    /// through the kernel facade; a null id or empty label is rejected.
    pub fn presentation_target(
        &self,
        kernel: &KernelApi,
        raw_id: u64,
        label: &'static str,
    ) -> HostResult<HostPresentationTarget> {
        HostPresentationTarget::new(kernel.handle_id(raw_id), label)
    }

    /// Mint an opaque [`HostSurfaceHandle`]. The handle id is built through
    /// the kernel facade; a null id is rejected.
    pub fn surface_handle(&self, kernel: &KernelApi, raw_id: u64) -> HostResult<HostSurfaceHandle> {
        HostSurfaceHandle::new(kernel.handle_id(raw_id))
    }

    /// Describe a surface shape from an already-validated viewport and the
    /// abstract present/alpha/colour enums. Dimension and scale validity is
    /// carried by the [`HostViewport`] (build it via [`Self::viewport`]).
    pub fn surface_descriptor(
        &self,
        viewport: HostViewport,
        present_mode: HostPresentMode,
        alpha_mode: HostAlphaMode,
        color_format: HostColorFormat,
    ) -> HostSurfaceDescriptor {
        HostSurfaceDescriptor::new(viewport, present_mode, alpha_mode, color_format)
    }

    /// Construct an adapter request (pure data; every combination valid).
    pub fn adapter_request(
        &self,
        power_preference: HostPowerPreference,
        require_presentation_surface: bool,
    ) -> HostAdapterRequest {
        HostAdapterRequest::new(power_preference, require_presentation_surface)
    }

    /// Construct a device request (pure data; every combination valid).
    pub fn device_request(
        &self,
        require_presentation: bool,
        profile: HostDeviceProfile,
    ) -> HostDeviceRequest {
        HostDeviceRequest::new(require_presentation, profile)
    }

    /// Validate and bind a presentation request from its parts. Rejects a
    /// missing target/surface or an inconsistent adapter/device pairing.
    pub fn presentation_request(
        &self,
        target: HostPresentationTarget,
        surface: HostSurfaceHandle,
        descriptor: HostSurfaceDescriptor,
        adapter: HostAdapterRequest,
        device: HostDeviceRequest,
    ) -> HostResult<HostPresentationRequest> {
        HostPresentationRequest::new(target, surface, descriptor, adapter, device)
    }

    /// Evaluate a validated presentation request into a deterministic
    /// report. This pass has no live backend, so the report's status is
    /// always [`crate::HostPresentationStatus::PendingBackend`] — it never
    /// claims a real GPU exists.
    pub fn evaluate_presentation(
        &self,
        request: &HostPresentationRequest,
    ) -> HostPresentationReport {
        HostPresentationReport::from_request(request)
    }

    // The facade only carries already-decoded data (SPEC-12): it never reads a
    // URL, a clock, or `localStorage` itself.

    /// Carry a session config from a `seed` and already-decoded opaque `params`.
    /// The `seed` is the determinism input fixed before tick 0 (SPEC-12 §6).
    pub fn session_config(&self, seed: u64, params: HostSessionParams) -> HostSessionConfig {
        HostSessionConfig::new(seed, params)
    }

    /// Mint a terminal outcome with no metrics. `score` rides the [`Score`]
    /// boundary newtype, so no naked `f64` crosses the facade.
    pub fn outcome(&self, won: bool, score: Score) -> HostOutcome {
        HostOutcome::new(won, score, HostMetrics::new())
    }

    /// Mint a terminal outcome carrying named metrics.
    pub fn outcome_with_metrics(
        &self,
        won: bool,
        score: Score,
        metrics: HostMetrics,
    ) -> HostOutcome {
        HostOutcome::new(won, score, metrics)
    }

    /// Carry per-player terminal outcomes (SPEC-12 §16.6) in stable seat order.
    pub fn outcome_set(&self, entries: Vec<(PlayerId, HostOutcome)>) -> HostOutcomeSet {
        HostOutcomeSet::new(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host_error_code::HostErrorCode;
    use crate::host_skip_reason::HostSkipReason;
    use axiom_runtime::{Runtime, RuntimeConfig};

    const STEP_NANOS: u64 = 1_000;

    fn ratio(value: f32) -> Ratio {
        Ratio::new(value).unwrap()
    }

    fn api() -> HostApi {
        HostApi::new()
    }

    #[test]
    fn new_and_default_are_equivalent() {
        assert_eq!(
            HostApi::new()
                .viewport(800, 600, ratio(2.0))
                .unwrap()
                .physical_width(),
            HostApi::default()
                .viewport(800, 600, ratio(2.0))
                .unwrap()
                .physical_width(),
        );
    }

    #[test]
    fn viewport_rejects_non_positive_scale_factor() {
        let v = api().viewport(800, 600, ratio(2.0)).unwrap();
        assert_eq!(v.physical_width(), 1600);
        assert_eq!(
            api().viewport(800, 600, ratio(-1.0)).unwrap_err().code(),
            HostErrorCode::InvalidScaleFactor
        );
    }

    #[test]
    fn viewport_from_physical_round_trips_with_viewport() {
        let v = api()
            .viewport_from_physical(1600, 1200, ratio(2.0))
            .unwrap();
        assert_eq!(v.logical_width(), 800);
        assert_eq!(v.logical_height(), 600);
    }

    #[test]
    fn frame_input_carries_supplied_values() {
        let v = api().viewport(800, 600, ratio(1.0)).unwrap();
        let f = api().frame_input(3, 16_666_667, v);
        assert_eq!(f.sequence(), 3);
        assert_eq!(f.elapsed_nanos(), 16_666_667);
    }

    #[test]
    fn lifecycle_initial_and_apply_route_through_state() {
        let s =
            api().apply_lifecycle_signal(api().lifecycle_initial(), HostLifecycleSignal::Started);
        assert!(s.visible());
    }

    #[test]
    fn boundary_config_constructor_rejects_zero_max_steps() {
        assert_eq!(
            api().boundary_config(STEP_NANOS, 0).unwrap_err().code(),
            HostErrorCode::InvalidBoundaryConfig
        );
    }

    #[test]
    fn validate_boundary_config_rejects_zero_fixed_step() {
        let kernel = KernelApi::new();
        let c = api().boundary_config(0, 1).unwrap();
        assert_eq!(
            api()
                .validate_boundary_config(&c, &kernel)
                .unwrap_err()
                .code(),
            HostErrorCode::InvalidBoundaryConfig
        );
    }

    #[test]
    fn validate_boundary_config_accepts_valid_step() {
        let kernel = KernelApi::new();
        let c = api().boundary_config(STEP_NANOS, 2).unwrap();
        assert!(api().validate_boundary_config(&c, &kernel).is_ok());
    }

    #[test]
    fn step_driver_round_trips_through_facade() {
        let driver = api().step_driver(api().boundary_config(STEP_NANOS, 5).unwrap());
        assert_eq!(driver.accumulator_nanos(), 0);
        assert_eq!(driver.last_sequence(), None);
    }

    #[test]
    fn plan_frame_is_deterministic() {
        let v = api().viewport(100, 100, ratio(1.0)).unwrap();
        let cfg = api().boundary_config(STEP_NANOS, 5).unwrap();
        let lifecycle =
            api().apply_lifecycle_signal(api().lifecycle_initial(), HostLifecycleSignal::Started);
        let input = api().frame_input(1, 3 * STEP_NANOS, v);
        let a = api().plan_frame(&input, &cfg, &lifecycle, 0);
        let b = api().plan_frame(&input, &cfg, &lifecycle, 0);
        assert_eq!(a, b);
        assert_eq!(a.steps(), 3);
    }

    #[test]
    fn report_no_step_frame_describes_skip() {
        let v = api().viewport(100, 100, ratio(1.0)).unwrap();
        let cfg = api().boundary_config(STEP_NANOS, 5).unwrap();
        let hidden = api().lifecycle_initial();
        let input = api().frame_input(7, STEP_NANOS, v);
        let plan = api().plan_frame(&input, &cfg, &hidden, 0);
        let report = api().report_no_step_frame(&input, plan, hidden);
        assert!(report.is_skipped());
        assert_eq!(
            report.plan().skip_reason(),
            Some(HostSkipReason::LifecycleHidden)
        );
        assert_eq!(report.steps_executed(), 0);
        assert_eq!(report.sequence(), 7);
    }

    use crate::host_alpha_mode::HostAlphaMode;
    use crate::host_color_format::HostColorFormat;
    use crate::host_device_profile::HostDeviceProfile;
    use crate::host_power_preference::HostPowerPreference;
    use crate::host_present_mode::HostPresentMode;
    use crate::host_presentation_status::HostPresentationStatus;

    fn kernel() -> KernelApi {
        KernelApi::new()
    }

    fn demo_descriptor(api: &HostApi) -> crate::host_surface_descriptor::HostSurfaceDescriptor {
        let viewport = api.viewport(800, 600, ratio(1.0)).unwrap();
        api.surface_descriptor(
            viewport,
            HostPresentMode::Fifo,
            HostAlphaMode::Opaque,
            HostColorFormat::Bgra8UnormSrgb,
        )
    }

    #[test]
    fn facade_mints_a_deterministic_presentation_target() {
        let t = api().presentation_target(&kernel(), 1, "primary").unwrap();
        assert_eq!(t.id().raw(), 1);
        assert_eq!(t.label(), "primary");
    }

    #[test]
    fn facade_rejects_null_target_and_empty_label() {
        assert_eq!(
            api()
                .presentation_target(&kernel(), 0, "x")
                .unwrap_err()
                .code(),
            HostErrorCode::InvalidPresentationTarget
        );
        assert_eq!(
            api()
                .presentation_target(&kernel(), 1, "")
                .unwrap_err()
                .code(),
            HostErrorCode::InvalidPresentationTarget
        );
    }

    #[test]
    fn facade_mints_a_deterministic_surface_handle() {
        let h = api().surface_handle(&kernel(), 9).unwrap();
        assert_eq!(h.id().raw(), 9);
        assert_eq!(
            api().surface_handle(&kernel(), 0).unwrap_err().code(),
            HostErrorCode::InvalidSurfaceHandle
        );
    }

    #[test]
    fn facade_builds_descriptor_adapter_and_device_requests() {
        let d = demo_descriptor(&api());
        assert_eq!(d.viewport().physical_width(), 800);
        let adapter = api().adapter_request(HostPowerPreference::HighPerformance, true);
        assert!(adapter.require_presentation_surface());
        let device = api().device_request(true, HostDeviceProfile::Baseline);
        assert!(device.require_presentation());
    }

    #[test]
    fn facade_builds_and_evaluates_a_valid_presentation_request() {
        let a = api();
        let k = kernel();
        let request = a
            .presentation_request(
                a.presentation_target(&k, 1, "primary").unwrap(),
                a.surface_handle(&k, 2).unwrap(),
                demo_descriptor(&a),
                a.adapter_request(HostPowerPreference::HighPerformance, true),
                a.device_request(true, HostDeviceProfile::Baseline),
            )
            .unwrap();
        let report = a.evaluate_presentation(&request);
        assert_eq!(report.status(), HostPresentationStatus::PendingBackend);
        assert!(!report.is_ready());
        assert_eq!(report.viewport().physical_width(), 800);
    }

    #[test]
    fn facade_presentation_request_is_deterministic() {
        let build = || {
            let a = api();
            let k = kernel();
            a.presentation_request(
                a.presentation_target(&k, 1, "primary").unwrap(),
                a.surface_handle(&k, 2).unwrap(),
                demo_descriptor(&a),
                a.adapter_request(HostPowerPreference::Default, true),
                a.device_request(false, HostDeviceProfile::Baseline),
            )
            .unwrap()
        };
        assert_eq!(build(), build());
        assert_eq!(
            api().evaluate_presentation(&build()),
            api().evaluate_presentation(&build())
        );
    }

    #[test]
    fn facade_rejects_inconsistent_presentation_request() {
        let a = api();
        let k = kernel();
        let err = a
            .presentation_request(
                a.presentation_target(&k, 1, "primary").unwrap(),
                a.surface_handle(&k, 2).unwrap(),
                demo_descriptor(&a),
                a.adapter_request(HostPowerPreference::Default, false),
                a.device_request(true, HostDeviceProfile::Baseline),
            )
            .unwrap_err();
        assert_eq!(err.code(), HostErrorCode::InvalidPresentationRequest);
    }

    use crate::host_param_value::HostParamValue;

    #[test]
    fn facade_mints_a_session_config_from_seed_and_params() {
        let params = HostSessionParams::new()
            .with(String::from("mode"), HostParamValue::Text(String::from("ranked")));
        let config = api().session_config(7, params.clone());
        assert_eq!(config.seed(), 7);
        assert_eq!(config.params(), &params);
    }

    #[test]
    fn facade_mints_an_outcome_with_empty_metrics() {
        let outcome = api().outcome(true, Score::new(42.0));
        assert_eq!(
            outcome,
            HostOutcome::new(true, Score::new(42.0), HostMetrics::new())
        );
        assert!(outcome.metrics().is_empty());
    }

    #[test]
    fn facade_mints_an_outcome_with_metrics() {
        let metrics = HostMetrics::new().with(String::from("kills"), Score::new(3.0));
        let outcome = api().outcome_with_metrics(false, Score::new(9.0), metrics.clone());
        assert_eq!(outcome, HostOutcome::new(false, Score::new(9.0), metrics));
    }

    #[test]
    fn facade_carries_a_per_player_outcome_set() {
        let entries = vec![
            (PlayerId::new(0), api().outcome(true, Score::new(10.0))),
            (PlayerId::new(1), api().outcome(false, Score::new(4.0))),
        ];
        let set = api().outcome_set(entries.clone());
        assert_eq!(set, HostOutcomeSet::new(entries));
        assert_eq!(set.get(PlayerId::new(0)), Some(&api().outcome(true, Score::new(10.0))));
    }

    #[test]
    fn facade_can_drive_a_runtime_through_a_driver() {
        let v = api().viewport(100, 100, ratio(1.0)).unwrap();
        let cfg = api().boundary_config(STEP_NANOS, 5).unwrap();
        let mut driver = api().step_driver(cfg);
        driver.apply_lifecycle_signal(HostLifecycleSignal::Started);

        let mut runtime = Runtime::new(RuntimeConfig::new(STEP_NANOS)).unwrap();
        runtime.initialize().unwrap();
        runtime.start().unwrap();

        let report = driver
            .drive(&mut runtime, api().frame_input(1, 2 * STEP_NANOS, v))
            .unwrap();
        assert_eq!(report.steps_executed(), 2);
    }
}
