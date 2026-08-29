//! The single GPU-backend facade: own the real wgpu binding and present frames.
//!
//! Split in two by *when* a method runs. Everything here is bind-time or
//! frame-time — construct the backend, initialise the live binding, present a
//! packet. The **preparation barrier** — where every surface program is compiled,
//! and the preparation-time queries a caller builds a frame's degraded-feature
//! report from — lives in [`surfaces`], because a shader compile has no business
//! sitting in the same file as a per-frame present.
//!
//! A third file, [`timing`], holds the queries that answer *what the frame
//! cost and what it ran on* — per-pass GPU time, the bound graphics API, and the
//! batching a packet lowers to. They are neither bind-time nor frame-time work:
//! they read back facts the other two produced.

mod surfaces;
mod timing;

use axiom_host::{Draw2dList, FramePacket, HostPresentationRequest, SdfScene};

/// The column-major identity, used where a frame carries no camera and a
/// matrix is nonetheless required by the call shape.
const IDENTITY_MATRIX: [f32; 16] = [
    1.0, 0.0, 0.0, 0.0, //
    0.0, 1.0, 0.0, 0.0, //
    0.0, 0.0, 1.0, 0.0, //
    0.0, 0.0, 0.0, 1.0, //
];

/// The real GPU presentation backend for one surface.
///
/// Constructed from a validated [`HostPresentationRequest`], from which it reads
/// the physical surface size. On `wasm32` it binds a real `wgpu` surface/device
/// and presents instanced draws; on native there is no GPU, so it holds only the
/// size and every present is a no-op.
#[derive(Debug)]
pub struct GpuBackendApi {
    width: u32,
    height: u32,
    // Render-target size the device tier renders at before the frame is resolved
    // onto the surface on present (`HostDeviceProfile::render_size`): smaller
    // than the surface on a high-DPR phone the tier caps, LARGER on a
    // supersampling tier, where the extra samples are box-filtered back down.
    render_width: u32,
    render_height: u32,
    // Shadow-atlas edge length from the device tier
    // (`HostDeviceProfile::shadow_map_size`), handed to the renderer on initialise.
    shadow_size: u32,
    // The most anisotropy the device tier will let a material sampler ask for
    // (`HostDeviceProfile::max_anisotropy`). The live binding takes the SMALLER of
    // this and what the adapter reports it can do: capability says what the device
    // may do, the tier says what it should be asked to afford. Without the tier
    // half, the WebGPU arm — whose downlevel flags wgpu fills in from an assumption
    // of compliance rather than from the hardware — asks every phone for 16 taps a
    // pixel on the surfaces that opt in.
    max_anisotropy: u16,
    // Which optional render capabilities this backend attempts. Defaults to every
    // capability this crate's code can execute, minus `HdrTargets` — the one bit
    // only a bound adapter can grant (`crate::hdr_target`). A host may restrict it
    // further (an fps/legibility lever) and the per-frame present consults it.
    capability: axiom_host::BackendCapabilityProfile,
    // CPU sprite/atlas textures the 2D Draw2dList sprite path samples, as
    // `(texture_id, width, height, RGBA8)` — same upload shape as the 3D
    // material set.
    draw2d_textures: Vec<(u64, u32, u32, Vec<u8>)>,
    // What the PREPARATION BARRIER compiled: every authored surface this backend
    // was handed, in sorted digest order, with the program each one lowers to.
    // Filled once by `prepare_surfaces` and read (never written) by every frame.
    // A frame naming a program that is not in here is a reported degradation,
    // never a compile — see `Self::frame_degradations`.
    catalog: crate::surface_program::cache::SurfaceProgramCatalog,
    // The **per-frame view** of the same surfaces, planned once by
    // `prepare_surfaces` alongside the catalog. `None` until the barrier has run.
    //
    // It lives here for exactly the reason the catalog does: building it
    // linearises every layer tree and composes every channel graph
    // (`axiom_surface::Surface::flatten`), and that is preparation work, not
    // frame work. It was rebuilt on every present until 2026-08-17, which a
    // throttled browser profile measured at 5.4 ms of an 8.1 ms frame.
    surface_set: Option<crate::surface_program::SurfaceProgramSet>,
    // Present only once initialised on wasm32; its absence means "not ready".
    #[cfg(target_arch = "wasm32")]
    live: Option<crate::live_gpu_binding::LiveGpuBinding>,
}

impl GpuBackendApi {
    /// A fresh backend sized from the configured presentation request. No browser
    /// or GPU object is touched — the surface size is read from host-owned data,
    /// so this runs and is tested on native exactly as on the web.
    pub fn new(request: &HostPresentationRequest) -> Self {
        let viewport = request.descriptor().viewport();
        let width = viewport.physical_width();
        let height = viewport.physical_height();
        let (render_width, render_height) = request.device().profile().render_size(width, height);
        GpuBackendApi {
            width,
            height,
            render_width,
            render_height,
            shadow_size: request.device().profile().shadow_map_size(),
            max_anisotropy: request.device().profile().max_anisotropy(),
            // **Everything this crate's code can do, minus the one bit only a
            // device can grant.** Procedural surfaces are included: that bit was
            // cleared for as long as this backend could generate a program but
            // not bind one, and it can bind one now — a prepared surface's
            // program is a real pipeline with a real parameter buffer behind it
            // (`crate::surface_program::compile`).
            //
            // `HdrTargets` is different in kind. Every other capability here is a
            // property of the source: the shaders, the extra targets and the
            // evaluators either exist or they do not. Whether a colour attachment
            // can hold a value above one is a property of the *adapter*, and a
            // backend that has bound no device has resolved no answer — so it
            // claims none, and `initialize` grants the bit only when the adapter
            // actually reported the format usable (`crate::hdr_target`). The
            // native off-screen capture path never grants it, correctly: its
            // target is an `Rgba8UnormSrgb` texture by construction.
            //
            // The capability word the main-pass WGSL reads is unaffected either
            // way — that shader reads no bit above 2048.
            capability: crate::hdr_target::unresolved_capability_profile(),
            draw2d_textures: Vec::new(),
            catalog: crate::surface_program::cache::SurfaceProgramCatalog::default(),
            surface_set: None,
            #[cfg(target_arch = "wasm32")]
            live: None,
        }
    }


