//! The single windowing facade: assemble a presentation request, drive the loop.

use axiom_host::{
    HostAlphaMode, HostApi, HostColorFormat, HostDeviceProfile, HostError, HostPowerPreference,
    HostPresentMode, HostPresentationRequest,
};
use axiom_kernel::{
    KernelApi, KernelError, KernelErrorCode, KernelErrorScope, KernelResult, Ratio,
};

/// Deterministic kernel `HandleId` raw value for the presentation target.
const TARGET_HANDLE_RAW: u64 = 1;
/// Deterministic kernel `HandleId` raw value for the surface handle.
const SURFACE_HANDLE_RAW: u64 = 2;
/// Deterministic presentation-target label.
const TARGET_LABEL: &str = "axiom-window-surface";

/// Map a host-boundary validation failure into the kernel error model, so the
/// windowing surface reports a single `KernelResult` failure type.
fn host_to_kernel(_: HostError) -> KernelError {
    KernelError::new(
        KernelErrorScope::Id,
        KernelErrorCode::InvalidId,
        "invalid host presentation data for the window surface",
    )
}

/// The deterministic presentation driver for one window.
///
/// It holds the validated [`HostPresentationRequest`] once a surface is
/// configured, plus the fixed-step loop counters `App::run` pumps. Plain data
/// in, replayable state out — no browser or GPU object lives here. The real GPU
/// work is delegated to `axiom-gpu-backend` (the `GpuBackendApi`) on wasm32, which
/// this driver constructs from the presentation request and drives once per
/// animation frame. Two `WindowingApi`s driven with the same calls reach the same
/// observable state.
#[derive(Debug)]
pub struct WindowingApi {
    surface: Option<HostPresentationRequest>,
    next_tick: u64,
    frames_driven: u64,
}

impl WindowingApi {
    /// A fresh driver: no surface configured, loop at tick 0.
    pub fn new() -> Self {
        WindowingApi {
            surface: None,
            next_tick: 0,
            frames_driven: 0,
        }
    }

    /// Assemble and store the validated presentation request for a
    /// `width` x `height` surface. **No browser objects are touched** — this is
    /// pure host-owned data, so it runs and is tested on native exactly as it
    /// will on the web. Fails (leaving the driver unconfigured) when the host
    /// rejects the viewport dimensions.
    pub fn configure_surface(&mut self, width: u32, height: u32) -> KernelResult<()> {
        let host = HostApi::new();
        let kernel = KernelApi::new();

        // The one genuinely fallible step with caller-supplied data: the host
        // rejects a zero/oversized viewport. The remaining steps use fixed,
        // valid constants and so cannot fail (documented at each site). The
        // success arm builds and stores the request; on the viewport error we
        // return it and leave the surface unconfigured — expressed branchlessly
        // through `map`, so this carries no `?`.
        host.viewport(
            width,
            height,
            Ratio::new(1.0).expect("unit scale factor is finite"),
        )
        .map_err(host_to_kernel)
        .map(|viewport| {
            let target = host
                .presentation_target(&kernel, TARGET_HANDLE_RAW, TARGET_LABEL)
                .expect("fixed non-zero target handle and non-empty label are valid");
            let surface = host
                .surface_handle(&kernel, SURFACE_HANDLE_RAW)
                .expect("fixed non-zero surface handle is valid");
            let descriptor = host.surface_descriptor(
                viewport,
                HostPresentMode::Fifo,
                HostAlphaMode::Opaque,
                HostColorFormat::Bgra8UnormSrgb,
            );
            let adapter = host.adapter_request(HostPowerPreference::HighPerformance, true);
            let device = host.device_request(true, HostDeviceProfile::Baseline);
            let request = host
                .presentation_request(target, surface, descriptor, adapter, device)
                .expect("adapter requires a presentation surface, matching the device request");
            self.surface = Some(request);
        })
    }

    /// Whether a surface has been configured.
    pub fn is_surface_configured(&self) -> bool {
        self.surface.is_some()
    }

