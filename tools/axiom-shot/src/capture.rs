//! The one shared offscreen-capture routine.
//!
//! Both the `axiom-shot` binary and the backend-parity tests render through
//! these functions, so the per-frame `batches → FramePacket` reconstruction (the
//! same one `axiom-windowing` does for its Canvas arm), the host presentation
//! request a backend is sized from, and the PNG writer all live here once,
//! instead of being copied per call site.
//!
//! The GPU arm ([`render_gpu`], [`render_draw2d_gpu`]) is compiled only behind
//! the crate's `offscreen` feature (real native wgpu); the canvas2d arm and the
//! neutral `FramePacket` reconstruction are always available.

use axiom::prelude::*;
use axiom_canvas2d_backend::Canvas2dBackendApi;
use axiom_host::{
    FrameCamera, FrameDrawItem, FrameFeatureSet, FrameLight, FramePacket, FrameViewport,
    HostAlphaMode, HostApi, HostColorFormat, HostDeviceProfile, HostPowerPreference,
    HostPresentMode, HostPresentationRequest,
};
use axiom_kernel::{KernelApi, Ratio};

/// Build the validated host presentation request a backend is sized from (the
/// way windowing does); a backend reads only the viewport size from it.
pub fn present_request(w: u32, h: u32) -> HostPresentationRequest {
    let host = HostApi::new();
    let kernel = KernelApi::new();
    let viewport = host
        .viewport(w, h, Ratio::new(1.0).expect("finite scale"))
        .expect("valid viewport");
    let target = host
        .presentation_target(&kernel, 1, "axiom-shot")
        .expect("valid target");
    let surface = host.surface_handle(&kernel, 2).expect("valid surface");
    let descriptor = host.surface_descriptor(
        viewport,
        HostPresentMode::Fifo,
        HostAlphaMode::Opaque,
        HostColorFormat::Bgra8UnormSrgb,
    );
    let adapter = host.adapter_request(HostPowerPreference::HighPerformance, true);
    let device = host.device_request(true, HostDeviceProfile::Baseline);
    host.presentation_request(target, surface, descriptor, adapter, device)
        .expect("valid presentation request")
}

/// Render a ticked frame through the native off-screen GPU path (the same
/// `scene_renderer` the browser's WebGPU/WebGL2 arm runs) at `w`×`h`. `offscreen`
/// feature only.
#[cfg(feature = "offscreen")]
#[allow(clippy::too_many_arguments)]
pub fn render_gpu(
    meshes: &[(u64, Vec<f32>, Vec<u32>)],
    materials: &[(u64, u32, u32, Vec<u8>)],
    outcome: &FrameOutcome,
    w: u32,
    h: u32,
    retro_32bit: Option<axiom_host::FrameRetro32BitProfile>,
) -> (Vec<u8>, u32, u32) {
    use axiom_gpu_backend::GpuBackendApi;
    let batches = outcome.mesh_batches();
    let lights: Vec<(u32, [f32; 3], [f32; 3], f32)> = outcome
        .lights()
        .iter()
        .map(|l| (l.kind(), l.vec(), l.color(), l.intensity()))
        .collect();
    let pixels = GpuBackendApi::render_offscreen_rgba(
        w,
        h,
        meshes,
        materials,
        &[],
        &lights,
        outcome.light_view_proj(),
        &batches,
        outcome.clear_color(),
        outcome.sdf_scene(),
        axiom_host::FrameAmbient::default_hemisphere(),
        retro_32bit,
        // Full-fidelity reference render; volumetrics/post-process aren't carried
        // on the FrameOutcome here.
        axiom_host::BackendCapabilityProfile::all(),
        None,
        None,
    )
    .expect("a native GPU adapter is required to render a GPU screenshot");
    (pixels, w, h)
}

/// Render a ticked frame through the software Canvas 2D backend (the same
/// rasterizer the browser's `?backend=canvas2d` path runs), fed the
/// backend-neutral `FramePacket` reconstructed from the instance batches.
/// Returns the internal low-poly framebuffer (its size is the quality tier's
/// resolution).
pub fn render_canvas2d(
    meshes: &[(u64, Vec<f32>, Vec<u32>)],
    outcome: &FrameOutcome,
    quality: u8,
    w: u32,
    h: u32,
) -> (Vec<u8>, u32, u32) {
    let mut backend = Canvas2dBackendApi::new(&present_request(w, h));
    backend.load_meshes(meshes);
    backend.set_quality_level(quality);
    backend.render_offscreen_rgba(&frame_packet(outcome, w, h))
}

/// Reconstruct the backend-neutral frame packet from the per-`(mesh, material)`
/// instance batches, exactly as `axiom-windowing` does for its Canvas arm: each
/// 36-float instance is `mvp(16) | world(16) | colour(4)`, object ids assigned in
/// draw order.
pub fn frame_packet(outcome: &FrameOutcome, w: u32, h: u32) -> FramePacket {
    let batches = outcome.mesh_batches();
    // The caster flags align with the `mesh_batches` instance expansion order.
    let casters = outcome.mesh_batch_casters();
    let mut draws = Vec::new();
    let mut object_id: u64 = 0;
    for (mesh_id, material_id, floats, count) in &batches {
        for i in 0..*count {
            let off = i as usize * 36;
            let mvp: [f32; 16] = floats[off..off + 16].try_into().unwrap_or([0.0; 16]);
            let world: [f32; 16] = floats[off + 16..off + 32].try_into().unwrap_or([0.0; 16]);
            let color: [f32; 4] = floats[off + 32..off + 36].try_into().unwrap_or([1.0; 4]);
            let casts = casters.get(object_id as usize).copied().unwrap_or(false);
            draws.push(FrameDrawItem::new(
                object_id, *mesh_id, *material_id, world, mvp, color, casts,
            ));
            object_id += 1;
        }
    }
    let lights: Vec<FrameLight> = outcome
        .lights()
        .iter()
        .map(|l| {
            let c = l.color();
            FrameLight::new(l.kind(), l.vec(), [c[0], c[1], c[2], l.intensity()])
        })
        .collect();
    let directional = outcome.lights().iter().filter(|l| l.kind() == 0).count() as u32;
    let point = outcome.lights().iter().filter(|l| l.kind() == 1).count() as u32;
    let features = FrameFeatureSet::new(false, directional > 0, directional, point);
    // The Canvas planar-shadow pass projects caster geometry through the camera;
    // view/projection are unused by the software path, so identity is fine.
    let identity = [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0_f32,
    ];
    let camera = Some(FrameCamera::new(identity, identity, outcome.camera_view_proj()));
    let packet = FramePacket::new(
        outcome.tick(),
        outcome.tick(),
        FrameViewport::new(w, h),
        outcome.clear_color(),
        camera,
        draws,
        lights,
        outcome.light_view_proj(),
        features,
    );
    // Attach the frame's SDF scene (if any) so the Canvas2D backend marches and
    // composites the raymarched shapes against the rasterized meshes.
    match outcome.sdf_scene() {
        Some(scene) => packet.with_sdf(scene.clone()),
        None => packet,
    }
}

/// Write an RGBA8 buffer to a PNG at `path` (creating parent directories).
pub fn write_png(path: &str, rgba: &[u8], width: u32, height: u32) {
    if let Some(parent) = std::path::Path::new(path).parent() {
        std::fs::create_dir_all(parent).expect("create output directory");
    }
    let file = std::fs::File::create(path).expect("create PNG file");
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().expect("write PNG header");
    writer.write_image_data(rgba).expect("write PNG data");
}