    /// Upload the CPU sprite/atlas textures the 2D [`Draw2dList`] sprite path
    /// samples, as `(texture_id, width, height, RGBA8 pixels)` — the same upload
    /// shape as the 3D material set and the Canvas 2D backend's `load_textures`.
    /// On native these supply the covered core's sprite UV sizes; the live arm
    /// uploads them as GPU textures.
    pub fn load_draw2d_textures(&mut self, textures: &[(u64, u32, u32, Vec<u8>)]) {
        self.draw2d_textures = textures.to_vec();
        #[cfg(target_arch = "wasm32")]
        self.live
            .iter_mut()
            .for_each(|live| live.set_draw2d_textures(&self.draw2d_textures));
    }

    /// Render the 3D scene at `scale` of the device tier's render size — the
    /// live arm of [`axiom_host::RenderScaleController`].
    ///
    /// The tier ([`axiom_host::HostDeviceProfile`]) decides the resolution the
    /// frame would like; this decides what the device can afford, which is not a
    /// question an app can answer at authoring time. Fragment cost is very nearly
    /// linear in pixels, so it is the one quality dial that trades smoothly
    /// against frame time rather than falling off a cliff.
    ///
    /// A no-op before the surface binds and on a scale that resolves to the size
    /// already in use, so a caller may hand it the same value every frame.
    #[cfg_attr(not(target_arch = "wasm32"), allow(unused_variables))]
    pub fn set_render_scale(&mut self, scale: axiom_host::RenderScale) {
        #[cfg(target_arch = "wasm32")]
        self.live
            .iter_mut()
            .for_each(|live| live.set_render_scale(scale));
    }

    /// The physical surface width the backend will bind.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// The physical surface height the backend will bind.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// The shadow-atlas edge length the backend's device tier selected. This is
    /// the tier's [`axiom_host::HostDeviceProfile::shadow_map_size`], carried
    /// from the presentation request to the renderer at initialise time.
    pub fn shadow_size(&self) -> u32 {
        self.shadow_size
    }

    /// The anisotropy ceiling the backend's device tier selected — the tier's
    /// [`axiom_host::HostDeviceProfile::max_anisotropy`], carried from the
    /// presentation request to the sampler resolution at initialise time.
    ///
    /// This is a *budget*, not a capability: the live binding resolves the actual
    /// clamp as the smaller of this and what the adapter reports, so a device that
    /// cannot filter anisotropically still gets `1` and a device that can is still
    /// held to what its tier can afford.
    pub fn max_anisotropy(&self) -> u16 {
        self.max_anisotropy
    }

    /// Restrict which optional render capabilities this backend attempts. The
    /// default is every capability this crate's code can execute, minus
    /// [`axiom_host::RenderCapability::HdrTargets`], which only a bound device can
    /// grant; a host may narrow it and the per-frame present
    /// ([`Self::present_frame`] / [`Self::present_packet`]) consults it, so the live GPU
    /// is no longer unconditionally full — it gates on the same profile the Canvas 2D
    /// backend does.
    pub fn set_capability_profile(&mut self, profile: axiom_host::BackendCapabilityProfile) {
        self.capability = profile;
    }

    /// `?nocaps=` on wasm; the identity everywhere else.
    #[cfg(target_arch = "wasm32")]
    fn bisected(
        profile: axiom_host::BackendCapabilityProfile,
    ) -> axiom_host::BackendCapabilityProfile {
        crate::live_gpu_binding::dropped_by_url(profile)
    }

    /// Native builds have no URL to read, so the profile is whatever the host set.
    #[cfg(not(target_arch = "wasm32"))]
    fn bisected(
        profile: axiom_host::BackendCapabilityProfile,
    ) -> axiom_host::BackendCapabilityProfile {
        profile
    }

    /// The optional render capabilities this backend attempts.
    ///
    /// Before a bind this is [`axiom_host::BackendCapabilityProfile::all`] with
    /// [`axiom_host::RenderCapability::HdrTargets`] cleared; after one it also
    /// carries whatever the adapter reported about half-float colour targets,
    /// which is the one entry in the set that is a fact about the device rather
    /// than about this source.
    pub fn capability_profile(&self) -> axiom_host::BackendCapabilityProfile {
        self.capability
    }

    /// The render-target width the device tier renders the scene at before the
    /// frame is resolved onto the swapchain — the tier's
    /// [`axiom_host::HostDeviceProfile::render_size`] applied to the surface. It
    /// may be below the surface (a capped tier) or above it (a supersampling
    /// tier); the present filter handles both directions.
    pub fn render_width(&self) -> u32 {
        self.render_width
    }

    /// The render-target height the device tier renders the scene at before the
    /// frame is resolved onto the swapchain.
    pub fn render_height(&self) -> u32 {
        self.render_height
    }

    /// Whether a live GPU binding is initialised and could present real pixels.
    /// Always `false` on native (there is no GPU); on wasm32, `true` once
    /// [`Self::initialize`] has succeeded.
    pub fn binding_is_ready(&self) -> bool {
        #[cfg(target_arch = "wasm32")]
        {
            return self.live.is_some();
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            false
        }
    }

