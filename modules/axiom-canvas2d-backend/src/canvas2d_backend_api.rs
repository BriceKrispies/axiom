//! The single Canvas 2D backend facade.

use std::collections::HashSet;

use axiom_host::{
    BackendKind, FrameDepthCueStats, FrameFeature, FramePacket, FrameRasterStats,
    FrameSubmissionReport, HostPresentationRequest,
};

use crate::canvas_policy::{CanvasQualityPreset, CanvasVisualProfile};
use crate::low_poly_raster_options::LowPolyRasterOptions;
use crate::mesh_cache::MeshCache;
use crate::software_rasterizer::{SoftwareRasterResult, SoftwareRasterizer};

/// The software, last-resort presentation backend for one surface.
///
/// Constructed from a validated [`HostPresentationRequest`] (a `host`-layer
/// value), from which it reads the canvas's display size. It consumes the
/// backend-neutral [`axiom_host::FramePacket`] — the same artifact the GPU
/// backend consumes — and renders it with a CPU software z-buffer rasterizer
/// into a small RGBA framebuffer (the `LowPolyFramebuffer` profile), which the
/// `wasm32` arm blits to the canvas. On native there is no canvas, so the
/// rasterizer still runs (and is fully tested) but nothing is blitted.
#[derive(Debug)]
pub struct Canvas2dBackendApi {
    width: u32,
    height: u32,
    profile: CanvasVisualProfile,
    options: LowPolyRasterOptions,
    meshes: MeshCache,
    // The real 2D context, present only once attached on wasm32. Its absence is
    // what "not yet bound" means; native never has one.
    #[cfg(target_arch = "wasm32")]
    binding: Option<crate::live_canvas_binding::LiveCanvasBinding>,
}

impl Canvas2dBackendApi {
    /// A fresh backend sized from the configured presentation request. No
    /// browser object is touched — the size is read from host-owned data — so
    /// this runs and is tested on native exactly as on the web. The internal
    /// rasterization resolution is the low-poly default, independent of the
    /// (larger) canvas display size.
    pub fn new(request: &HostPresentationRequest) -> Self {
        let viewport = request.descriptor().viewport();
        Canvas2dBackendApi {
            width: viewport.physical_width(),
            height: viewport.physical_height(),
            profile: CanvasVisualProfile::LowPolyFramebuffer,
            options: LowPolyRasterOptions::default(),
            meshes: MeshCache::default(),
            #[cfg(target_arch = "wasm32")]
            binding: None,
        }
    }

    /// Upload the mesh set the rasterizer will project, in the GPU backend's
    /// `(mesh_id, 12-float interleaved vertices, indices)` form — so windowing
    /// hands both backends the identical geometry.
    pub fn load_meshes(&mut self, meshes: &[(u64, Vec<f32>, Vec<u32>)]) {
        self.meshes = MeshCache::load(meshes);
    }

    /// Bind the real browser canvas's 2D context (wasm32 only) and switch the
    /// canvas backing store to the low internal resolution with pixelated
    /// upscale. On success later [`Self::present_packet`] calls blit real
    /// pixels; on failure the binding stays absent so the caller can fall
    /// through to "unsupported".
    #[cfg(target_arch = "wasm32")]
    pub fn attach_canvas(
        &mut self,
        canvas: &web_sys::HtmlCanvasElement,
    ) -> Result<(), wasm_bindgen::JsValue> {
        self.binding = Some(crate::live_canvas_binding::LiveCanvasBinding::attach(
            canvas,
            self.options.framebuffer_width(),
            self.options.framebuffer_height(),
            self.width,
            self.height,
        )?);
        Ok(())
    }

    /// Replace one cached mesh's geometry mid-loop (the streaming-terrain path),
    /// in the same 12-float interleaved form as [`Self::load_meshes`]. Pure cache
    /// update — no canvas needed — so it runs and is tested on native.
    pub fn replace_geometry(&mut self, mesh_id: u64, vertices: &[f32], indices: &[u32]) {
        self.meshes.replace(mesh_id, vertices, indices);
    }

