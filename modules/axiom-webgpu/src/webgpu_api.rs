//! The single public facade of the `axiom-webgpu` module.

use axiom_host::HostPresentationRequest;
use axiom_kernel::{KernelError, KernelErrorCode, KernelErrorScope, KernelResult};
use axiom_math::Mat4;

use crate::backend_kind::BackendKind;
use crate::gpu_command::GpuCommand;
use crate::gpu_submission::GpuSubmission;
use crate::gpu_submission_report::GpuSubmissionReport;
use crate::webgpu_backend_state::WebGpuBackendState;

/// The only public export of `axiom-webgpu`.
///
/// `WebGpuApi` carries a tiny, deterministic backend state and routes the
/// **one** [`GpuSubmission`] input contract through whichever backend it is
/// in:
///
/// - **Recording** (default): captures every command into a deterministic
///   [`GpuSubmissionReport`]. No GPU work. This is the proof backend the
///   headless slice depends on, and its behaviour is unchanged from before
///   backend modes existed.
/// - **Live**: the structural seam for real presentation, built from the
///   host presentation boundary ([`HostPresentationRequest`]). It accepts the
///   same `GpuSubmission` shape but performs no real GPU work this pass; its
///   report carries a deterministic not-bound / not-initialized status.
///
/// The boundary itself — `GpuSubmission` in, `GpuSubmissionReport` out — is
/// what every higher-layer test asserts on, so the future live backend that
/// actually presents changes only the *body* of `submit()`, never the
/// surface of the module.
#[derive(Debug, Clone, Copy, Default)]
pub struct WebGpuApi {
    state: WebGpuBackendState,
}

impl WebGpuApi {
    /// Construct the default deterministic **recording** backend. Identical
    /// behaviour to [`Self::new_recording`]; kept so existing callers
    /// (the headless slice) compile unchanged.
    pub const fn new() -> Self {
        WebGpuApi::new_recording()
    }

    /// Construct a deterministic **recording** backend. Captures submissions
    /// into a report and never touches a GPU.
    pub const fn new_recording() -> Self {
        WebGpuApi {
            state: WebGpuBackendState::Recording,
        }
    }

    /// Construct a **live** backend with no presentation target/surface bound
    /// yet. Accepts submissions but reports them as `LiveNotBound` — nothing
    /// is presented.
    pub const fn new_live_unbound() -> Self {
        WebGpuApi {
            state: WebGpuBackendState::LiveUnbound,
        }
    }

    /// Construct a **live** backend bound to a validated host presentation
    /// request.
    ///
    /// The request must describe a presentation-capable adapter; a request
    /// whose adapter does not require a presentation surface can never drive
    /// a live presenting backend, so it is rejected through the kernel error
    /// model. On success the backend is in the `LivePresentationRequested`
    /// state — validated and ready for a future live pass to bind a real
    /// device/surface, but presenting nothing yet.
    pub fn new_live(request: &HostPresentationRequest) -> KernelResult<Self> {
        // The request's surface handle is already guaranteed valid by
        // `HostApi`, which mints only valid handles; the only setup that can
        // actually fail is a request whose adapter cannot present.
        if !request.adapter().require_presentation_surface() {
            return Err(KernelError::new(
                KernelErrorScope::Id,
                KernelErrorCode::InvalidId,
                "live backend requires a presentation-capable adapter request",
            ));
        }
        Ok(WebGpuApi {
            state: WebGpuBackendState::LivePresentationRequested(*request),
        })
    }

    /// The coarse [`BackendKind`] this backend is operating in.
    pub const fn backend_kind(&self) -> BackendKind {
        self.state.kind()
    }

    /// Whether this is the deterministic recording backend.
    pub const fn is_recording(&self) -> bool {
        matches!(self.state.kind(), BackendKind::Recording)
    }

    /// Whether this is a live backend (bound or unbound).
    pub const fn is_live(&self) -> bool {
        matches!(self.state.kind(), BackendKind::Live)
    }

    /// Whether a live backend has a bound, validated host presentation
    /// request. `false` for recording and for an unbound live backend.
    pub const fn has_presentation_request(&self) -> bool {
        self.state.presentation_request().is_some()
    }