    /// The configured surface's physical width, if any.
    pub fn surface_width(&self) -> Option<u32> {
        self.surface
            .as_ref()
            .map(|r| r.descriptor().viewport().physical_width())
    }

    /// The configured surface's physical height, if any.
    pub fn surface_height(&self) -> Option<u32> {
        self.surface
            .as_ref()
            .map(|r| r.descriptor().viewport().physical_height())
    }

    /// The validated presentation request, once a surface is configured. This
    /// is a `host`-layer value (nameable across the engine graph, unlike a
    /// module contract type), so a consumer can drive a live presentation
    /// backend and register its surface handle from it.
    pub fn presentation_request(&self) -> Option<&HostPresentationRequest> {
        self.surface.as_ref()
    }

    /// Drive one frame of the fixed-step loop: return the tick to simulate this
    /// frame and advance the counters. Monotonic and browser-free; the web arm
    /// calls this once per animation frame, a native/headless drive in a plain
    /// loop.
    pub fn step(&mut self) -> u64 {
        let tick = self.next_tick;
        self.next_tick += 1;
        self.frames_driven += 1;
        tick
    }

    /// The next tick this driver will hand out.
    pub fn next_tick(&self) -> u64 {
        self.next_tick
    }

    /// How many frames the loop has driven.
    pub fn frames_driven(&self) -> u64 {
        self.frames_driven
    }

    /// Drive the terminal web run loop over a **multi-mesh, multi-material**
    /// scene. Construct the GPU backend from the configured presentation request,
    /// upload the distinct mesh set `meshes` (`(mesh_id, interleaved
    /// position+normal+uv+colour vertices [12 floats/vertex], triangle indices)`)
    /// and the material set `materials` (`(material_id, width, height, RGBA8
    /// albedo pixels)` — one albedo bind group per material), then present one
    /// frame per `requestAnimationFrame`: each frame the loop owns the monotonic
    /// tick ([`Self::step`]), hands it to `frame_fn`, and delegates the
    /// per-`(mesh, material)` instance batches it returns — `(clear_color,
    /// [(mesh_id, material_id, [mvp(16),colour(4)] per instance, count)])` — to
    /// the backend. wasm32 only; consumes the driver into the loop. If no surface
    /// is configured or init fails, nothing presents.
    #[cfg(target_arch = "wasm32")]
    pub fn run_web_multi<F>(
        self,
        canvas_id: &str,
        meshes: Vec<(u64, Vec<f32>, Vec<u32>)>,
        materials: Vec<(u64, u32, u32, Vec<u8>)>,
        max_instances: u32,
        frame_fn: F,
    ) -> Result<(), wasm_bindgen::JsValue>
    where
        F: FnMut(
                u64,
            ) -> (
                [f32; 4],
                Vec<(u32, [f32; 3], [f32; 3], f32)>,
                [f32; 16],
                Vec<(u64, u64, Vec<f32>, u32)>,
            ) + 'static,
    {
        // Scrub-only (no fork hooks). The forkable variant lives in `run_web_forkable`.
        self.drive_web_multi(canvas_id, meshes, materials, max_instances, frame_fn, None, None)
    }