    /// Select the internal-resolution quality tier (`0` = UltraLow … `3` = High,
    /// clamped). The forced-fallback default is Low; the platform arm resolves a
    /// level from a `?quality=` query, and this is the seam a future
    /// dynamic-resolution policy would drive from measured frame time. Resizing
    /// the framebuffer mid-run is supported because the binding tracks the
    /// framebuffer size on each blit.
    pub fn set_quality_level(&mut self, level: u8) {
        self.options = LowPolyRasterOptions::from_preset(CanvasQualityPreset::from_level(level));
    }

    /// Rasterize one [`FramePacket`] in the low-poly framebuffer profile and
    /// return the uniform [`FrameSubmissionReport`] (carrying the neutral
    /// [`FrameRasterStats`]). The rasterizer and report run identically on every
    /// target (so the whole path is native-tested); the resulting framebuffer is
    /// blitted by the `wasm32` arm and discarded on native.
    pub fn present_packet(&self, packet: &FramePacket) -> FrameSubmissionReport {
        // v1 ships a single visual profile; this field is the seam where a
        // future profile would select a different rasterization strategy.
        let _ = self.profile;
        // Wall-clock timing is read only on wasm (`now_ms` is 0.0 on native, so
        // the native path stays deterministic and timer-free); the pure
        // rasterizer never reads a clock.
        // Fog recedes toward the *frame's* sky: override the cue profile's fog
        // colour with the packet clear colour each frame (the "default to the
        // frame clear colour" rule), leaving every other cue knob as configured.
        let mut cues = self.options.depth_cues();
        cues.fog.color = packet.clear_color();
        let options = self.options.with_depth_cues(cues);

        let t0 = now_ms();
        let result = SoftwareRasterizer::new(options).rasterize_packet(packet, &self.meshes);
        let t1 = now_ms();
        self.blit(result.rgba_bytes(), result.width(), result.height());
        let t2 = now_ms();
        log_timing(&result, t1 - t0, t2 - t1);
        self.report(packet, &result)
    }

    /// Build the uniform host report from the rasterizer result and the packet's
    /// feature metadata.
    fn report(&self, packet: &FramePacket, result: &SoftwareRasterResult) -> FrameSubmissionReport {
        let features = packet.features();
        let degraded_features: Vec<FrameFeature> = [
            features
                .uses_textures()
                .then_some(FrameFeature::AlbedoSampling),
            features.uses_shadows().then_some(FrameFeature::Shadows),
        ]
        .into_iter()
        .flatten()
        .collect();

        let c = result.conversion();
        let raster = FrameRasterStats {
            framebuffer_width: result.width(),
            framebuffer_height: result.height(),
            projected_draws: c.projected_draws,
            projected_triangles: c.projected_triangles,
            culled_triangles: c.culled_triangles,
            rasterized_triangles: result.rasterized_triangles(),
            skipped_degenerate_triangles: c.skipped_degenerate_triangles,
            skipped_invalid_projection_triangles: c.skipped_invalid_projection_triangles,
            candidate_pixels: result.candidate_pixels(),
            depth_tested_pixels: result.depth_tested_pixels(),
            depth_written_pixels: result.depth_written_pixels(),
            depth_rejected_pixels: result.depth_rejected_pixels(),
            terrain_draws_preserved: c.terrain_draws_preserved,
            terrain_triangles_decimated: c.terrain_triangles_decimated,
            rasterized_objects: c.rasterized_objects,
            skipped_decorative_draws: c.skipped_decorative_draws,
            budget_exhausted: c.budget_exhausted,
            depth_cues: FrameDepthCueStats {
                lit_triangles: c.lit_triangles,
                height_tinted_triangles: c.height_tinted_triangles,
                distance_falloff_applied_triangles: c.distance_falloff_applied_triangles,
                depth_fog_applied_pixels: result.depth_fog_applied_pixels(),
                vertical_grade_applied_pixels: result.vertical_grade_applied_pixels(),
                contact_shadows_drawn: result.contact_shadows_drawn(),
                contact_shadow_pixels: result.contact_shadow_pixels(),
                outlined_objects: result.outlined_objects(),
                outline_pixels: result.outline_pixels(),
                horizon_silhouette_drawn: result.horizon_silhouette_drawn(),
                depth_cue_profile_name: self.options.depth_cues().name(),
            },
        };

        FrameSubmissionReport::new(
            BackendKind::Canvas2d,
            packet.frame_index(),
            packet.tick(),
            c.projected_draws,
            c.skipped_draws,
            c.critical_coverage_skipped,
            self.degraded_material_count(packet, features.uses_textures()),
            degraded_features,
            raster,
        )
    }

