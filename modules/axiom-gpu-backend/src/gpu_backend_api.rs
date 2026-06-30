//! The single GPU-backend facade: own the real wgpu binding and present frames.

use axiom_host::{Draw2dList, FramePacket, HostPresentationRequest, SdfScene};

/// The real GPU presentation backend for one surface.
///
/// It is constructed from a validated [`HostPresentationRequest`] (a `host`-layer
/// value — nameable across the engine graph, unlike a module contract type), from
/// which it reads the physical surface size. On `wasm32` it then binds a real
/// `wgpu` surface/device and presents instanced draws; on native there is no GPU,
/// so it holds only the size and every present is a no-op. This keeps the
/// deterministic, native-testable surface (size + readiness + a no-op present)
/// here, with the real, browser-only `wgpu` work behind the `wasm32` arm.
#[derive(Debug)]
pub struct GpuBackendApi {
    width: u32,
    height: u32,
    // The render-target size the device tier renders the 3D scene at before
    // upscaling to the (`width`×`height`) swapchain on present
    // (`HostDeviceProfile::render_size`). Equal to the surface size on an
    // in-budget surface; smaller on a high-DPR phone the tier caps. Stored from
    // the request at construction so the tier decision is observable and
    // native-testable, even though the intermediate target only exists on wasm32.
    render_width: u32,
    render_height: u32,
    // The shadow-atlas edge length this backend's device tier asked for
    // (`HostDeviceProfile::shadow_map_size`), read from the presentation
    // request at construction and handed to the renderer on initialise. Stored
    // here so the tier decision is observable — and native-testable — even
    // though the renderer that consumes it only exists on wasm32.
    shadow_size: u32,
    // The real GPU binding, present only once initialised on wasm32. Its absence
    // is what "not ready" means; native never has one.
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
            #[cfg(target_arch = "wasm32")]
            live: None,
        }
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
                    live.render_frame(lights, light_view_proj, batches, clear_color, sdf)
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
            .map(|live| live.render_frame(lights, light_view_proj, batches, clear_color, sdf))
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
    /// peer of [`Self::present_packet`]. The live wgpu 2D raster is the
    /// browser/`offscreen` platform arm (alpha-blended, outside the coverage
    /// gate); the native facade is a no-op returning `false` (no GPU), naming the
    /// neutral list the live arm consumes. Today the software Canvas 2D backend
    /// owns the covered 2D raster; this is the wgpu entry point alongside it.
    pub fn present_draw2d(&self, list: &Draw2dList) -> bool {
        let _ = list;
        false
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
        lights: &[(u32, [f32; 3], [f32; 3], f32)],
        light_view_proj: [f32; 16],
        batches: &[(u64, u64, Vec<f32>, u32)],
        clear: [f32; 4],
        sdf: Option<&SdfScene>,
    ) -> Option<Vec<u8>> {
        crate::offscreen::render_to_rgba(
            width,
            height,
            meshes,
            materials,
            lights,
            light_view_proj,
            batches,
            clear,
            sdf,
        )
    }

    /// Initialise the real wgpu binding from a canvas, the engine's distinct mesh
    /// set (`(mesh_id, interleaved position+normal+uv+colour vertices [12
    /// floats/vertex], triangle indices)`), and the material set
    /// (`(material_id, width, height, RGBA8 albedo pixels)`) — one bind group
    /// (texture + sampler) is built per material. wasm32 only; on success later
    /// [`Self::present_frame`] calls draw real pixels. On failure the binding
    /// stays absent (not ready).
    #[cfg(target_arch = "wasm32")]
    pub async fn initialize(
        &mut self,
        canvas: web_sys::HtmlCanvasElement,
        meshes: &[(u64, Vec<f32>, Vec<u32>)],
        materials: &[(u64, u32, u32, Vec<u8>)],
        max_instances: u32,
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
        // The mobile-first Baseline tier asks the renderer for the 1024² shadow
        // atlas; ExtendedLimits opts up to 2048². The backend reads this from the
        // presentation request's device profile at construction and will hand it
        // to the renderer on initialise.
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
    fn native_is_never_ready_and_present_is_a_no_op() {
        // On native there is no GPU binding: not ready, and present draws nothing.
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
        // A packet with one draw + one light flows through the packet→batches
        // adapter and the same present path; on native it draws nothing.
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
        // No SDF attached → present_packet forwards `None`; native no-op.
        assert!(!backend.present_packet(&packet));
        // An SDF scene attached → present_packet extracts `Some(scene)` and
        // forwards it through the same path; still a native no-op (no GPU).
        let prim = SdfPrimitive::new(SdfPrimitive::SPHERE, [0.0; 16], [1.0, 0.0, 0.0, 1.0], [1.0; 4]);
        let scene = SdfScene::new(vec![prim], [0.0; 16], [0.0; 16], [0.0, 0.0, 5.0], [100.0, 0.001, 0.0, 0.0]);
        assert!(!backend.present_packet(&packet.with_sdf(scene)));
    }

    #[test]
    fn present_draw2d_consumes_a_list_and_no_ops_on_native() {
        // The 2D entry point names the neutral Draw2dList; on native (no GPU) it
        // is a no-op, exactly like present_packet. The live alpha-blended wgpu 2D
        // raster is the exempt browser/offscreen arm.
        let backend = GpuBackendApi::new(&request(640, 480));
        let list = Draw2dList::default();
        assert!(!backend.present_draw2d(&list));
    }
}