    /// The shared multi-mesh web run loop, parameterized by the optional fork
    /// hooks. `run_web_multi` (scrub-only) and `run_web_forkable` (single-mesh,
    /// forkable) both funnel through here.
    #[cfg(target_arch = "wasm32")]
    fn drive_web_multi<F>(
        self,
        canvas_id: &str,
        meshes: Vec<(u64, Vec<f32>, Vec<u32>)>,
        materials: Vec<(u64, u32, u32, Vec<u8>)>,
        max_instances: u32,
        frame_fn: F,
        snapshot: Option<crate::frame_scrubber::SnapshotHook>,
        restore: Option<crate::frame_scrubber::RestoreHook>,
    ) -> Result<(), wasm_bindgen::JsValue>
    where
        F: FnMut(
                u64,
            ) -> (
                [f32; 4],
                Vec<(u32, [f32; 3], [f32; 3], f32)>,
                [f32; 16],
                Vec<(u64, u64, Vec<f32>, u32)>,
            ) + 'static,
    {
        use std::cell::RefCell;
        use std::rc::Rc;
        use wasm_bindgen::closure::Closure;

        let canvas = find_canvas(canvas_id)?;
        let frame_fn = Rc::new(RefCell::new(frame_fn));
        let windowing = self;
        let force_canvas = force_canvas2d();

        wasm_bindgen_futures::spawn_local(async move {
            let request = match windowing.surface.as_ref() {
                Some(request) => *request,
                None => return,
            };
            let width = request.descriptor().viewport().physical_width();
            let height = request.descriptor().viewport().physical_height();
            let backend = match select_backend(
                force_canvas,
                &request,
                canvas,
                &meshes,
                &materials,
                max_instances,
            )
            .await
            {
                Some(backend) => Rc::new(backend),
                None => return,
            };

            let windowing = Rc::new(RefCell::new(windowing));
            // The shared dev frame-scrubber overlay (records each presented frame;
            // re-presents it while scrubbing; forks when hooks are present).
            // `None` if there is no DOM.
            let scrubber = crate::frame_scrubber::FrameScrubber::mount(snapshot, restore);
            let f: Rc<RefCell<Option<Closure<dyn FnMut()>>>> = Rc::new(RefCell::new(None));
            let g = f.clone();
            let win = windowing.clone();
            let be = backend.clone();
            let ff = frame_fn.clone();
            *g.borrow_mut() = Some(Closure::wrap(Box::new(move || {
                let scrubbing = scrubber.as_ref().map(|s| !s.is_live()).unwrap_or(false);
                let paused = scrubber.as_ref().map(|s| !s.is_active()).unwrap_or(false);
                // Focus lost while live: PAUSE the whole game — don't advance the
                // tick, don't step the app, don't present. The last frame holds on
                // screen until focus returns. (Scrubbing keeps presenting recorded
                // frames; it ignores the pause.)
                if paused && !scrubbing {
                    let next = f.borrow();
                    next.as_ref().into_iter().for_each(|cb| {
                        let _ = request_animation_frame(cb);
                    });
                    return;
                }
                let tick = win.borrow_mut().step();
                // Live: step the app and record this frame. Scrubbing: freeze the
                // app (don't call its closure) and re-present the recorded frame.
                let present = if scrubbing {
                    scrubber
                        .as_ref()
                        .and_then(|s| s.scrub_frame())
                        .unwrap_or_else(|| ([0.0; 4], Vec::new(), [0.0; 16], Vec::new()))
                } else {
                    let (clear, lights, light_vp, batches) = (ff.borrow_mut())(tick);
                    if let Some(s) = scrubber.as_ref() {
                        s.record(tick, clear, &lights, light_vp, &batches);
                    }
                    (clear, lights, light_vp, batches)
                };
                let (clear, lights, light_vp, batches) = present;
                be.present(tick, width, height, clear, &lights, light_vp, &batches);
                let next = f.borrow();
                if let Some(cb) = next.as_ref() {
                    let _ = request_animation_frame(cb);
                }
            }) as Box<dyn FnMut()>));
            let initial = g.borrow();
            if let Some(cb) = initial.as_ref() {
                let _ = request_animation_frame(cb);
            }
        });
        Ok(())
    }

