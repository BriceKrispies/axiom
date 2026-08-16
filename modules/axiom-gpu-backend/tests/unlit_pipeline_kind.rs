//! **`RenderPipelineKind::UNLIT` reaches a backend**, derived from the surface.
//!
//! `axiom_render::RenderPipelineKind` has declared `BASIC_LIT = 1` and
//! `UNLIT = 2` since it was written, and the value has always died at the
//! `axiom_host::FramePacket` boundary — the packet carries no pipeline lane, and
//! widening it for something a backend can *compute* would be a duplicated fact
//! that could disagree with the surface it duplicates.
//!
//! So this backend derives it: a draw carries a `surface_program` digest, the
//! digest names a `Surface`, and a `Surface` states its
//! `axiom_surface::LightingModel`. `GpuBackendApi::surface_pipeline_kind` is
//! that derivation, and this is the first test in the repository to assert that
//! a backend answers the render module's second pipeline marker at all.
//!
//! It lives in `tests/` because `gpu_backend_api.rs` is at the engine
//! file-size budget.

use axiom_field::FieldValue;
use axiom_gpu_backend::GpuBackendApi;
use axiom_host::{
    HostAlphaMode, HostApi, HostColorFormat, HostDeviceProfile, HostPowerPreference,
    HostPresentMode, HostPresentationRequest,
};
use axiom_kernel::{KernelApi, Ratio};
use axiom_math::Vec4;
use axiom_surface::{LightingModel, Surface, SurfaceBuilder, SurfaceChannel};

/// The markers `axiom_render::RenderPipelineKind` declares. Mirrored rather than
/// imported: a module may never depend on another module, which is exactly why
/// the backend restates them too — and why this test pins the numbers.
const PIPELINE_BASIC_LIT: u32 = 1;
const PIPELINE_UNLIT: u32 = 2;

/// A validated presentation request, the way windowing builds one.
fn request() -> HostPresentationRequest {
    let host = HostApi::new();
    let kernel = KernelApi::new();
    let viewport = host
        .viewport(320, 240, Ratio::new(1.0).expect("finite"))
        .expect("valid viewport");
    let target = host
        .presentation_target(&kernel, 1, "axiom-unlit-pipeline-test")
        .expect("valid target");
    let surface = host.surface_handle(&kernel, 2).expect("valid surface");
    let descriptor = host.surface_descriptor(
        viewport,
        HostPresentMode::Fifo,
        HostAlphaMode::Opaque,
        HostColorFormat::Bgra8UnormSrgb,
    );
    host.presentation_request(
        target,
        surface,
        descriptor,
        host.adapter_request(HostPowerPreference::HighPerformance, true),
        host.device_request(true, HostDeviceProfile::Baseline),
    )
    .expect("valid request")
}

/// One constant-channel surface under `model`.
fn surface(model: LightingModel) -> Surface {
    SurfaceBuilder::new()
        .lighting(model)
        .constant(
            SurfaceChannel::BaseColor,
            FieldValue::vec4(Vec4::new(0.2, 0.4, 0.6, 1.0)),
        )
        .build()
        .expect("a vec4 constant is legal under every lighting model")
}

#[test]
fn an_unlit_surface_makes_its_draws_select_the_unlit_pipeline_marker() {
    let backend = GpuBackendApi::new(&request());
    let surfaces: Vec<Surface> = LightingModel::ALL.iter().copied().map(surface).collect();
    let kind = |index: usize| backend.surface_pipeline_kind(&surfaces, surfaces[index].digest().raw());
    assert_eq!(kind(0), PIPELINE_UNLIT, "Unlit selects the unlit marker");
    assert_eq!(kind(1), PIPELINE_BASIC_LIT, "Lambert is lit");
    assert_eq!(kind(2), PIPELINE_BASIC_LIT, "LambertSpecular is lit");
}

/// The compatibility half: every draw that authored no surface, and every
/// program this backend was never handed, is lit — which is what the engine has
/// always done, so no existing content changes pipeline.
#[test]
fn a_draw_with_no_surface_or_an_unknown_one_is_lit() {
    let backend = GpuBackendApi::new(&request());
    let known = surface(LightingModel::Unlit);
    let set = [known.clone()];
    // `0` is the digest every draw that authored no surface carries.
    assert_eq!(backend.surface_pipeline_kind(&set, 0), PIPELINE_BASIC_LIT);
    // A digest this backend was never handed.
    assert_eq!(
        backend.surface_pipeline_kind(&set, known.digest().raw() ^ 1),
        PIPELINE_BASIC_LIT
    );
    // And a frame that authored nothing at all.
    assert_eq!(backend.surface_pipeline_kind(&[], 0), PIPELINE_BASIC_LIT);
}
