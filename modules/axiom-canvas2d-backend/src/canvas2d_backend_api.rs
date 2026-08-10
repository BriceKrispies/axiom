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
// The per-frame console telemetry sinks and their clock (no-ops on native).
use crate::frame_telemetry::{deep_log, log_phases, log_timing, now_ms};

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
    ///
    /// The canvas's on-screen *size* is deliberately not passed and not set:
    /// the CSS box is the page's, the backing store is the backend's. See
    /// [`crate::live_canvas_binding::LiveCanvasBinding::attach`].
    #[cfg(target_arch = "wasm32")]
    pub fn attach_canvas(
        &mut self,
        canvas: &web_sys::HtmlCanvasElement,
    ) -> Result<(), wasm_bindgen::JsValue> {
        self.binding = Some(crate::live_canvas_binding::LiveCanvasBinding::attach(
            canvas,
            self.options.framebuffer_width(),
            self.options.framebuffer_height(),
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
    /// and hemisphere ambient come from the frame, leaving every other knob as
    /// configured) and run the pure software z-buffer rasterizer.
    fn rasterize(&self, packet: &FramePacket, skinned: &[SkinnedDraw]) -> SoftwareRasterResult {
        // Only one visual profile exists; this avoids an unused-field warning.
        let _ = self.profile;
        let mut cues = self.options.depth_cues();
        // The frame's authored atmospheric fog (`axiom_host::FrameDepthFog`) is the
        // one definition of aerial perspective, and the GPU backend now reads the
        // same numbers — so an authored fog looks the same on both. A frame that
        // authors none keeps this backend's historical behaviour exactly: the
        // profile's own gentle range, receding toward the frame's clear colour.
        let clear = packet.clear_color();
        let authored = packet.depth_fog();
        cues.fog.near = authored.map_or(cues.fog.near, |fog| fog.near().get());
        cues.fog.far = authored.map_or(cues.fog.far, |fog| fog.far().get());
        cues.fog.strength = authored.map_or(cues.fog.strength, |fog| fog.strength().get());
        cues.fog.color = authored.map_or(clear, |fog| {
            let rgb = fog.color();
            [rgb[0], rgb[1], rgb[2], clear[3]]
        });
        // The frame's hemisphere ambient drives the software lighting too, matching the
        // GPU path's ambient uniform. Colours are strength-folded, so the ambient scale
        // is 1.0; an absent frame ambient falls back to the engine default hemisphere.
        let amb = packet
            .ambient()
            .copied()
            .unwrap_or_else(axiom_host::FrameAmbient::default_hemisphere);
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
        // The three shader-and-render-target capabilities the software path has no
        // answer for. Each is read from what the frame actually carries rather than
        // from a separately-authored flag, so the report cannot claim a drop for a
        // frame that asked for nothing — or stay silent about one that did.
        //
        // This is what "skip it for the Canvas 2D version" means here: not an
        // app-level `if backend == canvas2d`, but the frame carrying its full intent
        // and this backend declaring, per frame, the three parts of it that it did
        // not honour.
        let sky_degraded = packet.sky().is_some() & !profile.contains(RenderCapability::Sky);
        let bloom_degraded = packet.bloom().is_some() & !profile.contains(RenderCapability::Bloom);
        let specular_degraded =
            packet.uses_specular() & !profile.contains(RenderCapability::Specular);
        // The second *substitute*, alongside the directional shadow: a frame that
        // authored a per-metre extinction rate gets the normalized-depth ramp of
        // the same fog here, because this backend's fog is a post-pass holding a
        // z-buffer and no world position. Keyed on the frame having authored a
        // non-zero rate, so a fog with only a depth window reports nothing —
        // there is nothing it did not honour.
        let aerial_degraded = packet
            .depth_fog()
            .is_some_and(|fog| fog.extinction().get() != 0.0)
            & !profile.contains(RenderCapability::AerialPerspective);
        let degraded_features: Vec<FrameFeature> = [
            textures_degraded.then_some(FrameFeature::AlbedoSampling),
            shadows_degraded.then_some(FrameFeature::Shadows),
            sky_degraded.then_some(FrameFeature::Sky),
            specular_degraded.then_some(FrameFeature::SpecularHighlight),
            bloom_degraded.then_some(FrameFeature::Bloom),
            aerial_degraded.then_some(FrameFeature::AerialPerspective),
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

#[cfg(test)]
mod tests;