    /// Drive the terminal web run loop over a **single mesh** (the
    /// back-compatible shape): equivalent to [`Self::run_web_multi`] with one
    /// uploaded mesh and one untextured material (a 1×1 white albedo, so the
    /// sampled albedo is `(1,1,1,1)` and the draw colour reduces to vertex ×
    /// instance colour). Its instances are the flat `(clear_color, [mvp(16),
    /// colour(4)] per instance, count)` the closure returns. wasm32 only;
    /// consumes the driver into the loop.
    #[cfg(target_arch = "wasm32")]
    pub fn run_web<F>(
        self,
        canvas_id: &str,
        vertices: Vec<f32>,
        indices: Vec<u32>,
        max_instances: u32,
        mut frame_fn: F,
    ) -> Result<(), wasm_bindgen::JsValue>
    where
        F: FnMut(u64) -> ([f32; 4], Vec<f32>, u32) + 'static,
    {
        const SINGLE_MESH_ID: u64 = 0;
        const DEFAULT_MATERIAL_ID: u64 = 0;
        let meshes = vec![(SINGLE_MESH_ID, vertices, indices)];
        // One untextured material: a 1×1 opaque-white albedo.
        let materials = vec![(DEFAULT_MATERIAL_ID, 1, 1, vec![255_u8, 255, 255, 255])];
        // Identity light view-projection ⇒ the shadow map is unused (single-mesh
        // apps are unshadowed, matching their previous look).
        const NO_SHADOW: [f32; 16] = [
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ];
        self.run_web_multi(canvas_id, meshes, materials, max_instances, move |tick| {
            let (clear, instances, count) = frame_fn(tick);
            // One default directional light (the back-compatible fixed look).
            let lights = vec![(0_u32, [0.4, 0.7, 0.6], [1.0, 1.0, 1.0], 1.0_f32)];
            (
                clear,
                lights,
                NO_SHADOW,
                vec![(SINGLE_MESH_ID, DEFAULT_MATERIAL_ID, instances, count)],
            )
        })
    }

    /// Like [`Self::run_web`] (single mesh) but **forkable**: the dev scrubber
    /// records the app's sim state every frame via `snapshot` and grows a
    /// `⏏ fork` button that restores the selected frame's recorded state via
    /// `restore` and resumes live play from it (a new timeline branch).
    #[cfg(target_arch = "wasm32")]
    pub fn run_web_forkable<F>(
        self,
        canvas_id: &str,
        vertices: Vec<f32>,
        indices: Vec<u32>,
        max_instances: u32,
        mut frame_fn: F,
        snapshot: std::rc::Rc<dyn Fn() -> Vec<u8>>,
        restore: std::rc::Rc<dyn Fn(&[u8])>,
    ) -> Result<(), wasm_bindgen::JsValue>
    where
        F: FnMut(u64) -> ([f32; 4], Vec<f32>, u32) + 'static,
    {
        const SINGLE_MESH_ID: u64 = 0;
        const DEFAULT_MATERIAL_ID: u64 = 0;
        let meshes = vec![(SINGLE_MESH_ID, vertices, indices)];
        let materials = vec![(DEFAULT_MATERIAL_ID, 1, 1, vec![255_u8, 255, 255, 255])];
        const NO_SHADOW: [f32; 16] = [
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ];
        self.drive_web_multi(
            canvas_id,
            meshes,
            materials,
            max_instances,
            move |tick| {
                let (clear, instances, count) = frame_fn(tick);
                let lights = vec![(0_u32, [0.4, 0.7, 0.6], [1.0, 1.0, 1.0], 1.0_f32)];
                (
                    clear,
                    lights,
                    NO_SHADOW,
                    vec![(SINGLE_MESH_ID, DEFAULT_MATERIAL_ID, instances, count)],
                )
            },
            Some(snapshot),
            Some(restore),
        )
    }

