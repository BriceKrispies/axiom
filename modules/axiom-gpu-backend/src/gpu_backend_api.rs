//! The single GPU-backend facade: own the real wgpu binding and present frames.

use axiom_host::{Draw2dList, FramePacket, HostPresentationRequest, SdfScene};

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
    // Render-target size the device tier renders at before upscaling to the
    // surface on present (`HostDeviceProfile::render_size`); smaller than the
    // surface on a high-DPR phone the tier caps.
    render_width: u32,
    render_height: u32,
    // Shadow-atlas edge length from the device tier
    // (`HostDeviceProfile::shadow_map_size`), handed to the renderer on initialise.
    shadow_size: u32,
    // Which optional render capabilities this backend attempts. Defaults to
    // `BackendCapabilityProfile::all()` (the hardware GPU attempts everything); a host
    // may restrict it (an fps/legibility lever) and the per-frame present consults it.
    capability: axiom_host::BackendCapabilityProfile,
    // CPU sprite/atlas textures the 2D Draw2dList sprite path samples, as
    // `(texture_id, width, height, RGBA8)` — same upload shape as the 3D
    // material set.
    draw2d_textures: Vec<(u64, u32, u32, Vec<u8>)>,
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
            capability: axiom_host::BackendCapabilityProfile::all(),
            draw2d_textures: Vec::new(),
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

    /// Restrict which optional render capabilities this backend attempts. The default
    /// is [`axiom_host::BackendCapabilityProfile::all`] (the hardware GPU attempts
    /// everything); a host may narrow it and the per-frame present
    /// ([`Self::present_frame`] / [`Self::present_packet`]) consults it, so the live GPU
    /// is no longer unconditionally full — it gates on the same profile the Canvas 2D
    /// backend does.
    pub fn set_capability_profile(&mut self, profile: axiom_host::BackendCapabilityProfile) {
        self.capability = profile;
    }

    /// The optional render capabilities this backend attempts (default
    /// [`axiom_host::BackendCapabilityProfile::all`]).
    pub fn capability_profile(&self) -> axiom_host::BackendCapabilityProfile {
        self.capability
    }

    /// The render-target width the device tier renders the scene at before
    /// upscaling to the swapchain — the tier's
    /// [`axiom_host::HostDeviceProfile::render_size`] applied to the surface.
    pub fn render_width(&self) -> u32 {
        self.render_width
    }

    /// The render-target height the device tier renders the scene at before
    /// upscaling to the swapchain.
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
        batches: &[(u64, u64, Vec<f32>, u32)],
        sdf: Option<&SdfScene>,
    ) -> bool {
        #[cfg(target_arch = "wasm32")]
        {
            return self
                .live
                .as_ref()
                .map(|live| {
                    live.render_frame(lights, light_view_proj, batches, clear_color, sdf, self.capability.bits())
                        .is_ok()
                })
                .unwrap_or(false);
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = (clear_color, lights, light_view_proj, batches, sdf);
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
    #[cfg(target_arch = "wasm32")]
    pub fn present_frame_result(
        &self,
        clear_color: [f32; 4],
        lights: &[(u32, [f32; 3], [f32; 3], f32)],
        light_view_proj: [f32; 16],
        batches: &[(u64, u64, Vec<f32>, u32)],
        sdf: Option<&SdfScene>,
    ) -> Result<(), wasm_bindgen::JsValue> {
        self.live
            .as_ref()
            .map(|live| live.render_frame(lights, light_view_proj, batches, clear_color, sdf, self.capability.bits()))
            .unwrap_or(Ok(()))
    }

    /// Present one frame from the backend-neutral [`axiom_host::FramePacket`] —
    /// the single artifact this backend and the future Canvas 2D backend both
    /// consume. It derives the live path's instance batches + lights from the
    /// packet (see [`crate::frame_packet_adapter`]) and presents them through the
    /// exact same path as [`Self::present_frame`], so behaviour is unchanged.
    /// Returns whether real pixels were drawn — always `false` on native.
    pub fn present_packet(&self, packet: &FramePacket) -> bool {
        let batches = crate::frame_packet_adapter::frame_packet_to_batches(packet);
        let lights = crate::frame_packet_adapter::frame_packet_lights(packet);
        self.present_frame(
            packet.clear_color(),
            &lights,
            packet.light_view_proj(),
            &batches,
            packet.sdf(),
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
                .map(|live| live.render_draw2d(list, &self.draw2d_textures, clear).is_ok())
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
        materials: &[(u64, u32, u32, Vec<u8>)],
        normals: &[(u64, u32, u32, Vec<u8>)],
        lights: &[(u32, [f32; 3], [f32; 3], f32)],
        light_view_proj: [f32; 16],
        batches: &[(u64, u64, Vec<f32>, u32)],
        clear: [f32; 4],
        sdf: Option<&SdfScene>,
        ambient: axiom_host::FrameAmbient,
        retro_32bit: Option<axiom_host::FrameRetro32BitProfile>,
        profile: axiom_host::BackendCapabilityProfile,
        volumetrics: Option<axiom_host::FrameVolumetrics>,
        postprocess: Option<axiom_host::FramePostProcess>,
    ) -> Option<Vec<u8>> {
        crate::offscreen::render_to_rgba(
            width,
            height,
            meshes,
            materials,
            normals,
            lights,
            light_view_proj,
            batches,
            // Skinned meshes + draws are wired from the frame packet as a follow-up;
            // the offscreen path currently renders none.
            &[],
            &[],
            clear,
            sdf,
            ambient,
            retro_32bit,
            profile,
            volumetrics,
            postprocess,
        )
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
    #[cfg(target_arch = "wasm32")]
    pub async fn initialize(
        &mut self,
        canvas: web_sys::HtmlCanvasElement,
        meshes: &[(u64, Vec<f32>, Vec<u32>)],
        materials: &[(u64, u32, u32, Vec<u8>)],
        max_instances: u32,
        preference: Option<axiom_host::BackendKind>,
    ) -> Result<(), wasm_bindgen::JsValue> {
        let binding = crate::live_gpu_binding::LiveGpuBinding::initialize(
            canvas,
            self.width,
            self.height,
            self.render_width,
            self.render_height,
            meshes,
            materials,
            max_instances,
            self.shadow_size,
            preference,
        )
        .await?;
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
        self.live.iter_mut().for_each(|live| live.load_meshes(meshes));
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
    fn request(width: u32, height: u32) -> HostPresentationRequest {
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
        // ExtendedLimits' 4096 cap leaves the same large surface 1:1.
        let extended = GpuBackendApi::new(&request_with_profile(
            3000,
            1500,
            HostDeviceProfile::ExtendedLimits,
        ));
        assert_eq!(
            (extended.render_width(), extended.render_height()),
            (3000, 1500)
        );
    }

    #[test]
    fn new_reads_surface_size_from_the_request() {
        let backend = GpuBackendApi::new(&request(800, 600));
        assert_eq!(backend.width(), 800);
        assert_eq!(backend.height(), 600);
        assert!(format!("{backend:?}").starts_with("GpuBackendApi"));
    }

    #[test]
    fn capability_profile_defaults_to_all_and_is_settable() {
        // The hardware GPU attempts everything by default.
        let mut backend = GpuBackendApi::new(&request(320, 240));
        assert_eq!(
            backend.capability_profile(),
            axiom_host::BackendCapabilityProfile::all()
        );
        // A host can restrict it; the present path then consults the narrowed profile.
        let restricted = axiom_host::BackendCapabilityProfile::all()
            .without(axiom_host::RenderCapability::Shadows);
        backend.set_capability_profile(restricted);
        assert_eq!(backend.capability_profile(), restricted);
        assert!(!backend
            .capability_profile()
            .contains(axiom_host::RenderCapability::Shadows));
    }

    #[test]
    fn native_is_never_ready_and_present_is_a_no_op() {
        let backend = GpuBackendApi::new(&request(640, 480));
        assert!(!backend.binding_is_ready());
        // One batch of one instance: mesh 7, material 5, mvp(16)+world(16)+colour(4).
        let batches = vec![(7_u64, 5_u64, vec![0.0_f32; 36], 1_u32)];
        let lights = vec![(0_u32, [0.0, 1.0, 0.0], [1.0, 1.0, 1.0], 1.0_f32)];
        let light_vp = [0.0_f32; 16];
        assert!(!backend.present_frame([0.1, 0.2, 0.3, 1.0], &lights, light_vp, &batches, None));
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
        let prim = SdfPrimitive::new(SdfPrimitive::SPHERE, [0.0; 16], [1.0, 0.0, 0.0, 1.0], [1.0; 4]);
        let scene = SdfScene::new(vec![prim], [0.0; 16], [0.0; 16], [0.0, 0.0, 5.0], [100.0, 0.001, 0.0, 0.0]);
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
            Fill2d::color(Rgba::new(one, Ratio::new(0.0).unwrap(), Ratio::new(0.0).unwrap(), one)),
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