    /// Distinct materials referenced by drawable (mesh-present) draws — degraded
    /// when the frame wanted textures (the software path samples no albedo).
    fn degraded_material_count(&self, packet: &FramePacket, uses_textures: bool) -> u32 {
        let distinct: HashSet<u64> = packet
            .draws()
            .iter()
            .filter(|draw| self.meshes.get(draw.mesh_id()).is_some())
            .map(|draw| draw.material_id())
            .collect();
        distinct.len() as u32 * u32::from(uses_textures)
    }

    /// Blit the rasterized framebuffer to the bound canvas. wasm32 uploads it via
    /// `putImageData`; on native there is no canvas, so this is a no-op. The
    /// bytes/size are read in `present_packet` (not here) so the read is not
    /// gated behind `cfg(wasm32)`.
    #[cfg(target_arch = "wasm32")]
    fn blit(&self, rgba: &[u8], width: u32, height: u32) {
        self.binding
            .iter()
            .for_each(|b| b.blit(width, height, rgba));
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn blit(&self, _rgba: &[u8], _width: u32, _height: u32) {}

    /// The target (canvas display) width in device pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// The target (canvas display) height in device pixels.
    pub fn height(&self) -> u32 {
        self.height
    }
}

/// A monotonic millisecond clock for timing. wasm reads `performance.now()`;
/// native returns `0.0`, so the native (tested) path is deterministic and the
/// pure rasterizer is never timed.
#[cfg(target_arch = "wasm32")]
fn now_ms() -> f64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map(|p| p.now())
        .unwrap_or(0.0)
}

#[cfg(not(target_arch = "wasm32"))]
fn now_ms() -> f64 {
    0.0
}

/// Log the per-frame raster telemetry + timings (wasm only; native is a no-op so
/// the deterministic path emits nothing and reads no clock).
#[cfg(target_arch = "wasm32")]
fn log_timing(result: &SoftwareRasterResult, raster_ms: f64, blit_ms: f64) {
    let c = result.conversion();
    let msg = format!(
        "axiom-canvas2d: backend=Canvas2d profile=LowPolyFramebuffer depth_cue_profile={} {}x{} \
         raster={raster_ms:.2}ms blit={blit_ms:.2}ms draws(proj/skip)={}/{} \
         tris(proj/rast/cull/decim)={}/{}/{}/{} candidate_px={} depth(test/write/reject)={}/{}/{} \
         cues(lit/tint/falloff)={}/{}/{} fog_px={} grade_px={} shadows={}/{}px outlines={}/{}px \
         horizon={} budget_exhausted={}",
        crate::canvas_depth_cue_profile::CanvasDepthCueProfile::low_poly_framebuffer().name(),
        result.width(),
        result.height(),
        c.projected_draws,
        c.skipped_draws,
        c.projected_triangles,
        result.rasterized_triangles(),
        c.culled_triangles,
        c.terrain_triangles_decimated,
        result.candidate_pixels(),
        result.depth_tested_pixels(),
        result.depth_written_pixels(),
        result.depth_rejected_pixels(),
        c.lit_triangles,
        c.height_tinted_triangles,
        c.distance_falloff_applied_triangles,
        result.depth_fog_applied_pixels(),
        result.vertical_grade_applied_pixels(),
        result.contact_shadows_drawn(),
        result.contact_shadow_pixels(),
        result.outlined_objects(),
        result.outline_pixels(),
        result.horizon_silhouette_drawn(),
        c.budget_exhausted,
    );
    web_sys::console::log_1(&wasm_bindgen::JsValue::from_str(&msg));
}

