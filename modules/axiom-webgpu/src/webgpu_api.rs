//! The single public facade of the `axiom-webgpu` module.

use axiom_math::Mat4;

use crate::backend_kind::BackendKind;
use crate::gpu_command::GpuCommand;
use crate::gpu_submission::GpuSubmission;
use crate::gpu_submission_report::GpuSubmissionReport;

/// The only public export of `axiom-webgpu`.
///
/// Today this is a **deterministic recorder**: every command the app
/// pushes into a [`GpuSubmission`] is captured in the
/// [`GpuSubmissionReport`] `submit()` returns. Real WebGPU/wgpu
/// integration is deferred until the host layer exposes a surface
/// (see `ARCHITECTURE.md`).
///
/// The boundary itself — `GpuSubmission` shape + `GpuSubmissionReport`
/// shape — is what every higher-layer test asserts on, so swapping the
/// recorder for a live backend later changes only the body of
/// `submit()` and not the surface of the module.
#[derive(Debug, Clone, Copy, Default)]
pub struct WebGpuApi {
    _sealed: (),
}

impl WebGpuApi {
    pub const fn new() -> Self {
        WebGpuApi { _sealed: () }
    }

    pub const fn backend(&self) -> BackendKind {
        BackendKind::Recording
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

    /// Submit a [`GpuSubmission`] to the backend. In the
    /// [`BackendKind::Recording`] mode this builds a deterministic
    /// [`GpuSubmissionReport`] from the supplied commands.
    pub fn submit(&self, sub: GpuSubmission) -> GpuSubmissionReport {
        let width = sub.target_width();
        let height = sub.target_height();
        let commands = sub.commands().to_vec();
        GpuSubmissionReport::new(commands, width, height)
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
        let _ = WebGpuApi::new();
        let _ = WebGpuApi::default();
    }

    #[test]
    fn backend_is_recording_today() {
        assert_eq!(api().backend(), BackendKind::Recording);
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
}