    /// Drive the terminal web run loop **with streaming geometry** (a single
    /// mesh) textured by a single supplied `material` (`(width, height, RGBA8
    /// albedo pixels)` — e.g. a biome atlas the terrain samples). Identical to
    /// [`Self::run_web`] except the streamed mesh samples `material` instead of a
    /// white default, and the per-frame closure ALSO returns optional new
    /// geometry: `(clear_color, [mvp(16), colour(4)] per instance, count,
    /// Option<(vertices, indices)>)`. On the frames where it returns `Some`, the
    /// streamed mesh's buffers are replaced *before* drawing that frame, so the
    /// uploaded mesh follows the player while the camera stays continuous in world
    /// space. wasm32 only; consumes the driver into the loop.
    #[cfg(target_arch = "wasm32")]
    pub fn run_web_streaming<F>(
        self,
        canvas_id: &str,
        vertices: Vec<f32>,
        indices: Vec<u32>,
        material: (u32, u32, Vec<u8>),
        max_instances: u32,
        frame_fn: F,
    ) -> Result<(), wasm_bindgen::JsValue>
    where
        F: FnMut(
                u64,
            ) -> (
                [f32; 4],
                Vec<(u32, [f32; 3], [f32; 3], f32)>,
                [f32; 16],
                Vec<f32>,
                u32,
                Option<(Vec<f32>, Vec<u32>)>,
            ) + 'static,
    {
        use std::cell::RefCell;
        use std::rc::Rc;
        use wasm_bindgen::closure::Closure;

        const STREAM_MESH_ID: u64 = 0;
        const STREAM_MATERIAL_ID: u64 = 0;
        let canvas = find_canvas(canvas_id)?;
        let frame_fn = Rc::new(RefCell::new(frame_fn));
        let windowing = self;
        let force_canvas = force_canvas2d();

        wasm_bindgen_futures::spawn_local(async move {
            let request = match windowing.surface.as_ref() {
                Some(request) => *request,
                None => return,
            };
            let width = request.descriptor().viewport().physical_width();
            let height = request.descriptor().viewport().physical_height();
            let meshes = vec![(STREAM_MESH_ID, vertices, indices)];
            let (mat_w, mat_h, mat_pixels) = material;
            let materials = vec![(STREAM_MATERIAL_ID, mat_w, mat_h, mat_pixels)];
            let backend = match select_backend(
                force_canvas,
                &request,
                canvas,
                &meshes,
                &materials,
                max_instances,
            )
            .await
            {
                Some(backend) => Rc::new(RefCell::new(backend)),
                None => return,
            };
            let windowing = Rc::new(RefCell::new(windowing));
            // The shared dev frame-scrubber overlay (see `run_web_multi`).
            let scrubber = crate::frame_scrubber::FrameScrubber::mount(None, None);
            let f: Rc<RefCell<Option<Closure<dyn FnMut()>>>> = Rc::new(RefCell::new(None));
            let g = f.clone();
            let win = windowing.clone();
            let be = backend.clone();
            let ff = frame_fn.clone();
            *g.borrow_mut() = Some(Closure::wrap(Box::new(move || {
                let scrubbing = scrubber.as_ref().map(|s| !s.is_live()).unwrap_or(false);
                let paused = scrubber.as_ref().map(|s| !s.is_active()).unwrap_or(false);
                // Focus lost while live: PAUSE the game — don't advance the tick,
                // don't step the app (so no streamed geometry, no sim), don't
                // present. The last frame holds until focus returns.
                if paused && !scrubbing {
                    let next = f.borrow();
                    next.as_ref().into_iter().for_each(|cb| {
                        let _ = request_animation_frame(cb);
                    });
                    return;
                }
                let tick = win.borrow_mut().step();
                // Scrubbing freezes the app (its closure, and so its streamed
                // geometry, is not called) and re-presents the recorded frame.
                let present = if scrubbing {
                    scrubber
                        .as_ref()
                        .and_then(|s| s.scrub_frame())
                        .unwrap_or_else(|| ([0.0; 4], Vec::new(), [0.0; 16], Vec::new()))
                } else {
                    let (clear, lights, light_vp, instances, count, new_geometry) =
                        (ff.borrow_mut())(tick);
                    // Slide the streamed mesh on the frames that carry new geometry.
                    // The `Option` is consumed with `into_iter().for_each` (a
                    // combinator, not `if let`/`match`); an empty option iterates
                    // zero times.
                    new_geometry.into_iter().for_each(|(v, i)| {
                        be.borrow_mut().replace_geometry(STREAM_MESH_ID, &v, &i);
                    });
                    let batches = vec![(STREAM_MESH_ID, STREAM_MATERIAL_ID, instances, count)];
                    if let Some(s) = scrubber.as_ref() {
                        s.record(tick, clear, &lights, light_vp, &batches);
                    }
                    (clear, lights, light_vp, batches)
                };
                let (clear, lights, light_vp, batches) = present;
                be.borrow()
                    .present(tick, width, height, clear, &lights, light_vp, &batches);
                let next = f.borrow();
                if let Some(cb) = next.as_ref() {
                    let _ = request_animation_frame(cb);
                }
            }) as Box<dyn FnMut()>));
            let initial = g.borrow();
            if let Some(cb) = initial.as_ref() {
                let _ = request_animation_frame(cb);
            }
        });
        Ok(())
    }
}

