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
            state: WebGpuBackendState::recording(),
        }
    }

    /// Construct a **live** backend with no presentation target/surface bound
    /// yet. Accepts submissions but reports them as `LiveNotBound` — nothing
    /// is presented.
    pub const fn new_live_unbound() -> Self {
        WebGpuApi {
            state: WebGpuBackendState::live_unbound(),
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
        request
            .adapter()
            .require_presentation_surface()
            .then_some(WebGpuApi {
                state: WebGpuBackendState::live_presentation_requested(*request),
            })
            .ok_or_else(|| {
                KernelError::new(
                    KernelErrorScope::Id,
                    KernelErrorCode::InvalidId,
                    "live backend requires a presentation-capable adapter request",
                )
            })
    }

    /// The coarse [`BackendKind`] this backend is operating in.
    pub const fn backend_kind(&self) -> BackendKind {
        self.state.kind()
    }

    /// Whether this is the deterministic recording backend.
    pub const fn is_recording(&self) -> bool {
        (self.state.kind() as u8) == (BackendKind::Recording as u8)
    }

    /// Whether this is a live backend (bound or unbound).
    pub const fn is_live(&self) -> bool {
        (self.state.kind() as u8) == (BackendKind::Live as u8)
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

    pub fn new_submission(&self, target_width: u32, target_height: u32) -> GpuSubmission {
        GpuSubmission::new(target_width, target_height)
    }

    pub fn submission_clear_frame(&self, sub: &mut GpuSubmission, color: [f32; 4]) {
        sub.push(GpuCommand::clear_frame(color));
    }

    pub fn submission_set_pipeline(&self, sub: &mut GpuSubmission, pipeline_id: u32) {
        sub.push(GpuCommand::set_pipeline(pipeline_id));
    }

    pub fn submission_set_camera(&self, sub: &mut GpuSubmission, view: Mat4, projection: Mat4) {
        sub.push(GpuCommand::set_camera(view, projection));
    }

    pub fn submission_set_mesh(&self, sub: &mut GpuSubmission, mesh_id: u64) {
        sub.push(GpuCommand::set_mesh(mesh_id));
    }

    pub fn submission_set_material(
        &self,
        sub: &mut GpuSubmission,
        material_id: u64,
        material_texture_id: u64,
    ) {
        sub.push(GpuCommand::set_material(material_id, material_texture_id));
    }

    pub fn submission_draw_indexed(&self, sub: &mut GpuSubmission, index_count: u32, world: Mat4) {
        sub.push(GpuCommand::draw_indexed(index_count, world));
    }

    pub fn submission_present(&self, sub: &mut GpuSubmission) {
        sub.push(GpuCommand::present());
    }

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
        report
            .submitted_commands()
            .get(idx)
            .map(GpuCommand::kind_code)
    }

    /// The albedo texture id (`0` = untextured) bound by the `SetMaterial`
    /// command at `idx`, or `None` if that command is absent or another kind.
    /// Makes the material→texture binding observable in the recorded receipt.
    pub fn report_material_texture_at(
        &self,
        report: &GpuSubmissionReport,
        idx: usize,
    ) -> Option<u64> {
        report
            .submitted_commands()
            .get(idx)
            .and_then(GpuCommand::as_set_material_texture)
    }

    /// **Live arm:** realize a [`GpuSubmission`] on a real native GPU
    /// **off-screen** and read the frame back as `width * height * 4` RGBA8
    /// bytes (row-major, top-down). This is the real-pixel body the
    /// [`BackendKind::Live`] seam promised: it executes the *same*
    /// `GpuSubmission` the recording backend records, so the deterministic
    /// command chain the engine proves is the chain that renders.
    ///
    /// `meshes` is `(mesh_id, position floats [3 per vertex], triangle
    /// indices)` and `materials` is `(material_id, linear-RGBA colour)`, keyed
    /// by the ids the submission's `SetMesh` / `SetMaterial` commands bind — the
    /// resource payloads a real backend uploads out-of-band from the per-frame
    /// command list. Returns `None` when no native GPU adapter is available.
    ///
    /// Compiled only behind the off-by-default `offscreen` feature, so the
    /// engine's default build, coverage gate, and branchless lint never see the
    /// real-GPU arm. The deterministic [`Self::submit`] receipt is unaffected —
    /// the [`GpuSubmission`] / [`GpuSubmissionReport`] shapes are unchanged.
    #[cfg(all(not(target_arch = "wasm32"), feature = "offscreen"))]
    pub fn present_submission_offscreen_rgba(
        &self,
        submission: &GpuSubmission,
        meshes: &[(u64, Vec<f32>, Vec<u32>)],
        materials: &[(u64, [f32; 4])],
    ) -> Option<Vec<u8>> {
        // A live backend built from a `HostPresentationRequest` drives the real
        // GPU adapter selection with the *host's* declared power preference — the
        // presentation capability is consumed by the live render, not just
        // validated. An unbound live backend defaults to high performance.
        let power_preference = self
            .state
            .presentation_request()
            .map(|request| request.adapter().power_preference())
            .unwrap_or(axiom_host::HostPowerPreference::HighPerformance);
        crate::live_present::render_submission_to_rgba(
            submission,
            meshes,
            materials,
            power_preference,
        )
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
        api().submission_set_material(&mut sub, 9, 4);
        api().submission_draw_indexed(&mut sub, 36, Mat4::IDENTITY);
        api().submission_present(&mut sub);
        let report = api().submit(sub);
        assert_eq!(api().report_command_count(&report), 7);
        assert_eq!(api().report_clear_count(&report), 1);
        assert_eq!(api().report_draw_count(&report), 1);
        assert_eq!(api().report_present_count(&report), 1);
        assert_eq!(report.target_width(), 800);
        assert_eq!(report.target_height(), 600);
        assert_eq!(api().report_material_texture_at(&report, 4), Some(4));
        assert_eq!(api().report_material_texture_at(&report, 0), None);
        assert_eq!(api().report_material_texture_at(&report, 99), None);
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

    use crate::gpu_submission_status::GpuSubmissionStatus;
    use axiom_host::{
        HostAlphaMode, HostApi, HostColorFormat, HostDeviceProfile, HostPowerPreference,
        HostPresentMode, HostPresentationRequest,
    };
    use axiom_kernel::{KernelApi, Ratio};

    /// Build a presentation-capable host presentation request. When
    /// `presentable` is false the adapter does not require a presentation
    /// surface (and the device does not require presentation, so the host
    /// itself accepts it) — an unsuitable request for a live presenting
    /// backend.
    fn host_request(presentable: bool) -> HostPresentationRequest {
        let host = HostApi::new();
        let kernel = KernelApi::new();
        let viewport = host.viewport(800, 600, Ratio::new(1.0).unwrap()).unwrap();
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
        assert_eq!(report.clear_count(), 1);
        assert_eq!(report.draw_count(), 1);
        assert_eq!(report.present_count(), 1);
    }

    #[test]
    fn recording_report_is_deterministic_including_status() {
        let api = WebGpuApi::new_recording();
        assert_eq!(
            api.submit(demo_submission(&api)),
            api.submit(demo_submission(&api))
        );
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
        assert_eq!(report.clear_count(), 1);
        assert_eq!(report.draw_count(), 1);
        assert_eq!(report.present_count(), 1);
        assert_eq!(report.status(), GpuSubmissionStatus::LiveNotInitialized);
        assert!(report.is_live_not_initialized());
        assert!(!report.presented());
    }

    #[test]
    fn live_submit_is_deterministic() {
        let api = WebGpuApi::new_live(&host_request(true)).unwrap();
        assert_eq!(
            api.submit(demo_submission(&api)),
            api.submit(demo_submission(&api))
        );
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
        let unbound = WebGpuApi::new_live_unbound();
        assert!(!unbound.is_recording());
        let live = WebGpuApi::new_live(&host_request(true)).unwrap();
        assert!(!live.is_recording());
    }
}

/// Live-arm proof: the real off-screen GPU realization of a `GpuSubmission`.
///
/// Compiled only behind the `offscreen` feature on native, so it runs as
/// `cargo test -p axiom-webgpu --features offscreen` on a machine with a GPU —
/// outside the default build, coverage gate, and branchless lint (the real wgpu
/// arm is the sanctioned platform boundary). Each test renders a known scene,
/// reads the pixels back, and asserts non-trivially on them.
#[cfg(all(test, not(target_arch = "wasm32"), feature = "offscreen"))]
mod live_present_tests {
    use super::*;

    /// A unit cube centred at the origin, spanning `[-0.5, 0.5]^3`: 8 corner
    /// positions (3 floats each) + 36 triangle indices (12 tris).
    fn unit_cube() -> (Vec<f32>, Vec<u32>) {
        #[rustfmt::skip]
        let positions = vec![
            -0.5, -0.5, -0.5,
             0.5, -0.5, -0.5,
             0.5,  0.5, -0.5,
            -0.5,  0.5, -0.5,
            -0.5, -0.5,  0.5,
             0.5, -0.5,  0.5,
             0.5,  0.5,  0.5,
            -0.5,  0.5,  0.5,
        ];
        #[rustfmt::skip]
        let indices = vec![
            0, 1, 2, 0, 2, 3, // back
            4, 6, 5, 4, 7, 6, // front
            0, 4, 5, 0, 5, 1, // bottom
            3, 2, 6, 3, 6, 7, // top
            0, 3, 7, 0, 7, 4, // left
            1, 5, 6, 1, 6, 2, // right
        ];
        (positions, indices)
    }

    fn pixel(pixels: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * w + x) * 4) as usize;
        [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
    }

    /// Build a presentation-capable host request with a chosen adapter power
    /// preference, the way a host adapter would, so the request-bound live path
    /// can be exercised end-to-end.
    fn host_request(power: axiom_host::HostPowerPreference) -> HostPresentationRequest {
        use axiom_host::{
            HostAlphaMode, HostApi, HostColorFormat, HostDeviceProfile, HostPresentMode,
        };
        use axiom_kernel::{KernelApi, Ratio};
        let host = HostApi::new();
        let kernel = KernelApi::new();
        let viewport = host.viewport(64, 64, Ratio::new(1.0).unwrap()).unwrap();
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
            host.adapter_request(power, true),
            host.device_request(true, HostDeviceProfile::Baseline),
        )
        .unwrap()
    }

    #[test]
    fn a_live_backend_built_from_a_host_request_presents_real_pixels() {
        // The host presentation capability is load-bearing: a live backend built
        // from a `HostPresentationRequest` (here asking for a low-power adapter)
        // drives the real GPU init and renders the submission's pixels.
        let request = host_request(axiom_host::HostPowerPreference::LowPower);
        let api = WebGpuApi::new_live(&request).unwrap();
        assert!(api.has_presentation_request());
        let mut sub = api.new_submission(32, 32);
        api.submission_clear_frame(&mut sub, [0.0, 0.0, 0.0, 1.0]);
        api.submission_present(&mut sub);
        let pixels = api
            .present_submission_offscreen_rgba(&sub, &[], &[])
            .expect("native GPU adapter available");
        assert_eq!(pixels.len(), 32 * 32 * 4);
        assert!(pixels.chunks_exact(4).all(|p| p == [0, 0, 0, 255]));
    }

    #[test]
    fn cleared_frame_reads_back_the_clear_color() {
        let api = WebGpuApi::new_live_unbound();
        let mut sub = api.new_submission(64, 64);
        api.submission_clear_frame(&mut sub, [0.0, 0.0, 0.0, 1.0]);
        api.submission_present(&mut sub);
        let pixels = api
            .present_submission_offscreen_rgba(&sub, &[], &[])
            .expect("a native GPU adapter is available in this environment");
        assert_eq!(pixels.len(), 64 * 64 * 4);
        // Every pixel is the opaque-black clear colour — no draws touched it.
        assert!(pixels.chunks_exact(4).all(|p| p == [0, 0, 0, 255]));
    }

    #[test]
    fn a_nonblack_clear_propagates_to_every_pixel() {
        let api = WebGpuApi::new_live_unbound();
        let mut sub = api.new_submission(16, 16);
        api.submission_clear_frame(&mut sub, [0.0, 1.0, 0.0, 1.0]);
        api.submission_present(&mut sub);
        let pixels = api
            .present_submission_offscreen_rgba(&sub, &[], &[])
            .expect("native GPU adapter available");
        let corner = pixel(&pixels, 16, 0, 0);
        assert_eq!(corner[0], 0, "no red in a green clear: {corner:?}");
        assert_eq!(corner[2], 0, "no blue in a green clear: {corner:?}");
        assert_eq!(corner[3], 255);
        assert!(corner[1] > 200, "green clear reads back green: {corner:?}");
    }

    #[test]
    fn a_drawn_cube_covers_the_center_and_leaves_the_corners_clear() {
        let api = WebGpuApi::new_live_unbound();
        let (positions, indices) = unit_cube();
        let index_count = indices.len() as u32;
        let mut sub = api.new_submission(64, 64);
        api.submission_clear_frame(&mut sub, [0.0, 0.0, 0.0, 1.0]);
        // Identity camera: the cube's [-0.5,0.5] xy fills the central quarter.
        api.submission_set_camera(&mut sub, Mat4::IDENTITY, Mat4::IDENTITY);
        api.submission_set_mesh(&mut sub, 7);
        api.submission_set_material(&mut sub, 3, 0);
        api.submission_draw_indexed(&mut sub, index_count, Mat4::IDENTITY);
        api.submission_present(&mut sub);

        let meshes = vec![(7_u64, positions, indices)];
        let materials = vec![(3_u64, [1.0_f32, 1.0, 1.0, 1.0])];
        let pixels = api
            .present_submission_offscreen_rgba(&sub, &meshes, &materials)
            .expect("native GPU adapter available");

        let center = pixel(&pixels, 64, 32, 32);
        assert!(
            center[0] > 200 && center[1] > 200 && center[2] > 200,
            "white cube face covers the centre: {center:?}"
        );
        // Screen corner is outside the cube's xy footprint -> untouched clear.
        assert_eq!(
            pixel(&pixels, 64, 0, 0),
            [0, 0, 0, 255],
            "corner stays clear"
        );
    }

    #[test]
    fn the_bound_material_color_reaches_the_drawn_pixels() {
        let api = WebGpuApi::new_live_unbound();
        let (positions, indices) = unit_cube();
        let index_count = indices.len() as u32;
        let mut sub = api.new_submission(32, 32);
        api.submission_clear_frame(&mut sub, [0.0, 0.0, 0.0, 1.0]);
        api.submission_set_camera(&mut sub, Mat4::IDENTITY, Mat4::IDENTITY);
        api.submission_set_mesh(&mut sub, 1);
        api.submission_set_material(&mut sub, 5, 0);
        api.submission_draw_indexed(&mut sub, index_count, Mat4::IDENTITY);
        api.submission_present(&mut sub);
        // A red material (linear (1,0,0)) reads back as sRGB red at the centre.
        let meshes = vec![(1_u64, positions, indices)];
        let materials = vec![(5_u64, [1.0_f32, 0.0, 0.0, 1.0])];
        let pixels = api
            .present_submission_offscreen_rgba(&sub, &meshes, &materials)
            .expect("native GPU adapter available");
        let center = pixel(&pixels, 32, 16, 16);
        assert!(center[0] > 200, "expected red: {center:?}");
        assert!(
            center[1] < 40 && center[2] < 40,
            "expected pure red: {center:?}"
        );
    }

    #[test]
    fn an_unbound_mesh_id_is_skipped_rather_than_panicking() {
        // A draw referencing a mesh id with no uploaded geometry is silently
        // skipped (like the live/offscreen renderer), leaving the clear frame.
        let api = WebGpuApi::new_live_unbound();
        let mut sub = api.new_submission(8, 8);
        api.submission_clear_frame(&mut sub, [0.0, 0.0, 0.0, 1.0]);
        api.submission_set_mesh(&mut sub, 999);
        api.submission_draw_indexed(&mut sub, 36, Mat4::IDENTITY);
        api.submission_present(&mut sub);
        let pixels = api
            .present_submission_offscreen_rgba(&sub, &[], &[])
            .expect("native GPU adapter available");
        assert!(pixels.chunks_exact(4).all(|p| p == [0, 0, 0, 255]));
    }

    #[test]
    fn identical_submissions_read_back_byte_identical_pixels() {
        // The live off-screen realization is deterministic for identical input
        // on the same device — the determinism the whole chain promises, now
        // proven through real pixels, not only the recorded receipt.
        let api = WebGpuApi::new_live_unbound();
        let (positions, indices) = unit_cube();
        let index_count = indices.len() as u32;
        let build = || {
            let mut sub = api.new_submission(48, 48);
            api.submission_clear_frame(&mut sub, [0.02, 0.03, 0.05, 1.0]);
            api.submission_set_camera(&mut sub, Mat4::IDENTITY, Mat4::IDENTITY);
            api.submission_set_mesh(&mut sub, 2);
            api.submission_set_material(&mut sub, 9, 0);
            api.submission_draw_indexed(&mut sub, index_count, Mat4::IDENTITY);
            api.submission_present(&mut sub);
            sub
        };
        let meshes = vec![(2_u64, positions, indices)];
        let materials = vec![(9_u64, [0.8_f32, 0.4, 0.2, 1.0])];
        let a = api
            .present_submission_offscreen_rgba(&build(), &meshes, &materials)
            .unwrap();
        let b = api
            .present_submission_offscreen_rgba(&build(), &meshes, &materials)
            .unwrap();
        assert_eq!(a, b);
    }
}