    /// Present one frame from per-`(mesh, material)` instance batches: each batch
    /// is `(mesh_id, material_id, instance floats [mvp(16)+colour(4) per
    /// instance], count)`, referencing a mesh and a material uploaded at
    /// [`Self::initialize`]. The material selects the albedo texture/sampler bind
    /// group. Returns whether real pixels were drawn — always `false` on native
    /// (headless), and on wasm32 `true` when a live binding rendered the frame.
    pub fn present_frame(
        &self,
        clear_color: [f32; 4],
        lights: &[(u32, [f32; 3], [f32; 3], f32)],
        light_view_proj: [f32; 16],
        // The frame's camera — view, projection and their product, as
        // `axiom_host` publishes them. All three travel because a product cannot
        // be split into its factors: the sky pass inverts the view-projection,
        // the depth/normal prepass works in view space, and the ambient occlusion
        // built on it inverts the projection. `FrameCamera::default()` on a frame
        // that reads none of them.
        camera: axiom_host::FrameCamera,
        batches: &[(u64, u64, Vec<f32>, u32)],
        sdf: Option<&SdfScene>,
    ) -> bool {
        // A caller that names no surface set has no time-reading surface either,
        // so its surface-time lane is an exact zero and its packed lighting
        // uniform is byte-identical to what it was before there was a lane.
        self.present_frame_at(
            clear_color,
            lights,
            light_view_proj,
            camera,
            batches,
            // No batch names a surface program, so every one of them draws the
            // default pipeline — this entry's whole behaviour, unchanged.
            &[],
            sdf,
            0.0,
        )
    }

    /// [`Self::present_frame`] with the frame's **surface time** — the seconds a
    /// time-varying authored surface samples, in both the vertex and the
    /// fragment stage.
    ///
    /// It is a separate entry rather than a parameter on `present_frame` because
    /// the number is not the caller's to invent: it is
    /// `axiom_surface`-gated engine time, decided by
    /// [`crate::surface_program::SurfaceProgramSet::surface_time`] from what the
    /// packet supplied and from whether anything in the frame actually reads a
    /// clock. `present_frame` therefore passes zero, which is what it always
    /// effectively wrote.
    fn present_frame_at(
        &self,
        clear_color: [f32; 4],
        lights: &[(u32, [f32; 3], [f32; 3], f32)],
        light_view_proj: [f32; 16],
        // The frame's camera — view, projection and their product, as
        // `axiom_host` publishes them. All three travel because a product cannot
        // be split into its factors: the sky pass inverts the view-projection,
        // the depth/normal prepass works in view space, and the ambient occlusion
        // built on it inverts the projection. `FrameCamera::default()` on a frame
        // that reads none of them.
        camera: axiom_host::FrameCamera,
        batches: &[(u64, u64, Vec<f32>, u32)],
        // The surface program each batch draws with, in `batches` order. Empty
        // for every caller that names no surface, which draws them all with the
        // default pipeline.
        programs: &[u64],
        sdf: Option<&SdfScene>,
        surface_time: f32,
    ) -> bool {
        #[cfg(target_arch = "wasm32")]
        {
            return self
                .live
                .as_ref()
                .map(|live| {
                    live.render_frame(
                        lights,
                        light_view_proj,
                        batches,
                        programs,
                        &[],
                        clear_color,
                        sdf,
                        self.capability.bits(),
                        camera,
                        surface_time,
                    )
                    .is_ok()
                })
                .unwrap_or(false);
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = (
                clear_color,
                lights,
                light_view_proj,
                camera,
                batches,
                programs,
                sdf,
                surface_time,
            );
            false
        }
    }

    /// Like [`Self::present_frame`] but **surfaces a device-loss error** instead
    /// of flattening it to a bool, so the run loop can rebuild the binding when
    /// the GPU surface is unrecoverably lost — a backgrounded mobile tab whose
    /// drawing context the browser destroyed. `Ok(())` when the frame presented,
    /// was cleanly skipped for a transient surface hiccup the binding already
    /// reconfigured around, or there is no live binding; `Err` only on an
    /// unrecoverable loss. wasm32 only.
    ///
    /// **It carries the frame's surfaces, exactly as [`Self::present_packet_with_surfaces`]
    /// does.** `programs` is the surface program each batch draws with, in
    /// `batches` order — a batch is one `(mesh, material)` pair and a material
    /// names at most one surface, so the program is a per-batch value, not a
    /// per-instance one. An empty slice draws every batch with the default
    /// pipeline, which is byte-identical to what this entry did before the lane
    /// existed. A program the preparation barrier ([`Self::prepare_surfaces`])
    /// did not prepare renders the constant fallback and is reported through
    /// [`Self::program_degradations`]; nothing is compiled here.
    ///
    /// `time` is the frame's **presentation time** — explicitly supplied engine
    /// time, never a wall clock. It is gated through the prepared surface set
    /// exactly as the packet path gates it, so a backend that prepared nothing,
    /// or whose surfaces read no clock, writes an exact zero into the
    /// surface-time lane and its packed lighting uniform is unchanged.
    #[cfg(target_arch = "wasm32")]
    #[allow(clippy::too_many_arguments)]
    pub fn present_frame_result(
        &self,
        clear_color: [f32; 4],
        lights: &[(u32, [f32; 3], [f32; 3], f32)],
        light_view_proj: [f32; 16],
        // The frame's camera — view, projection and their product, as
        // `axiom_host` publishes them. All three travel because a product cannot
        // be split into its factors: the sky pass inverts the view-projection,
        // the depth/normal prepass works in view space, and the ambient occlusion
        // built on it inverts the projection. `FrameCamera::default()` on a frame
        // that reads none of them.
        camera: axiom_host::FrameCamera,
        batches: &[(u64, u64, Vec<f32>, u32)],
        programs: &[u64],
        skinned_draws: &[(u64, u64, [f32; 16], [f32; 16], [f32; 4], Vec<[f32; 16]>)],
        sdf: Option<&SdfScene>,
        time: axiom_kernel::Seconds,
    ) -> Result<(), wasm_bindgen::JsValue> {
        let skinned: Vec<crate::scene_renderer::SkinnedGpuDraw> = skinned_draws
            .iter()
            .map(|(mesh_id, material_id, mvp, world, color, palette)| {
                crate::scene_renderer::SkinnedGpuDraw {
                    mesh_id: *mesh_id,
                    material_id: *material_id,
                    mvp: *mvp,
                    world: *world,
                    color: *color,
                    palette: palette.clone(),
                }
            })
            .collect();
        // The same rule the packet path applies: the barrier's own set decides
        // whether the frame carries a clock at all. The skinned vertex stage
        // still runs no displacement program (the 16-attribute ceiling), which
        // `Self::skinned_surface_degradations` reports.
        let surface_time = self.frame_surface_set(&[]).surface_time(time);
        self.live
            .as_ref()
            .map(|live| {
                live.render_frame(
                    lights,
                    light_view_proj,
                    batches,
                    programs,
                    &skinned,
                    clear_color,
                    sdf,
                    self.capability.bits(),
                    camera,
                    surface_time,
                )
            })
            .unwrap_or(Ok(()))
    }