    /// GPU-command kind codes (mirrored from [`GpuCommand`]).
    pub const KIND_CLEAR_FRAME: u32 = GpuCommand::KIND_CLEAR_FRAME;
    pub const KIND_SET_PIPELINE: u32 = GpuCommand::KIND_SET_PIPELINE;
    pub const KIND_SET_CAMERA: u32 = GpuCommand::KIND_SET_CAMERA;
    pub const KIND_SET_MESH: u32 = GpuCommand::KIND_SET_MESH;
    pub const KIND_SET_MATERIAL: u32 = GpuCommand::KIND_SET_MATERIAL;
    pub const KIND_DRAW_INDEXED: u32 = GpuCommand::KIND_DRAW_INDEXED;
    pub const KIND_PRESENT: u32 = GpuCommand::KIND_PRESENT;

    // --- Submission construction ---

    pub fn new_submission(&self, target_width: u32, target_height: u32) -> GpuSubmission {
        GpuSubmission::new(target_width, target_height)
    }

    pub fn submission_clear_frame(&self, sub: &mut GpuSubmission, color: [f32; 4]) {
        sub.push(GpuCommand::ClearFrame { color });
    }

    pub fn submission_set_pipeline(&self, sub: &mut GpuSubmission, pipeline_id: u32) {
        sub.push(GpuCommand::SetPipeline { pipeline_id });
    }

    pub fn submission_set_camera(
        &self,
        sub: &mut GpuSubmission,
        view: Mat4,
        projection: Mat4,
    ) {
        sub.push(GpuCommand::SetCamera { view, projection });
    }

    pub fn submission_set_mesh(&self, sub: &mut GpuSubmission, mesh_id: u64) {
        sub.push(GpuCommand::SetMesh { mesh_id });
    }

    pub fn submission_set_material(&self, sub: &mut GpuSubmission, material_id: u64) {
        sub.push(GpuCommand::SetMaterial { material_id });
    }

    pub fn submission_draw_indexed(
        &self,
        sub: &mut GpuSubmission,
        index_count: u32,
        world: Mat4,
    ) {
        sub.push(GpuCommand::DrawIndexed { index_count, world });
    }

    pub fn submission_present(&self, sub: &mut GpuSubmission) {
        sub.push(GpuCommand::Present);
    }

    // --- Submission ---

    /// Submit a [`GpuSubmission`] to the backend.
    ///
    /// Both backends accept the same submission shape and build a
    /// deterministic [`GpuSubmissionReport`] from the supplied commands. The
    /// recording backend tags the report `Recorded`; a live backend tags it
    /// with its deterministic not-bound / not-initialized status. No backend
    /// presents pixels in this pass, so [`GpuSubmissionReport::presented`] is
    /// always `false`.
    pub fn submit(&self, sub: GpuSubmission) -> GpuSubmissionReport {
        let width = sub.target_width();
        let height = sub.target_height();
        let commands = sub.commands().to_vec();
        GpuSubmissionReport::new(commands, width, height, self.state.submission_status())
    }

    // --- Report inspection ---

    pub fn report_command_count(&self, report: &GpuSubmissionReport) -> usize {
        report.submitted_command_count()
    }

    pub fn report_clear_count(&self, report: &GpuSubmissionReport) -> u32 {
        report.clear_count()
    }

    pub fn report_draw_count(&self, report: &GpuSubmissionReport) -> u32 {
        report.draw_count()
    }

    pub fn report_present_count(&self, report: &GpuSubmissionReport) -> u32 {
        report.present_count()
    }

