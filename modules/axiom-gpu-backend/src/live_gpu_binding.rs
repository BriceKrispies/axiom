//! The real wgpu **swap-chain** presentation binding — wasm32 only.
//!
//! This is the surface arm: it acquires a `wgpu` surface from the browser canvas,
//! configures it, and presents one frame per call. All the actual *rendering* —
//! pipeline, mesh/material caches, lighting uniform, instance packing, draw loop —
//! lives in the shared [`crate::scene_renderer::SceneRenderer`], which the native
//! off-screen arm ([`crate::offscreen`]) uses too, so there is a single
//! definition of how a frame is drawn (no second copy to drift from).
//!
//! None of this compiles on native, so the deterministic engine, `cargo test`,
//! and the coverage gate never pull in wgpu/web-sys.

use wasm_bindgen::JsValue;
use web_sys::HtmlCanvasElement;

use crate::scene_renderer::{create_depth_view, SceneRenderer};
use crate::surface_recovery::{RecoveryAction, SurfaceStatus};
use crate::upscale::UpscaleBlit;

/// The real, browser-owned GPU objects (surface + device + queue) plus the shared
/// [`SceneRenderer`]. Each frame the scene is recorded into an **intermediate
/// colour target** sized to the device tier's render resolution (with a matching
/// depth view), then the [`UpscaleBlit`] samples that target across the acquired
/// swap-chain texture, upscaling it on present.
///
/// The surface `config` is retained so the binding can **reconfigure and
/// re-acquire** the drawing context after a backgrounded mobile browser drops it
/// (the surface then reports `Lost`/`Outdated`) — see [`Self::render_frame`].
#[derive(Debug)]
pub struct LiveGpuBinding {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    /// The reduced-resolution colour target the scene is rendered into (then
    /// upscaled to the swapchain). Sized `render_width × render_height`.
    intermediate_view: wgpu::TextureView,
    /// The depth buffer for the scene pass, sized to the intermediate target.
    depth_view: wgpu::TextureView,
    /// Presents `intermediate_view` to the swapchain with a linear upscale.
    upscale: UpscaleBlit,
    renderer: SceneRenderer,
}

/// Translate a `wgpu` surface acquisition failure into the engine's
/// [`SurfaceStatus`], whose [`SurfaceStatus::recovery_action`] decides what to do.
fn classify(error: &wgpu::SurfaceError) -> SurfaceStatus {
    match error {
        wgpu::SurfaceError::Timeout => SurfaceStatus::Timeout,
        wgpu::SurfaceError::Outdated => SurfaceStatus::Outdated,
        wgpu::SurfaceError::Lost => SurfaceStatus::Lost,
        wgpu::SurfaceError::OutOfMemory => SurfaceStatus::OutOfMemory,
        _ => SurfaceStatus::Other,
    }
}