    /// Present one frame from the backend-neutral [`axiom_host::FramePacket`] —
    /// the single artifact this backend and the future Canvas 2D backend both
    /// consume. It derives the live path's instance batches + lights from the
    /// packet (see [`crate::frame_packet_adapter`]) and presents them through the
    /// exact same path as [`Self::present_frame`], so behaviour is unchanged.
    /// Returns whether real pixels were drawn — always `false` on native.
    pub fn present_packet(&self, packet: &FramePacket) -> bool {
        self.present_packet_with_surfaces(packet, &[])
    }

    /// Like [`Self::present_packet`], but with the frame's authored
    /// [`axiom_surface::Surface`] set.
    ///
    /// A draw whose surface was **prepared** at the barrier
    /// ([`Self::prepare_surfaces`]) is drawn with that surface's own compiled
    /// program. A draw whose surface was not — or whose surface needs no program,
    /// because every channel of it is a plain constant — renders its **constant**
    /// channels through the instance lanes the stream already has: a constant
    /// base colour multiplies the instance colour and a constant emission adds to
    /// the instance emissive. Nothing is compiled here, and nothing is
    /// *flattened* here either (see `Self::frame_surface_set`); a miss is
    /// reported through [`Self::frame_degradations`].
    ///
    /// Passing an empty slice is what [`Self::present_packet`] does, and on a
    /// backend that never prepared it makes every packed byte identical to the
    /// pre-surface stream.
    pub fn present_packet_with_surfaces(
        &self,
        packet: &FramePacket,
        surfaces: &[axiom_surface::Surface],
    ) -> bool {
        let set = self.frame_surface_set(surfaces);
        let (batches, programs) =
            crate::frame_packet_adapter::frame_packet_to_batches(packet, &set);
        let lights = crate::frame_packet_adapter::frame_packet_lights(packet);
        self.present_frame_at(
            packet.clear_color(),
            &lights,
            packet.light_view_proj(),
            // The packet's camera, whole. It used to be narrowed to its
            // view-projection here, on the reasoning that only the sky pass read
            // it — which stopped being true when the depth/normal prepass and the
            // ambient occlusion above it started needing the view and the
            // projection separately, and neither is recoverable from the product.
            // A packet carrying no camera gets the default, whose matrices are
            // identity; the passes that would read it are not built for such a
            // frame anyway.
            packet.camera().unwrap_or(axiom_host::FrameCamera::IDENTITY),
            &batches,
            &programs,
            packet.sdf(),
            // The frame's surface time: the packet's own supplied engine time
            // when something in the set reads a clock, and an exact zero when
            // nothing does.
            set.surface_time(packet.time()),
        )
    }


    /// Present a host-neutral [`Draw2dList`] through the GPU backend — the 2D
    /// peer of [`Self::present_packet`]. It walks the layer-sorted list into
    /// backend-neutral quad geometry via the **covered core**
    /// ([`crate::draw2d_geometry`]) and draws it alpha-blended (honouring layer
    /// order) to the swap-chain through a non-sRGB view, so the live frame matches
    /// the software Canvas 2D backend byte-for-byte. `clear` is the background
    /// colour. Returns whether real pixels were drawn — always `false` on native
    /// (headless: the geometry is built and discarded, exactly as
    /// [`Self::present_packet`] no-ops after building its batches), and on wasm32
    /// `true` when a live binding drew the frame.
    pub fn present_draw2d(&self, list: &Draw2dList, clear: [f32; 4]) -> bool {
        #[cfg(target_arch = "wasm32")]
        {
            return self
                .live
                .as_ref()
                .map(|live| {
                    live.render_draw2d(list, &self.draw2d_textures, clear)
                        .is_ok()
                })
                .unwrap_or(false);
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = clear;
            let sizes =
                crate::draw2d_geometry::Draw2dTextureSizes::from_textures(&self.draw2d_textures);
            let geometry =
                crate::draw2d_geometry::build_geometry(list, self.width, self.height, &sizes);
            let _ = (
                geometry.quad_count(),
                geometry.vertices().len(),
                geometry.sources().len(),
            );
            false
        }
    }

    /// Rasterize a host-neutral [`Draw2dList`] **off-screen** to `width * height *
    /// 4` linear RGBA8 bytes (row-major, top-left origin), headless on native —
    /// the 2D peer of [`Self::render_offscreen_rgba`] and the screenshot path for
    /// 2D surfaces. It builds the geometry through the covered core
    /// ([`crate::draw2d_geometry`]) and draws it alpha-blended through the shared
    /// [`crate::draw2d_renderer`] into a **linear** (non-sRGB) target, so the
    /// pixels match the software Canvas 2D backend byte-for-byte (within ±1
    /// rounding). `textures` are the sprite atlases sampled by sprite commands.
    /// `None` if no native GPU adapter is available. Compiled only behind the
    /// `offscreen` feature, so it never enters the default build or gates.
    #[cfg(all(not(target_arch = "wasm32"), feature = "offscreen"))]
    pub fn render_draw2d_offscreen_rgba(
        width: u32,
        height: u32,
        list: &Draw2dList,
        textures: &[(u64, u32, u32, Vec<u8>)],
    ) -> Option<Vec<u8>> {
        let sizes = crate::draw2d_geometry::Draw2dTextureSizes::from_textures(textures);
        let geometry = crate::draw2d_geometry::build_geometry(list, width, height, &sizes);
        // Upload the app's sprite atlases plus the baked gradient ramp textures the
        // gradient-filled quads bind (the covered core registers them on the
        // geometry; the platform arm uploads them like any other texture).
        let all_textures: Vec<(u64, u32, u32, Vec<u8>)> = textures
            .iter()
            .cloned()
            .chain(geometry.gradient_textures())
            .collect();
        crate::draw2d_offscreen::render_draw2d_to_rgba(width, height, &geometry, &all_textures)
    }