#[cfg(not(target_arch = "wasm32"))]
fn log_timing(_result: &SoftwareRasterResult, _raster_ms: f64, _blit_ms: f64) {}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_host::{
        HostAlphaMode, HostApi, HostColorFormat, HostDeviceProfile, HostPowerPreference,
        HostPresentMode,
    };
    use axiom_kernel::{KernelApi, Ratio};

    /// Build a validated presentation request the way windowing does.
    pub(super) fn request(width: u32, height: u32) -> HostPresentationRequest {
        let host = HostApi::new();
        let kernel = KernelApi::new();
        let viewport = host
            .viewport(width, height, Ratio::new(1.0).expect("finite"))
            .expect("valid viewport");
        let target = host
            .presentation_target(&kernel, 1, "axiom-canvas2d-test")
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
            .expect("valid request")
    }

    const IDENTITY: [f32; 16] = [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ];

    fn vertex(pos: [f32; 3], color: [f32; 4]) -> [f32; 12] {
        [
            pos[0], pos[1], pos[2], 0.0, 1.0, 0.0, 0.0, 0.0, color[0], color[1], color[2], color[3],
        ]
    }

    fn ground(id: u64) -> (u64, Vec<f32>, Vec<u32>) {
        let c = [0.2, 0.6, 0.3, 1.0];
        let mut v = Vec::new();
        v.extend_from_slice(&vertex([-1.0, -1.0, 0.0], c));
        v.extend_from_slice(&vertex([1.0, -1.0, 0.0], c));
        v.extend_from_slice(&vertex([1.0, 1.0, 0.0], c));
        v.extend_from_slice(&vertex([-1.0, 1.0, 0.0], c));
        (id, v, vec![0, 1, 2, 0, 2, 3])
    }

    fn packet(draws: Vec<axiom_host::FrameDrawItem>, features: axiom_host::FrameFeatureSet) -> FramePacket {
        use axiom_host::{FrameCamera, FrameViewport};
        FramePacket::new(
            2,
            120,
            FrameViewport::new(800, 600),
            [0.4, 0.6, 0.9, 1.0],
            Some(FrameCamera::new(IDENTITY, IDENTITY, IDENTITY)),
            draws,
            Vec::new(),
            IDENTITY,
            features,
        )
    }

    #[test]
    fn new_reads_surface_size_from_the_request() {
        let backend = Canvas2dBackendApi::new(&request(800, 600));
        assert_eq!(backend.width(), 800);
        assert_eq!(backend.height(), 600);
        assert!(format!("{backend:?}").starts_with("Canvas2dBackendApi"));
    }

    #[test]
    fn presents_a_packet_to_a_canvas2d_report_with_raster_stats() {
        use axiom_host::{FrameDrawItem, FrameFeatureSet};
        let mut backend = Canvas2dBackendApi::new(&request(800, 600));
        backend.load_meshes(&[ground(7)]);
        let draws = vec![FrameDrawItem::new(1, 7, 9, IDENTITY, IDENTITY, [1.0, 0.0, 0.0, 1.0])];
        let report = backend.present_packet(&packet(draws, FrameFeatureSet::new(false, false, 0, 0)));

        assert_eq!(report.backend(), BackendKind::Canvas2d);
        assert_eq!(report.frame_index(), 2);
        assert_eq!(report.tick(), 120);
        assert_eq!(report.submitted_draws(), 1);
        assert_eq!(report.skipped_draws(), 0);
        assert_eq!(report.critical_coverage_skipped(), 0);
        // The framebuffer is the low internal resolution (Low tier 240×135), not
        // the 800x600 canvas.
        assert_eq!(report.raster().framebuffer_width, 240);
        assert_eq!(report.raster().framebuffer_height, 135);
        assert_eq!(report.raster().rasterized_triangles, 2);
        assert!(report.raster().depth_written_pixels > 0);
        assert_eq!(report.raster().terrain_draws_preserved, 1);
        assert!(report.raster().candidate_pixels > 0);
        assert!(!report.raster().budget_exhausted);
    }

    #[test]
    fn set_quality_level_changes_the_internal_resolution() {
        use axiom_host::{FrameDrawItem, FrameFeatureSet};
        let mut backend = Canvas2dBackendApi::new(&request(800, 600));
        backend.load_meshes(&[ground(7)]);
        let draws = vec![FrameDrawItem::new(1, 7, 9, IDENTITY, IDENTITY, [1.0; 4])];
        // Level 0 → UltraLow 160×90.
        backend.set_quality_level(0);
        let r0 = backend.present_packet(&packet(draws.clone(), FrameFeatureSet::new(false, false, 0, 0)));
        assert_eq!(r0.raster().framebuffer_width, 160);
        assert_eq!(r0.raster().framebuffer_height, 90);
        // Level 2 → Medium 320×180 (more candidate pixels than UltraLow).
        backend.set_quality_level(2);
        let r2 = backend.present_packet(&packet(draws, FrameFeatureSet::new(false, false, 0, 0)));
        assert_eq!(r2.raster().framebuffer_width, 320);
        assert_eq!(r2.raster().framebuffer_height, 180);
        assert!(r2.raster().candidate_pixels > r0.raster().candidate_pixels);
    }

    #[test]
    fn unknown_mesh_is_skipped_without_critical_violation() {
        use axiom_host::{FrameDrawItem, FrameFeatureSet};
        let backend = Canvas2dBackendApi::new(&request(640, 480));
        let draws = vec![FrameDrawItem::new(1, 404, 9, IDENTITY, IDENTITY, [1.0; 4])];
        let report = backend.present_packet(&packet(draws, FrameFeatureSet::new(false, false, 0, 0)));
        assert_eq!(report.submitted_draws(), 0);
        assert_eq!(report.skipped_draws(), 1);
        assert_eq!(report.critical_coverage_skipped(), 0);
        assert_eq!(report.degraded_materials(), 0);
    }

    #[test]
    fn reports_degraded_textures_and_shadows_and_materials() {
        use axiom_host::{FrameDrawItem, FrameFeatureSet};
        let mut backend = Canvas2dBackendApi::new(&request(320, 180));
        backend.load_meshes(&[ground(7)]);
        let draws = vec![FrameDrawItem::new(1, 7, 13, IDENTITY, IDENTITY, [1.0; 4])];
        let report = backend.present_packet(&packet(draws, FrameFeatureSet::new(true, true, 1, 0)));
        assert!(report
            .degraded_features()
            .contains(&FrameFeature::AlbedoSampling));
        assert!(report.degraded_features().contains(&FrameFeature::Shadows));
        assert_eq!(report.degraded_materials(), 1);
    }

    #[test]
    fn replace_geometry_keeps_the_draw_rasterizable() {
        use axiom_host::{FrameDrawItem, FrameFeatureSet};
        let mut backend = Canvas2dBackendApi::new(&request(320, 180));
        backend.load_meshes(&[ground(7)]);
        let draws = vec![FrameDrawItem::new(1, 7, 9, IDENTITY, IDENTITY, [1.0; 4])];
        let p = packet(draws, FrameFeatureSet::new(false, false, 0, 0));
        assert_eq!(backend.present_packet(&p).submitted_draws(), 1);

        let (_, v, i) = ground(7);
        backend.replace_geometry(7, &v, &i);
        assert_eq!(backend.present_packet(&p).submitted_draws(), 1);
    }
}