impl LiveGpuBinding {
    /// Real GPU initialisation: pick a backend (WebGPU, else WebGL2) → surface
    /// from canvas → adapter → device/queue → configure surface → build the shared
    /// [`SceneRenderer`] (mesh + material caches, pipeline for the surface format)
    /// → depth buffer. `meshes` is `(mesh_id, 12-float vertices, indices)` and
    /// `materials` is `(material_id, width, height, RGBA8)`. Errors surface as
    /// `JsValue`.
    ///
    /// Backend selection (see docs/render-fallback.md): a browser canvas can host
    /// exactly one context type, so the backend must be chosen *before* the
    /// surface is created. We first probe a WebGPU adapter via `navigator.gpu`
    /// (no canvas context needed); if one exists we present through WebGPU, and if
    /// it does not we fall back to wgpu's WebGL2 backend. The same shared
    /// [`SceneRenderer`], shaders, and instancing run unchanged on either, since
    /// the renderer is already held to `downlevel_webgl2_defaults` limits.
    #[allow(clippy::too_many_arguments)]
    pub async fn initialize(
        canvas: HtmlCanvasElement,
        width: u32,
        height: u32,
        render_width: u32,
        render_height: u32,
        meshes: &[(u64, Vec<f32>, Vec<u32>)],
        materials: &[(u64, u32, u32, Vec<u8>)],
        max_instances: u32,
        shadow_size: u32,
    ) -> Result<LiveGpuBinding, JsValue> {
        let width = width.max(1);
        let height = height.max(1);
        // The scene renders at the device tier's resolution (`render_size`),
        // never larger than the swapchain; it is upscaled to `width × height` on
        // present.
        let render_width = render_width.max(1).min(width);
        let render_height = render_height.max(1).min(height);

        // Probe WebGPU *fully* — adapter AND device — on its own instance, with no
        // canvas (`navigator.gpu` needs none), so the probe never acquires the
        // canvas's one-and-only context slot. We require a working device, not
        // just an adapter: some browsers expose a WebGPU adapter whose device
        // creation then fails ("Device failed at creation"), and since a canvas
        // context type cannot be reclaimed once taken, committing the canvas on
        // adapter presence alone would strand us on a dead backend with no way
        // back to WebGL2. Only a live device commits to WebGPU.
        let webgpu = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::BROWSER_WEBGPU,
            ..Default::default()
        });
        let webgpu_ready = match webgpu
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: None,
            })
            .await
        {
            Ok(adapter) => request_render_device(&adapter)
                .await
                .ok()
                .map(|(device, queue)| (adapter, device, queue)),
            Err(_) => None,
        };

        // WebGPU if its device is live, else WebGL2. Each arm creates the surface
        // on the instance whose backend it committed to (the canvas context is
        // acquired there), so the two never contend for the one context slot.
        let (surface, adapter, device, queue) = match webgpu_ready {
            Some((adapter, device, queue)) => {
                let surface = webgpu
                    .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
                    .map_err(|e| JsValue::from_str(&format!("create_surface failed: {e}")))?;
                (surface, adapter, device, queue)
            }
            None => {
                let gl = wgpu::Instance::new(&wgpu::InstanceDescriptor {
                    backends: wgpu::Backends::GL,
                    ..Default::default()
                });
                let surface = gl
                    .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
                    .map_err(|e| JsValue::from_str(&format!("create_surface failed: {e}")))?;
                let adapter = gl
                    .request_adapter(&wgpu::RequestAdapterOptions {
                        power_preference: wgpu::PowerPreference::HighPerformance,
                        force_fallback_adapter: false,
                        compatible_surface: Some(&surface),
                    })
                    .await
                    .map_err(|e| {
                        JsValue::from_str(&format!("no WebGPU and WebGL2 adapter failed: {e}"))
                    })?;
                let (device, queue) = request_render_device(&adapter)
                    .await
                    .map_err(|e| JsValue::from_str(&format!("request_device failed: {e}")))?;
                (surface, adapter, device, queue)
            }
        };

        // Record which backend won, so the browser console (and Playwright) can
        // confirm whether the WebGPU path or the WebGL2 fallback is live.
        web_sys::console::log_1(&JsValue::from_str(&format!(
            "axiom: render backend = {:?}",
            adapter.get_info().backend
        )));

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width,
            height,
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&device, &config);

        let renderer = SceneRenderer::new(
            &device,
            &queue,
            format,
            meshes,
            materials,
            max_instances,
            shadow_size,
        );

        // The intermediate colour target the scene renders into (then upscaled to
        // the swapchain). Same format as the surface, plus `TEXTURE_BINDING` so the
        // blit can sample it. Its depth view matches it, not the swapchain.
        let intermediate = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("axiom-render-target"),
            size: wgpu::Extent3d {
                width: render_width,
                height: render_height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let intermediate_view = intermediate.create_view(&wgpu::TextureViewDescriptor::default());
        let depth_view = create_depth_view(&device, render_width, render_height);
        let upscale = UpscaleBlit::new(&device, format, &intermediate_view);

        Ok(LiveGpuBinding {
            surface,
            device,
            queue,
            config,
            intermediate_view,
            depth_view,
            upscale,
            renderer,
        })
    }

    /// Acquire the next swap-chain texture, **recovering a dropped context** when
    /// the browser backgrounded the tab (a mobile-first necessity). On a
    /// `Lost`/`Outdated`/other failure the surface is reconfigured with its stored
    /// config — re-acquiring the WebGPU/WebGL context — and acquisition is retried
    /// once; a `Timeout` skips the frame; `OutOfMemory` signals a full rebuild. The
    /// returned `Ok(None)` means "skip this frame cleanly" (the context will be
    /// healthy again shortly), `Err` means the binding must be reinitialised.
    fn acquire_texture(&self) -> Result<Option<wgpu::SurfaceTexture>, JsValue> {
        match self.surface.get_current_texture() {
            Ok(frame) => Ok(Some(frame)),
            Err(error) => match classify(&error).recovery_action() {
                RecoveryAction::SkipFrame => Ok(None),
                RecoveryAction::Reconfigure => {
                    // Re-acquire the dropped drawing context, then retry once. A
                    // still-failing acquisition skips this frame; the next frame
                    // tries again from a freshly configured surface.
                    self.surface.configure(&self.device, &self.config);
                    Ok(self.surface.get_current_texture().ok())
                }
                RecoveryAction::Reinitialize => Err(JsValue::from_str(
                    "gpu surface unrecoverable (out of memory): binding needs reinitialize",
                )),
            },
        }
    }

    /// Draw + present one real frame from per-`(mesh, material)` instance batches
    /// and the frame's `lights`. The scene is recorded into the reduced-resolution
    /// intermediate target by the shared [`SceneRenderer`], then the
    /// [`UpscaleBlit`] samples it across the acquired swap-chain texture (upscaling
    /// on present). Real pixels. A frame skipped for surface recovery (see
    /// [`Self::acquire_texture`]) presents nothing and returns `Ok`.
    pub fn render_frame(
        &self,
        lights: &[(u32, [f32; 3], [f32; 3], f32)],
        light_view_proj: [f32; 16],
        batches: &[(u64, u64, Vec<f32>, u32)],
        clear: [f32; 4],
    ) -> Result<(), JsValue> {
        let frame = match self.acquire_texture()? {
            Some(frame) => frame,
            None => return Ok(()),
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        // Render the scene at tier resolution into the intermediate target
        // (renderer owns its own encoder + submit), ...
        self.renderer.record(
            &self.device,
            &self.queue,
            &self.intermediate_view,
            &self.depth_view,
            lights,
            light_view_proj,
            batches,
            clear,
        );
        // ... then upscale-blit it across the full swapchain view and present.
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("axiom-upscale-encoder"),
            });
        self.upscale.record(&mut encoder, &view);
        self.queue.submit(std::iter::once(encoder.finish()));
        frame.present();
        Ok(())
    }

    /// Replace one cached mesh's geometry mid-loop (sliding terrain streaming).
    pub fn replace_geometry(&mut self, mesh_id: u64, vertices: &[f32], indices: &[u32]) {
        self.renderer
            .replace_geometry(&self.device, mesh_id, vertices, indices);
    }
}

/// Request the render device + queue from an adapter, with the engine's shared
/// descriptor (`downlevel_webgl2_defaults` limits so WebGPU and WebGL2 agree).
/// Used both to *probe* WebGPU viability before committing the canvas and to
/// create the real device on the chosen backend.
async fn request_render_device(
    adapter: &wgpu::Adapter,
) -> Result<(wgpu::Device, wgpu::Queue), wgpu::RequestDeviceError> {
    adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("axiom-live-device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_webgl2_defaults(),
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::Off,
        })
        .await
}