    /// Bake one procedural surface **on the device** into its albedo, ORM and
    /// tangent-space normal maps.
    ///
    /// `library_wgsl` is the caller's whole shader library — its noise helpers
    /// and one `ow_surface_<name>` entry per surface; `request` names which one
    /// to run and at what size, plus the per-surface parameters. The backend
    /// splices the two, renders the surface to a full-screen quad, derives the
    /// normal from the height channel with a Sobel pass, and reads all three
    /// maps back.
    ///
    /// This exists because the CPU reference is the *semantic* definition, not
    /// the shipping path: evaluating a surface costs ~15.5 us a texel, so a
    /// nineteen-surface library at 1024^2 is minutes of CPU and milliseconds of
    /// GPU. The two must agree, which is what the caller's parity test measures.
    ///
    /// `None` when the machine has no adapter — the same contract
    /// [`Self::render_offscreen_rgba`] has, and for the same reason.
    #[cfg(all(not(target_arch = "wasm32"), feature = "offscreen"))]
    pub fn bake_procedural_texture(
        library_wgsl: &str,
        request: &axiom_host::ProceduralBakeRequest,
    ) -> Option<axiom_host::ProceduralBakeMaps> {
        crate::texture_bake::bake_offscreen(library_wgsl, request)
    }

    /// Render one frame **off-screen** to `width * height * 4` RGBA8 bytes,
    /// headless, on native — the screenshot path. It builds a throwaway GPU device
    /// and draws `meshes` / `materials` / `lights` / `batches` (the same data
    /// [`Self::present_frame`] takes, plus the mesh/material sets from
    /// [`Self::initialize`]) through the **same** [`crate::scene_renderer`] the
    /// browser arm uses, then reads the pixels back. `None` if no native GPU
    /// adapter is available. Compiled only behind the `offscreen` feature, so it
    /// never enters the engine's default build or gates.
    #[cfg(all(not(target_arch = "wasm32"), feature = "offscreen"))]
    #[allow(clippy::too_many_arguments)]
    pub fn render_offscreen_rgba(
        width: u32,
        height: u32,
        meshes: &[(u64, Vec<f32>, Vec<u32>)],
        // `normals` is gone: the four extra maps ride on `MaterialTexture`
        // itself now, so a caller cannot supply a normal map for a material it
        // did not also supply. The parallel slice made that mismatch expressible
        // — and every caller in the repo but one passed `&[]`, which is why the
        // live browser arm had no normal-map lane at all.
        materials: &[axiom_host::MaterialTexture],
        lights: &[(u32, [f32; 3], [f32; 3], f32)],
        light_view_proj: [f32; 16],
        // The frame's camera — view, projection and their product, as
        // `axiom_host` publishes them. All three travel because a product cannot
        // be split into its factors: the sky pass inverts the view-projection,
        // the depth/normal prepass works in view space, and the ambient occlusion
        // built on it inverts the projection. `FrameCamera::default()` on a frame
        // that reads none of them.
        camera: axiom_host::FrameCamera,
        batches: &[(u64, u64, Vec<f32>, u32)],
        skinned_mesh_set: &[(u64, Vec<f32>, Vec<u32>)],
        skinned_draws: &[(u64, u64, [f32; 16], [f32; 16], [f32; 4], Vec<[f32; 16]>)],
        clear: [f32; 4],
        sdf: Option<&SdfScene>,
        look: axiom_host::FrameRenderLook,
        retro_32bit: Option<axiom_host::FrameRetro32BitProfile>,
        profile: axiom_host::BackendCapabilityProfile,
        volumetrics: Option<axiom_host::FrameVolumetrics>,
        postprocess: Option<axiom_host::FramePostProcess>,
        // How many times to record the scene before reading it back. `1` for
        // every caller that wants a picture; higher only for a caller that wants
        // to *measure* one, which must difference two runs to cancel the device
        // setup this path pays on every call.
        repeat: u32,
    ) -> Option<Vec<u8>> {
        let skinned: Vec<crate::scene_renderer::SkinnedGpuDraw> = skinned_draws
            .iter()
            .map(|(mesh_id, material_id, mvp, world, color, palette)| {
                crate::scene_renderer::SkinnedGpuDraw {
                    mesh_id: *mesh_id,
                    material_id: *material_id,
                    mvp: *mvp,
                    world: *world,
                    color: *color,
                    palette: palette.clone(),
                }
            })
            .collect();
        crate::offscreen::render_to_rgba(
            width,
            height,
            meshes,
            materials,
            lights,
            light_view_proj,
            camera,
            batches,
            skinned_mesh_set,
            &skinned,
            clear,
            sdf,
            look,
            retro_32bit,
            profile,
            volumetrics,
            postprocess,
            repeat,
        )
        // This entry is the **screenshot**: its contract is bytes, and its
        // callers (`axiom-shot`, the parity proofs) compare them. The frame's
        // per-pass GPU timings come back from the same call and are the native
        // proof that the resolve path works — asserted on directly in this
        // crate's `offscreen_timing` test rather than widened into a second
        // twenty-argument public entry nobody asked for. The *live* arm, which
        // is where a 30 fps frame actually needs explaining, publishes them
        // through `Self::gpu_pass_timing`.
        .map(|(pixels, _timing)| pixels)
    }