impl Default for WindowingApi {
    fn default() -> Self {
        WindowingApi::new()
    }
}

/// The selected live presentation backend: the GPU (WebGPU/WebGL2) path or the
/// software Canvas 2D fallback. Both present the engine's per-frame data; the GPU
/// arm takes the instance batches directly (its proven route), while the Canvas
/// arm rasterizes the backend-neutral [`axiom_host::FramePacket`] reconstructed
/// from those same batches. wasm32 only.
#[cfg(target_arch = "wasm32")]
enum LiveBackend {
    Gpu(axiom_gpu_backend::GpuBackendApi),
    Canvas(axiom_canvas2d_backend::Canvas2dBackendApi),
}

#[cfg(target_arch = "wasm32")]
impl LiveBackend {
    /// Present one frame. The GPU arm draws the instance `batches` directly; the
    /// Canvas arm rasterizes a frame packet reconstructed from them.
    #[allow(clippy::too_many_arguments)]
    fn present(
        &self,
        tick: u64,
        width: u32,
        height: u32,
        clear: [f32; 4],
        lights: &[(u32, [f32; 3], [f32; 3], f32)],
        light_vp: [f32; 16],
        batches: &[(u64, u64, Vec<f32>, u32)],
    ) {
        match self {
            LiveBackend::Gpu(backend) => {
                let _ = backend.present_frame(clear, lights, light_vp, batches);
            }
            LiveBackend::Canvas(backend) => {
                let packet = frame_packet_from_batches(
                    tick, width, height, clear, lights, light_vp, batches,
                );
                let _ = backend.present_packet(&packet);
            }
        }
    }

    /// Replace one mesh's geometry mid-loop (streaming terrain).
    fn replace_geometry(&mut self, mesh_id: u64, vertices: &[f32], indices: &[u32]) {
        match self {
            LiveBackend::Gpu(backend) => backend.replace_geometry(mesh_id, vertices, indices),
            LiveBackend::Canvas(backend) => backend.replace_geometry(mesh_id, vertices, indices),
        }
    }
}

/// Whether the page asked to force the Canvas 2D backend (`?backend=canvas2d`),
/// so it can be exercised even where a GPU is available. wasm32 only.
#[cfg(target_arch = "wasm32")]
fn force_canvas2d() -> bool {
    web_sys::window()
        .and_then(|w| w.location().search().ok())
        .map(|search| search.contains("backend=canvas2d"))
        .unwrap_or(false)
}

