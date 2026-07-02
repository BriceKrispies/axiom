//! Shared helpers for the `axiom-shot` backend-parity proofs (SPEC-04 §7 2D
//! alpha-blend parity, SPEC-11 §7 GPU↔canvas2d 3D parity).
//!
//! These mirror the render glue in `src/main.rs` (the per-frame
//! batches→FramePacket reconstruction `axiom-windowing` does) so a test can feed
//! the SAME neutral frame data to both backends and compare pixels — without
//! exporting the binary's internals.

#![allow(dead_code)]

use axiom::prelude::*;
use axiom_canvas2d_backend::Canvas2dBackendApi;
use axiom_gpu_backend::GpuBackendApi;
use axiom_host::{
    FrameCamera, FrameDrawItem, FrameFeatureSet, FrameLight, FramePacket, FrameViewport,
    HostAlphaMode, HostApi, HostColorFormat, HostDeviceProfile, HostPowerPreference,
    HostPresentMode, HostPresentationRequest,
};
use axiom_kernel::{KernelApi, Ratio};

/// Build the validated host presentation request a backend is sized from (the way
/// windowing does); the backends read only the viewport size from it.
pub fn present_request(w: u32, h: u32) -> HostPresentationRequest {
    let host = HostApi::new();
    let kernel = KernelApi::new();
    let viewport = host
        .viewport(w, h, Ratio::new(1.0).expect("finite scale"))
        .expect("valid viewport");
    let target = host
        .presentation_target(&kernel, 1, "axiom-shot-parity")
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
/// `scene_renderer` the browser's WebGPU/WebGL2 arm runs) at `w`×`h`.
pub fn render_gpu(
    meshes: &[(u64, Vec<f32>, Vec<u32>)],
    materials: &[(u64, u32, u32, Vec<u8>)],
    outcome: &FrameOutcome,
    w: u32,
    h: u32,
) -> (Vec<u8>, u32, u32) {
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
    )
    .expect("a native GPU adapter is required to render a GPU screenshot");
    (pixels, w, h)
}

/// Render a ticked frame through the software Canvas 2D backend (the same
/// rasterizer the browser's `?backend=canvas2d` path runs), fed the
/// backend-neutral `FramePacket` reconstructed from the instance batches. Returns
/// the internal low-poly framebuffer (its size is the quality tier's resolution).
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
fn frame_packet(outcome: &FrameOutcome, w: u32, h: u32) -> FramePacket {
    let batches = outcome.mesh_batches();
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
    match outcome.sdf_scene() {
        Some(scene) => packet.with_sdf(scene.clone()),
        None => packet,
    }
}

/// The maximum per-channel absolute byte difference between two equal-length RGBA8
/// buffers — the tight 2D parity metric.
pub fn max_channel_diff(a: &[u8], b: &[u8]) -> u8 {
    a.iter()
        .zip(b.iter())
        .map(|(x, y)| x.abs_diff(*y))
        .max()
        .unwrap_or(0)
}

/// The mean per-channel absolute byte difference (a secondary 2D parity metric).
pub fn mean_channel_diff(a: &[u8], b: &[u8]) -> f64 {
    let sum: u64 = a
        .iter()
        .zip(b.iter())
        .map(|(x, y)| u64::from(x.abs_diff(*y)))
        .sum();
    sum as f64 / a.len().max(1) as f64
}

/// Coarse "where are the rendered objects" stats for one RGBA8 image: the
/// normalised `(centroid_x, centroid_y)` in `[0,1]` and the coverage fraction of
/// pixels that differ from the background by more than `threshold` on any channel.
/// The background colour is sampled from the top-left corner pixel (the scenes
/// keep their geometry centred, so the corner is always clear colour). This is the
/// resolution-independent 3D parity metric: two backends that draw the same
/// objects at the same screen positions agree on centroid + coverage even though
/// their shading and resolution differ.
pub fn region_stats(px: &[u8], w: u32, h: u32, threshold: u8) -> (f64, f64, f64) {
    let bg = [px[0], px[1], px[2]];
    let mut count = 0u64;
    let mut sx = 0f64;
    let mut sy = 0f64;
    (0..h).for_each(|y| {
        (0..w).for_each(|x| {
            let i = ((y * w + x) * 4) as usize;
            let d = px[i]
                .abs_diff(bg[0])
                .max(px[i + 1].abs_diff(bg[1]))
                .max(px[i + 2].abs_diff(bg[2]));
            (d > threshold).then(|| {
                count += 1;
                sx += f64::from(x);
                sy += f64::from(y);
            });
        })
    });
    let denom = count.max(1) as f64;
    (
        sx / denom / f64::from(w),
        sy / denom / f64::from(h),
        count as f64 / f64::from(w * h),
    )
}

/// The coverage fraction of object pixels in the left and right halves of an
/// image (objects = differ from the corner background by > `threshold`). Used to
/// assert both backends place an object in each half (the cube on one side, the
/// cylinder on the other).
pub fn half_coverage(px: &[u8], w: u32, h: u32, threshold: u8) -> (f64, f64) {
    let bg = [px[0], px[1], px[2]];
    let mid = w / 2;
    let mut left = 0u64;
    let mut right = 0u64;
    for y in 0..h {
        for x in 0..w {
            let i = ((y * w + x) * 4) as usize;
            let d = px[i]
                .abs_diff(bg[0])
                .max(px[i + 1].abs_diff(bg[1]))
                .max(px[i + 2].abs_diff(bg[2]));
            if d > threshold {
                if x < mid {
                    left += 1;
                } else {
                    right += 1;
                }
            }
        }
    }
    let half = f64::from((w / 2) * h);
    (left as f64 / half, right as f64 / half)
}
