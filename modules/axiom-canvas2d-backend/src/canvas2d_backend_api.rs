//! The single Canvas 2D backend facade.

use std::collections::HashSet;

use axiom_host::{
    BackendKind, Draw2dList, FrameDepthCueStats, FrameDrawItem, FrameFeature, FramePacket,
    FrameRasterStats, FrameSubmissionReport, HostPresentationRequest, RenderCapability,
};

use crate::canvas_policy::{CanvasQualityPreset, CanvasVisualProfile};
use crate::draw2d_raster::Draw2dTextures;
use crate::low_poly_raster_options::LowPolyRasterOptions;
use crate::mesh_cache::{MeshCache, MeshGeometry};
use crate::mesh_skinning::SkinnedMeshCache;
use crate::software_rasterizer::{SoftwareRasterResult, SoftwareRasterizer};

/// One skinned draw as neutral data — the same tuple the GPU backend and
/// windowing already pass: `(mesh_id, material_id, mvp, world, colour, joint
/// palette)`. The palette is this frame's column-major joint matrices.
#[allow(clippy::type_complexity)]
pub type SkinnedDraw = (u64, u64, [f32; 16], [f32; 16], [f32; 4], Vec<[f32; 16]>);

/// Object-id base for CPU-skinned draws, kept clear of the packet's draw-order
/// ids (assigned `0..draw_count`) so the two never collide in the rasterizer's
/// per-object stats.
const SKINNED_OBJECT_BASE: u64 = 1 << 48;

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
    // The bake-once skinned mesh set (20-float streams), uploaded at bind like
    // `meshes`. Each frame's skinned draws are CPU-posed against it (the software
    // peer of the GPU skinning pass). Empty for apps that submit no skinned bodies.
    skinned_meshes: SkinnedMeshCache,
    // CPU sprite/atlas textures the 2D Draw2dList consumer samples (uploaded by
    // the app, the same fetch-in-the-app rule the mesh/material path follows).
    textures: Draw2dTextures,
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
        let width = viewport.physical_width();
        let height = viewport.physical_height();
        Canvas2dBackendApi {
            width,
            height,
            profile: CanvasVisualProfile::LowPolyFramebuffer,
            // The internal framebuffer preserves the SURFACE aspect (not a fixed
            // 16:9), so the software image is the same shape the GPU renders and
            // upscales without vertical distortion. The default profile is the flat
            // rasterizer's real capability set (`canvas2d()`): it drops the shader-only
            // capabilities and substitutes the PCF shadow with a planar contact shadow,
            // so the live backend degrades from the one full-richness frame instead of
            // being handed default `all()` and silently no-op'ing what it can't do.
            options: LowPolyRasterOptions::from_preset_for_surface(
                CanvasQualityPreset::Low,
                width,
                height,
            )
            .with_capability_profile(axiom_host::BackendCapabilityProfile::canvas2d()),
            meshes: MeshCache::default(),
            skinned_meshes: SkinnedMeshCache::default(),
            textures: Draw2dTextures::default(),
            #[cfg(target_arch = "wasm32")]
            binding: None,
        }
    }

    /// Restrict which optional render capabilities this backend attempts (the default
    /// is [`axiom_host::BackendCapabilityProfile::all`] — attempt everything, like the
    /// GPU backends). The config lever for keeping Canvas 2D legible and fast:
    /// e.g. `all().without(RenderCapability::Volumetrics)` makes it skip the god-ray
    /// pass while the WebGPU / WebGL2 backends keep it.
    pub fn set_capability_profile(&mut self, profile: axiom_host::BackendCapabilityProfile) {
        self.options = self.options.with_capability_profile(profile);
    }

    /// Upload the CPU sprite/atlas textures the 2D [`Draw2dList`] sprite path
    /// samples, as `(texture_id, width, height, RGBA8 pixels)` — the same upload
    /// shape as the 3D material set. Resolved in the app (fetch/decode); the
    /// backend only ever names the id.
    pub fn load_textures(&mut self, textures: &[(u64, u32, u32, Vec<u8>)]) {
        self.textures = Draw2dTextures::load(textures);
    }

    /// Composite a host-neutral [`Draw2dList`] onto a fresh framebuffer at the
    /// canvas display size and return the finished `(rgba8 bytes, width, height)`
    /// — the 2D analogue of [`Self::render_offscreen_rgba`]. Each command's
    /// resolved `layer` (the list is pre-sorted), `alpha`, and baked transform are
    /// honoured, with **src-over alpha compositing** so translucent draws blend
    /// over what is beneath them. Pure; no canvas touched, so it is native-tested.
    pub fn render_draw2d_rgba(&self, list: &Draw2dList) -> (Vec<u8>, u32, u32) {
        crate::draw2d_raster::render(list, self.width, self.height, &self.textures)
    }

    /// Present one host-neutral [`Draw2dList`] live to the bound canvas (wasm32):
    /// rasterize it at the canvas display size ([`Self::render_draw2d_rgba`], which
    /// composites the layer-sorted commands over a transparent framebuffer), flatten
    /// that over the opaque `clear` background, and blit. Compositing the
    /// transparent result over `clear` is — by src-over associativity — identical to
    /// drawing the commands directly over `clear`, which is exactly what the GPU 2D
    /// arm does (clear, then alpha-blend), so the two backends present the same 2D
    /// frame. A no-op until [`Self::attach_canvas`] has bound a context.
    #[cfg(target_arch = "wasm32")]
    pub fn present_draw2d(&self, list: &Draw2dList, clear: [f32; 4]) {
        let (rgba, width, height) = self.render_draw2d_rgba(list);
        let opaque = flatten_over_clear(rgba, clear);
        self.blit(&opaque, width, height);
    }

    /// Upload the mesh set the rasterizer will project, in the GPU backend's
    /// `(mesh_id, 12-float interleaved vertices, indices)` form — so windowing
    /// hands both backends the identical geometry.
    pub fn load_meshes(&mut self, meshes: &[(u64, Vec<f32>, Vec<u32>)]) {
        self.meshes = MeshCache::load(meshes);
    }

    /// Upload the **skinned** mesh set — the bake-once bodies in the GPU backend's
    /// 20-float `(mesh_id, pos·normal·uv·colour·joints·weights, indices)` form,
    /// distinct from the ordinary `load_meshes` set. Uploaded once at bind (the
    /// per-frame joint palettes ride in on the skinned draws passed to
    /// [`Self::present_packet_skinned`] / [`Self::render_offscreen_rgba_skinned`]),
    /// so the software rasterizer CPU-skins them the way the GPU backend skins on
    /// the vertex stage. Empty for apps that submit no skinned bodies.
    pub fn load_skinned_meshes(&mut self, meshes: &[(u64, Vec<f32>, Vec<u32>)]) {
        self.skinned_meshes = SkinnedMeshCache::load(meshes);
    }

    /// CPU-skin this frame's skinned draws into drawable `(geometry, draw)` pairs:
    /// pose each against the uploaded skinned mesh set by its joint palette, and
    /// synthesize a draw carrying the same `mvp`/`world`/`colour`/`material` the
    /// GPU skinned pass uses (so the rasterizer projects + lights it identically).
    /// A draw whose mesh isn't in the skinned set is dropped (`filter_map`).
    fn pose_skinned(&self, skinned: &[SkinnedDraw]) -> Vec<(MeshGeometry, FrameDrawItem)> {
        skinned
            .iter()
            .enumerate()
            .filter_map(|(i, (mesh_id, material_id, mvp, world, color, palette))| {
                self.skinned_meshes.pose(*mesh_id, palette, mvp).map(|geo| {
                    let draw = FrameDrawItem::new(
                        SKINNED_OBJECT_BASE + i as u64,
                        *mesh_id,
                        *material_id,
                        *world,
                        *mvp,
                        *color,
                        false,
                    );
                    (geo, draw)
                })
            })
            .collect()
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
    /// level from a `?quality=` query. Resizing the framebuffer mid-run is
    /// supported because the binding tracks the framebuffer size on each blit.
    pub fn set_quality_level(&mut self, level: u8) {
        // Preserve the capability profile across a quality change (it is independent of
        // the resolution tier, so a `set_quality_level` must not wipe a configured one).
        let capability = self.options.capability_profile();
        self.options = LowPolyRasterOptions::from_preset_for_surface(
            CanvasQualityPreset::from_level(level),
            self.width,
            self.height,
        )
        .with_capability_profile(capability);
    }


    /// Rasterize one [`FramePacket`] in the low-poly framebuffer profile and
    /// return the uniform [`FrameSubmissionReport`] (carrying the neutral
    /// [`FrameRasterStats`]). The rasterizer and report run identically on every
    /// target (so the whole path is native-tested); the resulting framebuffer is
    /// blitted by the `wasm32` arm and discarded on native.
    pub fn present_packet(&self, packet: &FramePacket) -> FrameSubmissionReport {
        self.present_packet_skinned(packet, &[])
    }

    /// Like [`Self::present_packet`], but also CPU-skins `skinned` — this frame's
    /// bake-once skinned bodies (each with its own joint palette) — against the set
    /// uploaded by [`Self::load_skinned_meshes`], so the software backend renders
    /// the athletes the GPU skinning pass would. `present_packet` passes none.
    pub fn present_packet_skinned(
        &self,
        packet: &FramePacket,
        skinned: &[SkinnedDraw],
    ) -> FrameSubmissionReport {
        // Wall-clock timing is read only on wasm (`now_ms` is 0.0 on native, so
        // the native path stays deterministic and timer-free); the pure
        // rasterizer never reads a clock.
        let t0 = now_ms();
        let result = self.rasterize(packet, skinned);
        let t1 = now_ms();
        self.blit(result.rgba_bytes(), result.width(), result.height());
        let t2 = now_ms();
        log_timing(&result, t1 - t0, t2 - t1);
        self.report(packet, &result)
    }

    /// Rasterize one [`FramePacket`] into the low-poly framebuffer and return the
    /// finished image as `(rgba8 bytes, width, height)` — the *exact* pixels the
    /// `wasm32` arm would blit, with no canvas touched. This is the software
    /// analogue of [`axiom_gpu_backend::GpuBackendApi::render_offscreen_rgba`]:
    /// it lets a headless tool or test capture and inspect the Canvas 2D image
    /// natively (e.g. to reproduce a rendering artifact without a browser).
    pub fn render_offscreen_rgba(&self, packet: &FramePacket) -> (Vec<u8>, u32, u32) {
        self.render_offscreen_rgba_skinned(packet, &[])
    }

    /// Like [`Self::render_offscreen_rgba`], but also CPU-skins `skinned` (this
    /// frame's bake-once skinned bodies) against the [`Self::load_skinned_meshes`]
    /// set — the software analogue of
    /// [`axiom_gpu_backend::GpuBackendApi::render_offscreen_rgba`]'s skinned draws,
    /// so a headless capture (axiom-shot) renders the athletes on Canvas 2D too.
    pub fn render_offscreen_rgba_skinned(
        &self,
        packet: &FramePacket,
        skinned: &[SkinnedDraw],
    ) -> (Vec<u8>, u32, u32) {
        let result = self.rasterize(packet, skinned);
        (
            result.rgba_bytes().to_vec(),
            result.width(),
            result.height(),
        )
    }

    /// The shared rasterization step behind both [`Self::present_packet`] and
    /// [`Self::render_offscreen_rgba`]: build the per-frame cue options (the fog
    /// recedes toward the *frame's* sky — override the cue profile's fog colour
    /// with the packet clear colour each frame, leaving every other knob as
    /// configured) and run the pure software z-buffer rasterizer.
    fn rasterize(&self, packet: &FramePacket, skinned: &[SkinnedDraw]) -> SoftwareRasterResult {
        // Only one visual profile exists; this avoids an unused-field warning.
        let _ = self.profile;
        let mut cues = self.options.depth_cues();
        cues.fog.color = packet.clear_color();
        // The frame's hemisphere ambient drives the software lighting too, matching the
        // GPU path's ambient uniform. Colours are strength-folded, so the ambient scale
        // is 1.0; an absent frame ambient falls back to the engine default hemisphere.
        let amb = packet.ambient().copied().unwrap_or_else(axiom_host::FrameAmbient::default_hemisphere);
        cues.lighting.sky_color = amb.sky();
        cues.lighting.ground_color = amb.ground();
        cues.lighting.ambient = 1.0;
        let options = self.options.with_depth_cues(cues);
        // CPU-skin this frame's skinned bodies into drawable geometry (the software
        // peer of the GPU vertex-skinning pass); empty for non-skinned frames.
        let posed = self.pose_skinned(skinned);
        // `now_ms` is the injected phase clock and `log_phases` the phase sink:
        // both real on wasm (`performance.now()` + a console line), no-ops on native
        // — so the pure rasterizer stays clock- and `web_sys`-free, and the native
        // path stays deterministic (every phase time reads 0 and is discarded).
        SoftwareRasterizer::new(options)
            .with_clock(now_ms)
            .with_phase_sink(log_phases)
            .with_deep_sink(deep_log)
            .rasterize_packet(packet, &self.meshes, &posed)
    }

    /// Build the uniform host report from the rasterizer result and the packet's
    /// feature metadata.
    fn report(&self, packet: &FramePacket, result: &SoftwareRasterResult) -> FrameSubmissionReport {
        let features = packet.features();
        // A feature is degraded iff the frame relies on it AND this backend's capability
        // profile does not provide it — the declared policy, not blanket telemetry.
        // Albedo sampling is a reported drop (flat colour); the directional shadow is
        // reported here and substituted with a planar contact shadow in the rasterizer
        // (see `RenderCapability::degradation`). `&` (not `&&`) keeps this branchless.
        let profile = self.options.capability_profile();
        let textures_degraded =
            features.uses_textures() & !profile.contains(RenderCapability::Textures);
        let shadows_degraded =
            features.uses_shadows() & !profile.contains(RenderCapability::Shadows);
        let degraded_features: Vec<FrameFeature> = [
            textures_degraded.then_some(FrameFeature::AlbedoSampling),
            shadows_degraded.then_some(FrameFeature::Shadows),
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
            self.degraded_material_count(packet, textures_degraded),
            degraded_features,
            raster,
        )
    }

    /// Distinct materials referenced by drawable (mesh-present) draws — degraded
    /// when the frame wanted textures but this backend's profile drops the
    /// [`RenderCapability::Textures`] capability (the flat software path samples no
    /// albedo).
    fn degraded_material_count(&self, packet: &FramePacket, textures_degraded: bool) -> u32 {
        let distinct: HashSet<u64> = packet
            .draws()
            .iter()
            .filter(|draw| self.meshes.get(draw.mesh_id()).is_some())
            .map(|draw| draw.material_id())
            .collect();
        distinct.len() as u32 * u32::from(textures_degraded)
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

/// Flatten a straight-alpha RGBA8 image (the 2D rasterizer's transparent-background
/// output) onto an opaque `clear` background, returning opaque RGBA8 bytes — one
/// final src-over per pixel (`out = src·a + clear·(1−a)`, `out_a = 255`). By
/// src-over associativity this equals drawing the commands directly over `clear`,
/// the GPU 2D arm's clear-then-blend, so the two backends present the same frame.
/// wasm32 only (the live present path); branchless for consistency with the module.
#[cfg(target_arch = "wasm32")]
fn flatten_over_clear(mut rgba: Vec<u8>, clear: [f32; 4]) -> Vec<u8> {
    let to_byte = |c: f32| (c.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    rgba.chunks_exact_mut(4).for_each(|px| {
        let a = f32::from(px[3]) / 255.0;
        let inv = 1.0 - a;
        px[0] = to_byte(f32::from(px[0]) / 255.0 * a + clear[0] * inv);
        px[1] = to_byte(f32::from(px[1]) / 255.0 * a + clear[1] * inv);
        px[2] = to_byte(f32::from(px[2]) / 255.0 * a + clear[2] * inv);
        px[3] = 255;
    });
    rgba
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

/// The phase sink installed on the rasterizer: log the coarse `convert` /
/// `rasterize` / `post` millisecond split as its own console line (wasm only;
/// native is a no-op, so the deterministic path emits nothing). `convert` is the
/// dominant Canvas2D cost — this line is what the render benchmark parses.
#[cfg(target_arch = "wasm32")]
fn log_phases(convert_ms: f64, rasterize_ms: f64, post_ms: f64) {
    let msg = format!(
        "axiom-canvas2d PROFILE: convert={convert_ms:.1}ms rasterize={rasterize_ms:.1}ms post={post_ms:.1}ms"
    );
    web_sys::console::log_1(&wasm_bindgen::JsValue::from_str(&msg));
}

#[cfg(not(target_arch = "wasm32"))]
fn log_phases(_convert_ms: f64, _rasterize_ms: f64, _post_ms: f64) {}

/// The deep sink installed on the rasterizer: log the `convert`-phase project/shade
/// split as its own `axiom-canvas2d DEEP:` console line. The timing that feeds it is
/// gated to a **debug wasm** build, so this logger only ever sees a non-zero split
/// there; native and release-wasm hand it a zero split (or, on native, nothing at
/// all beyond the deterministic one-shot the discard path exercises). The render
/// benchmark's `--debug` mode parses this line.
#[cfg(all(target_arch = "wasm32", debug_assertions))]
fn deep_log(project_ms: f64, shade_ms: f64, draws: u32, triangles: usize) {
    let msg = format!(
        "axiom-canvas2d DEEP: project={project_ms:.1}ms shade={shade_ms:.1}ms draws={draws} tris={triangles}"
    );
    web_sys::console::log_1(&wasm_bindgen::JsValue::from_str(&msg));
}

#[cfg(not(all(target_arch = "wasm32", debug_assertions)))]
fn deep_log(_project_ms: f64, _shade_ms: f64, _draws: u32, _triangles: usize) {}

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

    fn packet(
        draws: Vec<axiom_host::FrameDrawItem>,
        features: axiom_host::FrameFeatureSet,
    ) -> FramePacket {
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
        let draws = vec![FrameDrawItem::new(
            1,
            7,
            9,
            IDENTITY,
            IDENTITY,
            [1.0, 0.0, 0.0, 1.0],
            false,
        )];
        let report =
            backend.present_packet(&packet(draws, FrameFeatureSet::new(false, false, 0, 0)));

        assert_eq!(report.backend(), BackendKind::Canvas2d);
        assert_eq!(report.frame_index(), 2);
        assert_eq!(report.tick(), 120);
        assert_eq!(report.submitted_draws(), 1);
        assert_eq!(report.skipped_draws(), 0);
        assert_eq!(report.critical_coverage_skipped(), 0);
        // The framebuffer is the low internal resolution, aspect-matched to the
        // 800×600 (4:3) surface — Low tier's 240 long-edge budget → 240×180, not a
        // distorting fixed 16:9, and not the full 800×600 canvas.
        assert_eq!(report.raster().framebuffer_width, 240);
        assert_eq!(report.raster().framebuffer_height, 180);
        assert_eq!(report.raster().rasterized_triangles, 2);
        assert!(report.raster().depth_written_pixels > 0);
        assert_eq!(report.raster().terrain_draws_preserved, 1);
        assert!(report.raster().candidate_pixels > 0);
        assert!(!report.raster().budget_exhausted);
    }

    /// One skinned quad vertex: the 20-float stream (pos·normal·uv·colour·
    /// joints·weights) fully weighted to bone 0.
    fn skinned_vertex(pos: [f32; 3], color: [f32; 4]) -> [f32; 20] {
        [
            pos[0], pos[1], pos[2], // position
            0.0, 1.0, 0.0, // normal
            0.0, 0.0, // uv
            color[0], color[1], color[2], color[3], // colour
            0.0, 0.0, 0.0, 0.0, // joints (bone 0)
            1.0, 0.0, 0.0, 0.0, // weights (all on bone 0)
        ]
    }

    fn skinned_quad(id: u64) -> (u64, Vec<f32>, Vec<u32>) {
        let c = [0.9, 0.1, 0.1, 1.0];
        let mut v = Vec::new();
        v.extend_from_slice(&skinned_vertex([-0.5, -0.5, 0.0], c));
        v.extend_from_slice(&skinned_vertex([0.5, -0.5, 0.0], c));
        v.extend_from_slice(&skinned_vertex([0.5, 0.5, 0.0], c));
        v.extend_from_slice(&skinned_vertex([-0.5, 0.5, 0.0], c));
        (id, v, vec![0, 1, 2, 0, 2, 3])
    }

    #[test]
    fn skinned_body_renders_via_cpu_skinning() {
        use axiom_host::FrameFeatureSet;
        let mut backend = Canvas2dBackendApi::new(&request(800, 600));
        // A bake-once skinned quad, no ordinary meshes at all — so anything drawn
        // came exclusively through the CPU skinning path.
        backend.load_skinned_meshes(&[skinned_quad(3)]);
        // One skinned draw, identity palette (bone 0 = identity) → posed at bind.
        let skinned = vec![(3_u64, 9_u64, IDENTITY, IDENTITY, [1.0, 1.0, 1.0, 1.0], vec![IDENTITY])];
        let p = packet(Vec::new(), FrameFeatureSet::new(false, false, 0, 0));

        let report = backend.present_packet_skinned(&p, &skinned);
        // The skinned quad's two triangles were projected + rasterized — the
        // athlete geometry the plain `present_packet` (no skinned) would drop.
        assert_eq!(report.raster().rasterized_triangles, 2);
        assert!(report.raster().depth_written_pixels > 0);

        // The offscreen peer paints the same body into the RGBA buffer.
        let (rgba, w, h) = backend.render_offscreen_rgba_skinned(&p, &skinned);
        assert_eq!(rgba.len() as u32, w * h * 4);
        // Some pixel reads red-dominant (the quad's colour), distinct from the
        // bluish clear — proof the skinned geometry actually shaded pixels.
        assert!(rgba.chunks_exact(4).any(|px| px[0] > px[2]));
    }

    #[test]
    fn skinned_draw_with_unloaded_mesh_is_dropped() {
        use axiom_host::FrameFeatureSet;
        let backend = Canvas2dBackendApi::new(&request(800, 600));
        // No skinned mesh uploaded, so the draw's mesh id resolves to nothing and
        // the draw is dropped before rasterization (nothing paints).
        let skinned = vec![(99_u64, 0_u64, IDENTITY, IDENTITY, [1.0; 4], vec![IDENTITY])];
        let p = packet(Vec::new(), FrameFeatureSet::new(false, false, 0, 0));
        let report = backend.present_packet_skinned(&p, &skinned);
        assert_eq!(report.raster().rasterized_triangles, 0);
    }

    #[test]
    fn render_offscreen_rgba_returns_the_blittable_framebuffer() {
        use axiom_host::{FrameDrawItem, FrameFeatureSet};
        let mut backend = Canvas2dBackendApi::new(&request(800, 600));
        backend.load_meshes(&[ground(7)]);
        // Low tier at the 800×600 (4:3) surface → a 240×180 internal framebuffer
        // (aspect-matched to the surface, the forced-fallback default tier).
        backend.set_quality_level(1);
        let draws = vec![FrameDrawItem::new(
            1,
            7,
            9,
            IDENTITY,
            IDENTITY,
            [1.0, 0.0, 0.0, 1.0],
            false,
        )];
        let p = packet(draws, FrameFeatureSet::new(false, false, 0, 0));

        let (rgba, w, h) = backend.render_offscreen_rgba(&p);
        // The dimensions are the internal raster resolution, and the buffer is a
        // tight RGBA8 image of exactly that size.
        assert_eq!((w, h), (240, 180));
        assert_eq!(rgba.len() as u32, w * h * 4);
        // It is the same framebuffer `present_packet` would blit: same size, and
        // every pixel opaque.
        let report = backend.present_packet(&p);
        assert_eq!(report.raster().framebuffer_width, w);
        assert_eq!(report.raster().framebuffer_height, h);
        assert!(rgba.chunks_exact(4).all(|px| px[3] == 255));
        // Pure function of the packet: identical bytes every call.
        let (again, _, _) = backend.render_offscreen_rgba(&p);
        assert_eq!(rgba, again);
    }

    #[test]
    fn frame_ambient_lifts_the_lit_result() {
        use axiom_host::{FrameAmbient, FrameDrawItem, FrameFeatureSet};
        let mut backend = Canvas2dBackendApi::new(&request(800, 600));
        backend.load_meshes(&[ground(7)]);
        backend.set_quality_level(1);
        let draws = vec![FrameDrawItem::new(1, 7, 9, IDENTITY, IDENTITY, [1.0, 1.0, 1.0, 1.0], false)];
        // No directional light → the ground is lit by the hemisphere ambient alone.
        let base = packet(draws.clone(), FrameFeatureSet::new(false, false, 0, 0));
        let (dim, _, _) = backend.render_offscreen_rgba(&base);
        // A bright frame ambient (the `Some` path) lifts the ground above the default.
        let bright = base.clone().with_ambient(FrameAmbient::new([0.95, 0.95, 0.95], [0.95, 0.95, 0.95]));
        let (lit, _, _) = backend.render_offscreen_rgba(&bright);
        assert_ne!(dim, lit);
        assert!(dim.iter().zip(&lit).any(|(d, l)| l > d));
    }

    #[test]
    fn set_capability_profile_gates_the_volumetric_pass() {
        use axiom_host::{
            BackendCapabilityProfile, FrameCamera, FrameFeatureSet, FrameLight, FrameViewport,
            FrameVolumetrics, RenderCapability,
        };
        // A view_proj with m[11] = 1 puts a +z to-light on-screen, and a bright uniform
        // frame (no draws → just the bright clear) exceeds the god-ray leak threshold, so
        // the pass produces a real difference only when a backend runs it.
        let front_vp = [
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 1.0,
        ];
        let vol = FramePacket::new(
            0,
            0,
            FrameViewport::new(800, 600),
            [0.9, 0.9, 0.9, 1.0],
            Some(FrameCamera::new(IDENTITY, IDENTITY, front_vp)),
            Vec::new(),
            vec![FrameLight::new(0, [0.0, 0.0, 1.0], [1.0, 1.0, 1.0, 1.0])],
            IDENTITY,
            FrameFeatureSet::new(false, true, 1, 0),
        )
        .with_volumetrics(FrameVolumetrics::low_poly());
        // Default profile (all): the god-ray pass runs.
        let full = Canvas2dBackendApi::new(&request(800, 600));
        let (a, _, _) = full.render_offscreen_rgba(&vol);
        // set_capability_profile restricting Volumetrics: the pass is skipped.
        let mut restricted = Canvas2dBackendApi::new(&request(800, 600));
        restricted.set_capability_profile(
            BackendCapabilityProfile::all().without(RenderCapability::Volumetrics),
        );
        let (b, _, _) = restricted.render_offscreen_rgba(&vol);
        assert_ne!(a, b, "set_capability_profile gates the god-ray pass on Canvas 2D");
    }

    #[test]
    fn set_quality_level_changes_the_internal_resolution() {
        use axiom_host::{FrameDrawItem, FrameFeatureSet};
        let mut backend = Canvas2dBackendApi::new(&request(800, 600));
        backend.load_meshes(&[ground(7)]);
        let draws = vec![FrameDrawItem::new(
            1, 7, 9, IDENTITY, IDENTITY, [1.0; 4], false,
        )];
        // Level 0 → UltraLow, 160×120 at the 800×600 (4:3) surface.
        backend.set_quality_level(0);
        let r0 = backend.present_packet(&packet(
            draws.clone(),
            FrameFeatureSet::new(false, false, 0, 0),
        ));
        assert_eq!(r0.raster().framebuffer_width, 160);
        assert_eq!(r0.raster().framebuffer_height, 120);
        // Level 2 → Medium, 320×240 (more candidate pixels than UltraLow).
        backend.set_quality_level(2);
        let r2 = backend.present_packet(&packet(draws, FrameFeatureSet::new(false, false, 0, 0)));
        assert_eq!(r2.raster().framebuffer_width, 320);
        assert_eq!(r2.raster().framebuffer_height, 240);
        assert!(r2.raster().candidate_pixels > r0.raster().candidate_pixels);
    }


    #[test]
    fn unknown_mesh_is_skipped_without_critical_violation() {
        use axiom_host::{FrameDrawItem, FrameFeatureSet};
        let backend = Canvas2dBackendApi::new(&request(640, 480));
        let draws = vec![FrameDrawItem::new(
            1, 404, 9, IDENTITY, IDENTITY, [1.0; 4], false,
        )];
        let report =
            backend.present_packet(&packet(draws, FrameFeatureSet::new(false, false, 0, 0)));
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
        let draws = vec![FrameDrawItem::new(
            1, 7, 13, IDENTITY, IDENTITY, [1.0; 4], false,
        )];
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
        let draws = vec![FrameDrawItem::new(
            1, 7, 9, IDENTITY, IDENTITY, [1.0; 4], false,
        )];
        let p = packet(draws, FrameFeatureSet::new(false, false, 0, 0));
        assert_eq!(backend.present_packet(&p).submitted_draws(), 1);

        let (_, v, i) = ground(7);
        backend.replace_geometry(7, &v, &i);
        assert_eq!(backend.present_packet(&p).submitted_draws(), 1);
    }

    #[test]
    fn renders_a_draw2d_list_with_a_sprite_at_the_canvas_size() {
        use axiom_host::{Common2d, Draw2dCommand, SpriteDraw2d, TextureId};
        use axiom_math::Vec2;

        let mut backend = Canvas2dBackendApi::new(&request(4, 4));
        // A 1×1 opaque red texture, blitted as a 1×1 sprite at the origin.
        backend.load_textures(&[(3, 1, 1, vec![255, 0, 0, 255])]);
        let one = Ratio::new(1.0).expect("finite");
        let opts = SpriteDraw2d::new(
            axiom_host::Rect::new(Vec2::ZERO, Vec2::ONE),
            Vec2::ZERO,
            axiom_host::Rgba::new(one, one, one, one),
            false,
            false,
        );
        let mut list = Draw2dList::default();
        list.push_command(Draw2dCommand::sprite(
            (0, axiom_math::Mat3::IDENTITY, Common2d::new(0, one)),
            TextureId::from_raw(3),
            opts,
        ));
        list.sort_commands();

        let (rgba, w, h) = backend.render_draw2d_rgba(&list);
        assert_eq!((w, h), (4, 4));
        assert_eq!(rgba.len() as u32, w * h * 4);
        // Pixel (0,0) is the opaque red sprite; an untouched pixel is transparent.
        assert_eq!(&rgba[0..4], &[255, 0, 0, 255]);
        let untouched = ((3 * 4 + 3) * 4) as usize;
        assert_eq!(&rgba[untouched..untouched + 4], &[0, 0, 0, 0]);
    }
}