    pub fn report_kind_at(&self, report: &GpuSubmissionReport, idx: usize) -> Option<u32> {
        report.submitted_commands().get(idx).map(GpuCommand::kind_code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn api() -> WebGpuApi {
        WebGpuApi::new()
    }

    #[test]
    fn new_and_default_facades_are_equivalent() {
        // Both construction paths start in the same recording backend.
        assert_eq!(
            WebGpuApi::new().backend_kind(),
            WebGpuApi::default().backend_kind(),
        );
    }

    #[test]
    fn default_backend_is_recording() {
        assert_eq!(api().backend_kind(), BackendKind::Recording);
        assert!(api().is_recording());
        assert!(!api().is_live());
        assert!(!api().has_presentation_request());
    }

    #[test]
    fn submission_round_trip_records_every_command() {
        let mut sub = api().new_submission(800, 600);
        api().submission_clear_frame(&mut sub, [0.1, 0.2, 0.3, 1.0]);
        api().submission_set_pipeline(&mut sub, 1);
        api().submission_set_camera(&mut sub, Mat4::IDENTITY, Mat4::IDENTITY);
        api().submission_set_mesh(&mut sub, 7);
        api().submission_set_material(&mut sub, 9);
        api().submission_draw_indexed(&mut sub, 36, Mat4::IDENTITY);
        api().submission_present(&mut sub);
        let report = api().submit(sub);
        assert_eq!(api().report_command_count(&report), 7);
        assert_eq!(api().report_clear_count(&report), 1);
        assert_eq!(api().report_draw_count(&report), 1);
        assert_eq!(api().report_present_count(&report), 1);
        assert_eq!(report.target_width(), 800);
        assert_eq!(report.target_height(), 600);
    }

    #[test]
    fn submit_is_deterministic_for_identical_input() {
        let build_sub = || {
            let mut sub = api().new_submission(100, 100);
            api().submission_clear_frame(&mut sub, [0.0, 0.0, 0.0, 1.0]);
            api().submission_draw_indexed(&mut sub, 36, Mat4::IDENTITY);
            api().submission_present(&mut sub);
            sub
        };
        assert_eq!(api().submit(build_sub()), api().submit(build_sub()));
    }

    #[test]
    fn report_count_passthroughs_match_underlying_with_values_distinct_from_one() {
        // Two of every counted kind so each passthrough returns 2 — distinct
        // from the mutant constants 0 and 1, and asserted to equal the
        // underlying report's own counts.
        let mut sub = api().new_submission(1, 1);
        api().submission_clear_frame(&mut sub, [0.0, 0.0, 0.0, 1.0]);
        api().submission_clear_frame(&mut sub, [1.0, 1.0, 1.0, 1.0]);
        api().submission_draw_indexed(&mut sub, 36, Mat4::IDENTITY);
        api().submission_draw_indexed(&mut sub, 6, Mat4::IDENTITY);
        api().submission_present(&mut sub);
        api().submission_present(&mut sub);
        let report = api().submit(sub);
        assert_eq!(api().report_clear_count(&report), 2);
        assert_eq!(api().report_clear_count(&report), report.clear_count());
        assert_eq!(api().report_draw_count(&report), 2);
        assert_eq!(api().report_draw_count(&report), report.draw_count());
        assert_eq!(api().report_present_count(&report), 2);
        assert_eq!(api().report_present_count(&report), report.present_count());
    }

    #[test]
    fn report_kind_at_returns_correct_codes() {
        let mut sub = api().new_submission(1, 1);
        api().submission_clear_frame(&mut sub, [0.0, 0.0, 0.0, 1.0]);
        api().submission_present(&mut sub);
        let report = api().submit(sub);
        assert_eq!(
            api().report_kind_at(&report, 0),
            Some(WebGpuApi::KIND_CLEAR_FRAME)
        );
        assert_eq!(
            api().report_kind_at(&report, 1),
            Some(WebGpuApi::KIND_PRESENT)
        );
        assert_eq!(api().report_kind_at(&report, 2), None);
    }

    // --- Backend modes ---

    use axiom_host::{
        HostAlphaMode, HostApi, HostColorFormat, HostDeviceProfile, HostPowerPreference,
        HostPresentMode, HostPresentationRequest,
    };
    use axiom_kernel::KernelApi;
    use axiom_math::MathApi;
    use crate::gpu_submission_status::GpuSubmissionStatus;

    /// Build a presentation-capable host presentation request. When
    /// `presentable` is false the adapter does not require a presentation
    /// surface (and the device does not require presentation, so the host
    /// itself accepts it) — an unsuitable request for a live presenting
    /// backend.
    fn host_request(presentable: bool) -> HostPresentationRequest {
        let host = HostApi::new();
        let kernel = KernelApi::new();
        let viewport = host.viewport(&MathApi::new(), 800, 600, 1.0).unwrap();
        let descriptor = host.surface_descriptor(
            viewport,
            HostPresentMode::Fifo,
            HostAlphaMode::Opaque,
            HostColorFormat::Bgra8UnormSrgb,
        );
        host.presentation_request(
            host.presentation_target(&kernel, 1, "primary").unwrap(),
            host.surface_handle(&kernel, 2).unwrap(),
            descriptor,
            host.adapter_request(HostPowerPreference::HighPerformance, presentable),
            host.device_request(presentable, HostDeviceProfile::Baseline),
        )
        .unwrap()
    }

    fn demo_submission(api: &WebGpuApi) -> GpuSubmission {
        let mut sub = api.new_submission(800, 600);
        api.submission_clear_frame(&mut sub, [0.05, 0.06, 0.08, 1.0]);
        api.submission_set_pipeline(&mut sub, 1);
        api.submission_draw_indexed(&mut sub, 36, Mat4::IDENTITY);
        api.submission_present(&mut sub);
        sub
    }

    #[test]
    fn recording_submit_is_tagged_recorded_and_claims_no_pixels() {
        let api = WebGpuApi::new_recording();
        let report = api.submit(demo_submission(&api));
        assert_eq!(report.status(), GpuSubmissionStatus::Recorded);
        assert!(report.is_recorded());
        assert!(!report.presented());
        // Recording shape is unchanged.
        assert_eq!(report.clear_count(), 1);
        assert_eq!(report.draw_count(), 1);
        assert_eq!(report.present_count(), 1);
    }

    #[test]
    fn recording_report_is_deterministic_including_status() {
        let api = WebGpuApi::new_recording();
        assert_eq!(api.submit(demo_submission(&api)), api.submit(demo_submission(&api)));
    }

    #[test]
    fn live_backend_can_be_built_from_valid_presentation_request() {
        let api = WebGpuApi::new_live(&host_request(true)).unwrap();
        assert_eq!(api.backend_kind(), BackendKind::Live);
        assert!(api.is_live());
        assert!(api.has_presentation_request());
    }

    #[test]
    fn live_backend_accepts_same_submission_shape_but_does_not_present() {
        let api = WebGpuApi::new_live(&host_request(true)).unwrap();
        let report = api.submit(demo_submission(&api));
        // Same submission shape is accepted and recorded.
        assert_eq!(report.clear_count(), 1);
        assert_eq!(report.draw_count(), 1);
        assert_eq!(report.present_count(), 1);
        // ...but nothing was presented and the status says so.
        assert_eq!(report.status(), GpuSubmissionStatus::LiveNotInitialized);
        assert!(report.is_live_not_initialized());
        assert!(!report.presented());
    }

    #[test]
    fn live_submit_is_deterministic() {
        let api = WebGpuApi::new_live(&host_request(true)).unwrap();
        assert_eq!(api.submit(demo_submission(&api)), api.submit(demo_submission(&api)));
    }

    #[test]
    fn live_unbound_backend_reports_not_bound() {
        let api = WebGpuApi::new_live_unbound();
        assert_eq!(api.backend_kind(), BackendKind::Live);
        assert!(!api.has_presentation_request());
        let report = api.submit(demo_submission(&api));
        assert_eq!(report.status(), GpuSubmissionStatus::LiveNotBound);
        assert!(report.is_live_not_bound());
        assert!(!report.presented());
    }

    #[test]
    fn live_backend_rejects_non_presentation_capable_request() {
        let err = WebGpuApi::new_live(&host_request(false)).unwrap_err();
        assert_eq!(err.scope(), axiom_kernel::KernelErrorScope::Id);
        assert_eq!(err.code(), axiom_kernel::KernelErrorCode::InvalidId);
    }

    #[test]
    fn is_recording_is_false_for_live_backends() {
        // Exercises the non-matching arm of `is_recording`'s `matches!`.
        let unbound = WebGpuApi::new_live_unbound();
        assert!(!unbound.is_recording());
        let live = WebGpuApi::new_live(&host_request(true)).unwrap();
        assert!(!live.is_recording());
    }
}