    /// Initialise the real wgpu binding from a canvas, the engine's distinct mesh
    /// set (`(mesh_id, interleaved position+normal+uv+colour vertices [12
    /// floats/vertex], triangle indices)`), and the material set
    /// (`(material_id, width, height, RGBA8 albedo pixels)`) — one bind group
    /// (texture + sampler) is built per material. wasm32 only; on success later
    /// [`Self::present_frame`] calls draw real pixels. On failure the binding
    /// stays absent (not ready).
    ///
    /// `preference` forces which graphics API is bound (see
    /// [`crate::live_gpu_binding::LiveGpuBinding::initialize`]): `None` auto-probes
    /// WebGPU→WebGL2; `Some(BackendKind::GpuPrimary)` binds WebGPU only (erroring if
    /// absent); `Some(BackendKind::GpuFallback)` binds WebGL2 only. This is what
    /// lets a caller render the same scene through each backend side by side.
    ///
    /// `skinned_meshes` are the bake-once skinned meshes (the 20-float
    /// pos/norm/uv/col/joints/weights vertex stream) uploaded through the skinning
    /// pipeline, distinct from the ordinary `meshes` — empty for apps that submit
    /// no skinned bodies. The per-frame joint palettes ride in on
    /// [`Self::present_frame_result`]'s `skinned_draws`.
    #[cfg(target_arch = "wasm32")]
    #[allow(clippy::too_many_arguments)]
    pub async fn initialize(
        &mut self,
        canvas: web_sys::HtmlCanvasElement,
        meshes: &[(u64, Vec<f32>, Vec<u32>)],
        skinned_meshes: &[(u64, Vec<f32>, Vec<u32>)],
        materials: &[axiom_host::MaterialTexture],
        max_instances: u32,
        look: axiom_host::FrameRenderLook,
        preference: Option<axiom_host::BackendKind>,
    ) -> Result<(), wasm_bindgen::JsValue> {
        let binding = crate::live_gpu_binding::LiveGpuBinding::initialize(
            canvas,
            self.width,
            self.height,
            self.render_width,
            self.render_height,
            meshes,
            skinned_meshes,
            materials,
            max_instances,
            self.shadow_size,
            self.max_anisotropy,
            look,
            // The profile as this backend holds it *before* the bind — the host's
            // own restrictions. The binding needs it to decide the scene target's
            // format (a declined `HdrTargets` must not get a float intermediate),
            // and it applies the same grant below to reach the same answer.
            self.capability,
            preference,
        )
        .await?;
        // **What the device actually resolved**, folded into the profile the
        // per-frame present consults. A bind is the first moment this backend can
        // answer whether it has HDR colour targets, so it is the moment the bit
        // is granted — and it is only ever *granted*: a host that narrowed the
        // profile keeps every restriction it set, because the device can add a
        // capability it has and can never take back one a host declined.
        self.capability =
            crate::hdr_target::grant_hdr_targets(self.capability, binding.has_hdr_targets());
        // Granted from the same bind, immediately after HDR, because the
        // G-buffer's own gate requires HDR: granting them apart would let a
        // device report a G-buffer it cannot allocate.
        self.capability =
            crate::gbuffer::grant_gbuffer(self.capability, binding.has_gbuffer());
        // The page's capability bisect (`?nocaps=`), applied LAST.
        //
        // It has to be last, and that is the whole lesson of this hook: the two
        // earlier homes for it both looked right and both did nothing. Masking
        // the bind's local profile touches only the HDR-attachment decision;
        // masking `set_capability_profile` touches a setter no caller in this
        // repo invokes, so the profile a frame actually consults -- this field,
        // as the grants above leave it -- never saw it. A lever that logs a
        // narrowed capability set and moves no pixel is worse than no lever,
        // because it answers a bisect with a confident wrong "not this one".
        self.capability = Self::bisected(self.capability);
        self.live = Some(binding);
        Ok(())
    }

    /// Replace one cached mesh's geometry mid-loop. wasm32 only, and a no-op when
    /// no live binding is initialised — the `Option` is consumed with
    /// `iter_mut().for_each` (a combinator, not an `if let`). The streaming run
    /// loop calls this before [`Self::present_frame`] on frames carrying new
    /// geometry, sliding the terrain mesh without rebinding.
    #[cfg(target_arch = "wasm32")]
    pub fn replace_geometry(&mut self, mesh_id: u64, vertices: &[f32], indices: &[u32]) {
        self.live
            .iter_mut()
            .for_each(|live| live.replace_geometry(mesh_id, vertices, indices));
    }