/// Reconstruct the backend-neutral frame packet from the per-`(mesh, material)`
/// instance batches the run loop produces: each 36-float instance is
/// `mvp(16) | world(16) | colour(4)`. Object ids are assigned in draw order.
/// wasm32 only.
#[cfg(target_arch = "wasm32")]
#[allow(clippy::too_many_arguments)]
fn frame_packet_from_batches(
    tick: u64,
    width: u32,
    height: u32,
    clear: [f32; 4],
    lights: &[(u32, [f32; 3], [f32; 3], f32)],
    light_vp: [f32; 16],
    batches: &[(u64, u64, Vec<f32>, u32)],
) -> axiom_host::FramePacket {
    use axiom_host::{FrameDrawItem, FrameFeatureSet, FrameLight, FramePacket, FrameViewport};
    let mut draws = Vec::new();
    let mut object_id: u64 = 0;
    for (mesh_id, material_id, floats, count) in batches {
        for i in 0..*count {
            let off = (i as usize) * 36;
            let mvp: [f32; 16] = floats[off..off + 16].try_into().unwrap_or([0.0; 16]);
            let world: [f32; 16] = floats[off + 16..off + 32].try_into().unwrap_or([0.0; 16]);
            let color: [f32; 4] = floats[off + 32..off + 36].try_into().unwrap_or([1.0; 4]);
            draws.push(FrameDrawItem::new(
                object_id, *mesh_id, *material_id, world, mvp, color,
            ));
            object_id += 1;
        }
    }
    let frame_lights = lights
        .iter()
        .map(|(kind, vec, color, intensity)| {
            FrameLight::new(*kind, *vec, [color[0], color[1], color[2], *intensity])
        })
        .collect();
    let directional = lights.iter().filter(|(kind, ..)| *kind == 0).count() as u32;
    let point = lights.iter().filter(|(kind, ..)| *kind == 1).count() as u32;
    let features = FrameFeatureSet::new(false, directional > 0, directional, point);
    FramePacket::new(
        tick,
        tick,
        FrameViewport::new(width, height),
        clear,
        None,
        draws,
        frame_lights,
        light_vp,
        features,
    )
}

/// Select the live backend without poisoning the canvas: a forced Canvas 2D run
/// never gives the canvas a GPU context; otherwise try the GPU (WebGPU→WebGL2)
/// and fall back to Canvas 2D only when it has no device. wasm32 only.
#[cfg(target_arch = "wasm32")]
async fn select_backend(
    force_canvas: bool,
    request: &axiom_host::HostPresentationRequest,
    canvas: web_sys::HtmlCanvasElement,
    meshes: &[(u64, Vec<f32>, Vec<u32>)],
    materials: &[(u64, u32, u32, Vec<u8>)],
    max_instances: u32,
) -> Option<LiveBackend> {
    if force_canvas {
        return make_canvas(request, &canvas, meshes).map(LiveBackend::Canvas);
    }
    let mut gpu = axiom_gpu_backend::GpuBackendApi::new(request);
    // Clone the canvas handle so a GPU failure leaves the element available for
    // the Canvas 2D fallback.
    if gpu
        .initialize(canvas.clone(), meshes, materials, max_instances)
        .await
        .is_ok()
    {
        return Some(LiveBackend::Gpu(gpu));
    }
    make_canvas(request, &canvas, meshes).map(LiveBackend::Canvas)
}

/// Build a Canvas 2D backend: size from the request, upload the meshes, bind the
/// canvas's 2D context. `None` if the context cannot be acquired. wasm32 only.
#[cfg(target_arch = "wasm32")]
fn make_canvas(
    request: &axiom_host::HostPresentationRequest,
    canvas: &web_sys::HtmlCanvasElement,
    meshes: &[(u64, Vec<f32>, Vec<u32>)],
) -> Option<axiom_canvas2d_backend::Canvas2dBackendApi> {
    let mut backend = axiom_canvas2d_backend::Canvas2dBackendApi::new(request);
    backend.load_meshes(meshes);
    // The forced-fallback default is the Low tier; `?quality=ultralow|low|medium|high`
    // (or 0..3) overrides it for testing/perf comparison.
    backend.set_quality_level(canvas_quality_level().unwrap_or(1));
    backend
        .attach_canvas(canvas)
        .ok()
        .map(|()| backend)
        .inspect(|_| {
            // Mirror the GPU arm's "render backend = …" line so the browser /
            // Playwright can confirm which path is live.
            web_sys::console::log_1(&wasm_bindgen::JsValue::from_str(
                "axiom: render backend = Canvas2d profile = LowPolyFramebuffer \
                 (software z-buffer rasterizer, putImageData blit)",
            ));
        })
}

