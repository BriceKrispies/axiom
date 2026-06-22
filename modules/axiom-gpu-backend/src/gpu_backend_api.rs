//! The single GPU-backend facade: own the real wgpu binding and present frames.

use axiom_host::{FramePacket, HostPresentationRequest};

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
        GpuBackendApi {
            width: viewport.physical_width(),
            height: viewport.physical_height(),
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
    ) -> bool {
        #[cfg(target_arch = "wasm32")]
        {
            return self
                .live
                .as_ref()
                .map(|live| {
                    live.render_frame(lights, light_view_proj, batches, clear_color)
                        .is_ok()
                })
                .unwrap_or(false);
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = (clear_color, lights, light_view_proj, batches);
            false
        }
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
        )
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
            meshes,
            materials,
            max_instances,
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
        let device = host.device_request(true, HostDeviceProfile::Baseline);
        host.presentation_request(target, surface, descriptor, adapter, device)
            .expect("valid request")
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
        assert!(!backend.present_frame([0.1, 0.2, 0.3, 1.0], &lights, light_vp, &batches));
    }

    #[test]
    fn present_packet_consumes_a_frame_packet_and_no_ops_on_native() {
        use axiom_host::{
            FrameDrawItem, FrameFeatureSet, FrameLight, FramePacket, FrameViewport,
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
            )],
            vec![FrameLight::new(0, [0.0, 1.0, 0.0], [1.0, 1.0, 1.0, 1.0])],
            [0.0; 16],
            FrameFeatureSet::new(false, false, 1, 0),
        );
        assert!(!backend.present_packet(&packet));
    }
}