    /// Re-upload the WHOLE mesh set (the 3D peer of [`Self::load_draw2d_textures`]),
    /// so a retained scene that registered meshes after bind renders them all —
    /// windowing calls this when its mesh-set generation changes. A no-op when no
    /// live binding is initialised.
    #[cfg(target_arch = "wasm32")]
    pub fn load_meshes(&mut self, meshes: &[(u64, Vec<f32>, Vec<u32>)]) {
        self.live
            .iter_mut()
            .for_each(|live| live.load_meshes(meshes));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_host::{
        HostAlphaMode, HostApi, HostColorFormat, HostDeviceProfile, HostPowerPreference,
        HostPresentMode,
    };
    use axiom_kernel::{KernelApi, Ratio};

    /// Build a validated presentation request the way windowing does, so the
    /// native backend can be constructed and exercised end-to-end.
    pub(super) fn request(width: u32, height: u32) -> HostPresentationRequest {
        request_with_profile(width, height, HostDeviceProfile::Baseline)
    }

    /// As [`request`], but with an explicit device tier, so the tier→renderer
    /// wiring (shadow-atlas size) can be exercised on native.
    fn request_with_profile(
        width: u32,
        height: u32,
        profile: HostDeviceProfile,
    ) -> HostPresentationRequest {
        let host = HostApi::new();
        let kernel = KernelApi::new();
        let viewport = host
            .viewport(width, height, Ratio::new(1.0).expect("finite"))
            .expect("valid viewport");
        let target = host
            .presentation_target(&kernel, 1, "axiom-test-surface")
            .expect("valid target");
        let surface = host.surface_handle(&kernel, 2).expect("valid surface");
        let descriptor = host.surface_descriptor(
            viewport,
            HostPresentMode::Fifo,
            HostAlphaMode::Opaque,
            HostColorFormat::Bgra8UnormSrgb,
        );
        let adapter = host.adapter_request(HostPowerPreference::HighPerformance, true);
        let device = host.device_request(true, profile);
        host.presentation_request(target, surface, descriptor, adapter, device)
            .expect("valid request")
    }

    #[test]
    fn new_carries_the_device_tier_shadow_size() {
        // Baseline asks for a 1024² shadow atlas; ExtendedLimits opts up to 2048².
        let baseline =
            GpuBackendApi::new(&request_with_profile(800, 600, HostDeviceProfile::Baseline));
        assert_eq!(baseline.shadow_size(), 1024);
        let extended = GpuBackendApi::new(&request_with_profile(
            800,
            600,
            HostDeviceProfile::ExtendedLimits,
        ));
        assert_eq!(extended.shadow_size(), 2048);
    }

    #[test]
    fn new_carries_the_device_tier_anisotropy_budget() {
        // The mobile-first tier spends a quarter of the taps the opt-up does. The
        // live binding then takes the smaller of this and what the adapter reports,
        // so this is a ceiling on the ask, never a promise the device can meet it.
        let baseline =
            GpuBackendApi::new(&request_with_profile(800, 600, HostDeviceProfile::Baseline));
        assert_eq!(baseline.max_anisotropy(), 4);
        let extended = GpuBackendApi::new(&request_with_profile(
            800,
            600,
            HostDeviceProfile::ExtendedLimits,
        ));
        assert_eq!(extended.max_anisotropy(), 16);
    }

    #[test]
    fn new_carries_the_device_tier_render_size() {
        // An in-budget surface (under the Baseline 1600 cap) renders 1:1.
        let small =
            GpuBackendApi::new(&request_with_profile(960, 600, HostDeviceProfile::Baseline));
        assert_eq!((small.render_width(), small.render_height()), (960, 600));
        // A large (high-DPR) surface is rendered smaller, aspect preserved, then
        // upscaled on present: 3000×1500 → 1600×800 under the Baseline cap.
        let large = GpuBackendApi::new(&request_with_profile(
            3000,
            1500,
            HostDeviceProfile::Baseline,
        ));
        assert_eq!((large.render_width(), large.render_height()), (1600, 800));
        // ExtendedLimits supersamples: it asks for 2× the surface and its 4096
        // cap takes the long edge back down, so the same large surface renders
        // ABOVE the swapchain and the present resolve does the downsample.
        let extended = GpuBackendApi::new(&request_with_profile(
            3000,
            1500,
            HostDeviceProfile::ExtendedLimits,
        ));
        assert_eq!(
            (extended.render_width(), extended.render_height()),
            (4096, 2048)
        );
        assert!(extended.render_width() > 3000);
    }

    /// The render scale is a **live-binding** dial: with no binding there is
    /// nothing to resize, so the tier's render size is exactly what it was and a
    /// caller may hand it the same value every frame without consequence.
    #[test]
    fn setting_a_render_scale_without_a_live_binding_changes_no_size() {
        let mut backend = GpuBackendApi::new(&request(960, 600));
        let before = (backend.render_width(), backend.render_height());
        backend.set_render_scale(axiom_host::RenderScale::FULL);
        assert_eq!((backend.render_width(), backend.render_height()), before);
    }

    #[test]
    fn new_reads_surface_size_from_the_request() {
        let backend = GpuBackendApi::new(&request(800, 600));
        assert_eq!(backend.width(), 800);
        assert_eq!(backend.height(), 600);
        assert!(format!("{backend:?}").starts_with("GpuBackendApi"));
    }

    #[test]
    fn capability_profile_defaults_to_everything_the_code_can_do_but_claims_no_hdr() {
        // The hardware GPU attempts everything it can execute — and it can
        // execute a procedural surface now that a prepared surface's program is a
        // real pipeline with a real parameter buffer behind it. The bit was
        // cleared for exactly as long as this backend could generate a program
        // but not bind one.
        let mut backend = GpuBackendApi::new(&request(320, 240));
        assert!(backend
            .capability_profile()
            .contains(axiom_host::RenderCapability::ProceduralSurface));
        // But it does NOT claim HDR colour targets, because it has bound no
        // device and so has resolved no answer. That bit is a property of the
        // adapter, not of this source, and `initialize` grants it from what the
        // adapter reported — the difference between a measurement and a policy.
        assert!(!backend
            .capability_profile()
            .contains(axiom_host::RenderCapability::HdrTargets));
        // The same is true of the second device-resolved bit, for the same
        // reason: whether a pass may bind three colour attachments at once is a
        // property of the adapter's limits, and this backend has read none.
        assert!(!backend
            .capability_profile()
            .contains(axiom_host::RenderCapability::GBuffer));
        assert_eq!(
            backend.capability_profile(),
            axiom_host::BackendCapabilityProfile::all()
                .without(axiom_host::RenderCapability::HdrTargets)
                .without(axiom_host::RenderCapability::GBuffer)
        );
        assert_ne!(
            backend.capability_profile(),
            axiom_host::BackendCapabilityProfile::all()
        );
        // Bit 12 is set; the word the main-pass WGSL reads is unchanged, because
        // that shader reads no bit above 2048. Both device-resolved bits sit
        // ABOVE it, so the shader contract is the same word it always was — and
        // so does bit 15 (`SurfaceOrnament`), which is read on the CPU when the
        // material shader is composed and never by the shader itself.
        assert_eq!(backend.capability_profile().bits(), 0b1001_1111_1111_1111);
        // It DOES claim surface ornament by default: the full material shader is
        // what this source composes unless a host trades it away, and that trade
        // is the app's to make, not this backend's.
        assert!(backend
            .capability_profile()
            .contains(axiom_host::RenderCapability::SurfaceOrnament));
        // A host can restrict it; the present path then consults the narrowed profile.
        let restricted = axiom_host::BackendCapabilityProfile::all()
            .without(axiom_host::RenderCapability::Shadows);
        backend.set_capability_profile(restricted);
        assert_eq!(backend.capability_profile(), restricted);
        assert!(!backend
            .capability_profile()
            .contains(axiom_host::RenderCapability::Shadows));
        // **The app-facing lever for the fill-rate trade**, and the whole point
        // of routing it through the capability profile: an app asks for the lean
        // material shader by declaring the degradation, on the same surface every
        // other declared degradation uses.
        let lean = axiom_host::BackendCapabilityProfile::all()
            .without(axiom_host::RenderCapability::SurfaceOrnament);
        backend.set_capability_profile(lean);
        assert!(!backend
            .capability_profile()
            .contains(axiom_host::RenderCapability::SurfaceOrnament));
        // And nothing else moved: exactly one bit separates it from the full set.
        assert_eq!(
            lean.bits() ^ axiom_host::BackendCapabilityProfile::all().bits(),
            axiom_host::RenderCapability::SurfaceOrnament as u32
        );
    }


    #[test]
    fn native_is_never_ready_and_present_is_a_no_op() {
        let backend = GpuBackendApi::new(&request(640, 480));
        assert!(!backend.binding_is_ready());
        // One batch of one instance: mesh 7, material 5, mvp(16)+world(16)+colour(4).
        let batches = vec![(7_u64, 5_u64, vec![0.0_f32; 40], 1_u32)];
        let lights = vec![(0_u32, [0.0, 1.0, 0.0], [1.0, 1.0, 1.0], 1.0_f32)];
        let light_vp = [0.0_f32; 16];
        assert!(!backend.present_frame(
            [0.1, 0.2, 0.3, 1.0],
            &lights,
            light_vp,
            axiom_host::FrameCamera::IDENTITY,
            &batches,
            None
        ));
    }

    #[test]
    fn present_packet_consumes_a_frame_packet_and_no_ops_on_native() {
        use axiom_host::{
            FrameDrawItem, FrameFeatureSet, FrameLight, FramePacket, FrameViewport, SdfPrimitive,
            SdfScene,
        };
        let backend = GpuBackendApi::new(&request(640, 480));
        let packet = FramePacket::new(
            1,
            60,
            FrameViewport::new(640, 480),
            [0.1, 0.2, 0.3, 1.0],
            None,
            vec![FrameDrawItem::new(
                7,
                11,
                13,
                [9.0; 16],
                [1.0; 16],
                [0.4, 0.5, 0.6, 1.0],
                false,
            )],
            vec![FrameLight::new(0, [0.0, 1.0, 0.0], [1.0, 1.0, 1.0, 1.0])],
            [0.0; 16],
            FrameFeatureSet::new(false, false, 1, 0),
        );
        assert!(!backend.present_packet(&packet));
        // A packet that DOES carry a camera takes the other arm: its view-
        // projection is what the sky pass reconstructs each pixel's world ray
        // from, so a packet with a camera and one without must both present.
        // (Both no-op on native — this pins the plumbing, not pixels.)
        let with_camera = FramePacket::new(
            1,
            60,
            FrameViewport::new(640, 480),
            [0.1, 0.2, 0.3, 1.0],
            Some(axiom_host::FrameCamera::new(
                [1.0; 16],
                [2.0; 16],
                [3.0; 16],
            )),
            Vec::new(),
            Vec::new(),
            [0.0; 16],
            FrameFeatureSet::new(false, false, 0, 0),
        );
        assert!(!backend.present_packet(&with_camera));
        // The surfaced arm takes the same path with the frame's authored set.
        assert!(!backend.present_packet_with_surfaces(
            &with_camera,
            std::slice::from_ref(
                &axiom_surface::SurfaceBuilder::new()
                    .build()
                    .expect("the default surface is legal")
            )
        ));
        let prim = SdfPrimitive::new(
            SdfPrimitive::SPHERE,
            [0.0; 16],
            [1.0, 0.0, 0.0, 1.0],
            [1.0; 4],
        );
        let scene = SdfScene::new(
            vec![prim],
            [0.0; 16],
            [0.0; 16],
            [0.0, 0.0, 5.0],
            [100.0, 0.001, 0.0, 0.0],
        );
        assert!(!backend.present_packet(&packet.with_sdf(scene)));
    }

    #[test]
    fn present_draw2d_builds_geometry_and_no_ops_on_native() {
        use axiom_host::{Common2d, Draw2dCommand, Fill2d, Rect, Rgba, SpriteDraw2d, TextureId};
        use axiom_math::{Mat3, Vec2};

        let mut backend = GpuBackendApi::new(&request(640, 480));
        assert!(!backend.present_draw2d(&Draw2dList::default(), [0.0; 4]));

        backend.load_draw2d_textures(&[(7, 2, 2, vec![255; 16])]);
        let one = Ratio::new(1.0).expect("finite");
        let header = |layer: i32| (0_u32, Mat3::IDENTITY, Common2d::new(layer, one));
        let mut list = Draw2dList::default();
        list.push_command(Draw2dCommand::rect(
            header(0),
            Rect::new(Vec2::ZERO, Vec2::new(4.0, 4.0)),
            Fill2d::color(Rgba::new(
                one,
                Ratio::new(0.0).unwrap(),
                Ratio::new(0.0).unwrap(),
                one,
            )),
        ));
        list.push_command(Draw2dCommand::sprite(
            header(1),
            TextureId::from_raw(7),
            SpriteDraw2d::new(
                Rect::new(Vec2::ZERO, Vec2::new(2.0, 2.0)),
                Vec2::ZERO,
                Rgba::new(one, one, one, one),
                false,
                false,
            ),
        ));
        list.sort_commands();
        assert!(!backend.present_draw2d(&list, [0.07, 0.09, 0.14, 1.0]));
    }
}