/// Parse the Canvas 2D quality tier from `?quality=` (`ultralow|low|medium|high`
/// or `0..3`), or `None` to use the default. wasm32 only — the platform edge, so
/// ordinary control flow is fine here.
#[cfg(target_arch = "wasm32")]
fn canvas_quality_level() -> Option<u8> {
    let search = web_sys::window()?.location().search().ok()?;
    let value = search.split("quality=").nth(1)?.split('&').next()?;
    match value {
        "ultralow" | "0" => Some(0),
        "low" | "1" => Some(1),
        "medium" | "2" => Some(2),
        "high" | "3" => Some(3),
        _ => None,
    }
}

/// Locate the `<canvas>` element by id. wasm32 only.
#[cfg(target_arch = "wasm32")]
fn find_canvas(canvas_id: &str) -> Result<web_sys::HtmlCanvasElement, wasm_bindgen::JsValue> {
    use wasm_bindgen::{JsCast, JsValue};

    let window = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
    let document = window
        .document()
        .ok_or_else(|| JsValue::from_str("no document"))?;
    let element = document
        .get_element_by_id(canvas_id)
        .ok_or_else(|| JsValue::from_str("canvas element not found by id"))?;
    element
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .map_err(|_| JsValue::from_str("element is not an HtmlCanvasElement"))
}

/// Schedule the next animation frame. wasm32 only.
#[cfg(target_arch = "wasm32")]
fn request_animation_frame(
    callback: &wasm_bindgen::closure::Closure<dyn FnMut()>,
) -> Result<(), wasm_bindgen::JsValue> {
    use wasm_bindgen::JsCast;

    let window = web_sys::window().ok_or_else(|| wasm_bindgen::JsValue::from_str("no window"))?;
    window
        .request_animation_frame(callback.as_ref().unchecked_ref())
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_is_unconfigured_at_tick_zero() {
        let w = WindowingApi::new();
        assert!(!w.is_surface_configured());
        assert_eq!(w.surface_width(), None);
        assert_eq!(w.surface_height(), None);
        assert!(w.presentation_request().is_none());
        assert_eq!(w.next_tick(), 0);
        assert_eq!(w.frames_driven(), 0);
        // Default matches new (compared through observable state), and the
        // driver is Debug-printable.
        let d = WindowingApi::default();
        assert_eq!(d.is_surface_configured(), w.is_surface_configured());
        assert_eq!(d.next_tick(), w.next_tick());
        assert!(format!("{w:?}").starts_with("WindowingApi"));
    }

    #[test]
    fn configure_surface_stores_validated_dimensions() {
        let mut w = WindowingApi::new();
        w.configure_surface(800, 600).expect("valid dimensions");
        assert!(w.is_surface_configured());
        assert_eq!(w.surface_width(), Some(800));
        assert_eq!(w.surface_height(), Some(600));
        // The assembled request is exposed for a live backend to consume.
        let request = w.presentation_request().expect("configured");
        assert_eq!(request.descriptor().viewport().physical_width(), 800);
        assert!(request.surface().is_valid());
    }

    #[test]
    fn configure_surface_is_deterministic() {
        // Same inputs reach the same observable state.
        let mut a = WindowingApi::new();
        let mut b = WindowingApi::new();
        a.configure_surface(1280, 720).unwrap();
        b.configure_surface(1280, 720).unwrap();
        assert_eq!(a.surface_width(), b.surface_width());
        assert_eq!(a.surface_height(), b.surface_height());
        assert_eq!(a.is_surface_configured(), b.is_surface_configured());
    }

    #[test]
    fn configure_surface_rejects_zero_dimensions_and_stays_unconfigured() {
        let mut w = WindowingApi::new();
        assert!(w.configure_surface(0, 600).is_err());
        assert!(!w.is_surface_configured());
        assert_eq!(w.surface_width(), None);
    }

    #[test]
    fn step_yields_monotonic_ticks_and_advances_counters() {
        let mut w = WindowingApi::new();
        assert_eq!(w.step(), 0);
        assert_eq!(w.step(), 1);
        assert_eq!(w.step(), 2);
        assert_eq!(w.next_tick(), 3);
        assert_eq!(w.frames_driven(), 3);
    }
}
