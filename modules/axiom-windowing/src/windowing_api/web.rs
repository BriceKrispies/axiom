//! The `wasm32`-only live presentation arm of the windowing facade: the browser
//! run loops, the live backend selection (WebGPU -> WebGL2 -> Canvas 2D), and the
//! DOM-facing helpers. Every item here is `#[cfg(target_arch = "wasm32")]` and the
//! whole module is gated on wasm32 from the parent, so none of it compiles (or is
//! coverage-gated) on native; the deterministic, fully-covered core lives in the
//! parent `windowing_api` module.

use super::WindowingApi;

impl WindowingApi {
    /// Install unified pointer capture (mouse + touch + pen) on the canvas with
    /// the given id, returning a handle whose [`samples`] the app reads each
    /// frame and feeds to `axiom_input::TouchControls`. `None` if no such canvas
    /// exists. wasm32 only — the capture owns DOM PointerEvent listeners.
    ///
    /// [`samples`]: crate::pointer_capture::PointerCapture::samples
    #[cfg(target_arch = "wasm32")]
    pub fn install_pointer_capture(
        canvas_id: &str,
    ) -> Option<crate::pointer_capture::PointerCapture> {
        find_canvas(canvas_id)
            .ok()
            .map(|canvas| crate::pointer_capture::PointerCapture::install(&canvas))
    }

    /// Read the device safe-area insets (notch / rounded-corner / home-indicator
    /// margins) the browser exposes via the CSS `env(safe-area-inset-*)` values,
    /// in CSS pixels `(top, right, bottom, left)`. These are the host facts a
    /// caller turns into an [`axiom_host::HostSafeAreaInsets`] to attach to its
    /// viewport (`HostViewport::with_safe_area_insets`), so engine-side UI can be
    /// laid out clear of system intrusions. Reads via a hidden probe element whose
    /// padding resolves the `env()` values; `None` if there is no DOM. wasm32 only.
    #[cfg(target_arch = "wasm32")]
    pub fn read_safe_area_insets() -> Option<(f32, f32, f32, f32)> {
        let document = web_sys::window()?.document()?;
        let body = document.body()?;
        let probe = document.create_element("div").ok()?;
        probe
            .set_attribute(
                "style",
                "position:fixed;visibility:hidden;pointer-events:none;top:0;left:0;\
                 padding-top:env(safe-area-inset-top);\
                 padding-right:env(safe-area-inset-right);\
                 padding-bottom:env(safe-area-inset-bottom);\
                 padding-left:env(safe-area-inset-left);",
            )
            .ok()?;
        body.append_child(&probe).ok()?;
        let style = web_sys::window()?.get_computed_style(&probe).ok()??;
        let read = |name: &str| -> f32 {
            style
                .get_property_value(name)
                .ok()
                .and_then(|v| v.trim_end_matches("px").trim().parse::<f32>().ok())
                .unwrap_or(0.0)
        };
        let insets = (
            read("padding-top"),
            read("padding-right"),
            read("padding-bottom"),
            read("padding-left"),
        );
        let _ = body.remove_child(&probe);
        Some(insets)
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
                // Camera view-projection + per-instance contact-shadow caster flags
                // (batch-expansion order) for the Canvas planar-shadow pass.
                [f32; 16],
                Vec<bool>,
            ) + 'static,
    {
        // Scrub-only (no fork hooks). The forkable variant lives in `run_web_forkable`.
        self.drive_web_multi(
            canvas_id,
            meshes,
            materials,
            max_instances,
            frame_fn,
            None,
            None,
        )
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
                // The camera view-projection and the per-instance contact-shadow
                // caster flags (in batch-expansion order) the Canvas planar-shadow
                // pass needs; the GPU arm ignores both.
                [f32; 16],
                Vec<bool>,
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
            // Shared so a device-loss rebuild can re-upload the same scene to a
            // fresh backend (the canvas is a cheap handle; the mesh/material data
            // is reference-counted).
            let meshes = Rc::new(meshes);
            let materials = Rc::new(materials);
            let backend = match select_backend_or_report(
                force_canvas,
                &request,
                canvas.clone(),
                &meshes[..],
                &materials[..],
                max_instances,
            )
            .await
            {
                Some(backend) => Rc::new(RefCell::new(backend)),
                None => return,
            };

            let windowing = Rc::new(RefCell::new(windowing));
            // The shared dev frame-scrubber overlay (records each presented frame;
            // re-presents it while scrubbing; forks when hooks are present).
            // `None` if there is no DOM.
            let scrubber = crate::frame_scrubber::FrameScrubber::mount(snapshot, restore);
            // Set while a device-loss rebuild is in flight, so a surface that
            // keeps failing every frame spawns exactly one rebuild, not a storm.
            let reinitializing = Rc::new(std::cell::Cell::new(false));
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
                const IDENTITY_VP: [f32; 16] = [
                    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
                ];
                // Live: step the app and record this frame. Scrubbing: freeze the
                // app (don't call its closure) and re-present the recorded frame.
                // The canvas planar-shadow inputs (camera + casters) are not
                // recorded, so scrubbed frames present without contact shadows (a
                // dev-only path); live frames carry the real values.
                let present = if scrubbing {
                    scrubber
                        .as_ref()
                        .and_then(|s| s.scrub_frame())
                        .map(|(clear, lights, light_vp, batches)| {
                            (
                                clear,
                                lights,
                                light_vp,
                                batches,
                                IDENTITY_VP,
                                Vec::<bool>::new(),
                            )
                        })
                        .unwrap_or_else(|| {
                            (
                                [0.0; 4],
                                Vec::new(),
                                [0.0; 16],
                                Vec::new(),
                                IDENTITY_VP,
                                Vec::new(),
                            )
                        })
                } else {
                    let (clear, lights, light_vp, batches, camera_vp, casters) =
                        (ff.borrow_mut())(tick);
                    if let Some(s) = scrubber.as_ref() {
                        s.record(tick, clear, &lights, light_vp, &batches);
                    }
                    (clear, lights, light_vp, batches, camera_vp, casters)
                };
                let (clear, lights, light_vp, batches, camera_vp, casters) = present;
                // Present; an unrecoverable GPU-surface loss (a backgrounded mobile
                // tab whose context was destroyed) rebuilds the backend off-loop —
                // re-probing WebGPU → WebGL2 → Canvas2D — and swaps it in, so play
                // resumes instead of going black.
                let lost = be
                    .borrow()
                    .present(
                        tick, width, height, clear, &lights, light_vp, &batches, camera_vp,
                        &casters,
                    )
                    .is_err();
                if lost && !reinitializing.get() {
                    reinitializing.set(true);
                    let be = be.clone();
                    let canvas = canvas.clone();
                    let meshes = meshes.clone();
                    let materials = materials.clone();
                    let flag = reinitializing.clone();
                    wasm_bindgen_futures::spawn_local(async move {
                        let rebuilt = select_backend(
                            force_canvas,
                            &request,
                            canvas,
                            &meshes[..],
                            &materials[..],
                            max_instances,
                        )
                        .await;
                        rebuilt
                            .into_iter()
                            .for_each(|backend| *be.borrow_mut() = backend);
                        flag.set(false);
                    });
                }
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
                // Single-mesh apps mark no contact-shadow casters, so the camera
                // is unused (identity) and the caster list is empty.
                NO_SHADOW,
                Vec::new(),
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
        // The closure returns the single-mesh instances plus the camera
        // view-projection and per-instance contact-shadow caster flags (in
        // instance order) the Canvas planar-shadow pass needs.
        F: FnMut(u64) -> ([f32; 4], Vec<f32>, u32, [f32; 16], Vec<bool>) + 'static,
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
                let (clear, instances, count, camera_vp, casters) = frame_fn(tick);
                let lights = vec![(0_u32, [0.4, 0.7, 0.6], [1.0, 1.0, 1.0], 1.0_f32)];
                (
                    clear,
                    lights,
                    NO_SHADOW,
                    vec![(SINGLE_MESH_ID, DEFAULT_MATERIAL_ID, instances, count)],
                    camera_vp,
                    casters,
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
            let backend = match select_backend_or_report(
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
                // The streaming path relies on the binding's own reconfigure-and-
                // retry for the common backgrounded-tab case; an unrecoverable loss
                // is ignored here (a full rebuild would discard the streamed-in
                // geometry), so the present result is explicitly dropped.
                // Streaming terrain marks no contact-shadow casters: identity
                // camera, empty caster list (the Canvas planar-shadow pass is a
                // no-op for it).
                const NO_CAMERA: [f32; 16] = [
                    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
                ];
                let _ = be.borrow().present(
                    tick,
                    width,
                    height,
                    clear,
                    &lights,
                    light_vp,
                    &batches,
                    NO_CAMERA,
                    &[],
                );
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
    /// Canvas arm rasterizes a frame packet reconstructed from them. Returns
    /// `Err` only when the GPU surface is **unrecoverably lost** (the run loop
    /// then rebuilds the backend); a transient hiccup the binding reconfigured
    /// around, and the always-software Canvas arm, return `Ok`.
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
        camera_view_proj: [f32; 16],
        casters: &[bool],
    ) -> Result<(), wasm_bindgen::JsValue> {
        match self {
            LiveBackend::Gpu(backend) => {
                backend.present_frame_result(clear, lights, light_vp, batches)
            }
            LiveBackend::Canvas(backend) => {
                let packet = frame_packet_from_batches(
                    tick,
                    width,
                    height,
                    clear,
                    lights,
                    light_vp,
                    batches,
                    camera_view_proj,
                    casters,
                );
                let _ = backend.present_packet(&packet);
                Ok(())
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
    camera_view_proj: [f32; 16],
    casters: &[bool],
) -> axiom_host::FramePacket {
    use axiom_host::{
        FrameCamera, FrameDrawItem, FrameFeatureSet, FrameLight, FramePacket, FrameViewport,
    };
    let mut draws = Vec::new();
    let mut object_id: u64 = 0;
    for (mesh_id, material_id, floats, count) in batches {
        for i in 0..*count {
            let off = (i as usize) * 36;
            let mvp: [f32; 16] = floats[off..off + 16].try_into().unwrap_or([0.0; 16]);
            let world: [f32; 16] = floats[off + 16..off + 32].try_into().unwrap_or([0.0; 16]);
            let color: [f32; 4] = floats[off + 32..off + 36].try_into().unwrap_or([1.0; 4]);
            // The caster flags arrive in the same instance order the batches expand
            // (see `FrameOutcome::mesh_batch_casters`); a missing flag defaults off.
            let casts = casters.get(object_id as usize).copied().unwrap_or(false);
            draws.push(FrameDrawItem::new(
                object_id,
                *mesh_id,
                *material_id,
                world,
                mvp,
                color,
                casts,
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
    // The Canvas backend's planar-shadow pass projects caster geometry through the
    // camera, so the packet carries the real camera (view/projection are unused by
    // the software path, so identity is fine; only the view-projection is read).
    let identity = [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0_f32,
    ];
    let camera = Some(FrameCamera::new(identity, identity, camera_view_proj));
    FramePacket::new(
        tick,
        tick,
        FrameViewport::new(width, height),
        clear,
        camera,
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
    // the Canvas 2D fallback. Log the GPU error rather than swallowing it, so the
    // (common, expected) GPU→Canvas2D fallback is diagnosable in the console / by
    // the Playwright suite instead of being silent.
    match gpu
        .initialize(canvas.clone(), meshes, materials, max_instances)
        .await
    {
        Ok(()) => return Some(LiveBackend::Gpu(gpu)),
        Err(err) => web_sys::console::warn_2(
            &wasm_bindgen::JsValue::from_str(
                "axiom: GPU backend init failed; falling back to Canvas2D:",
            ),
            &err,
        ),
    }
    make_canvas(request, &canvas, meshes).map(LiveBackend::Canvas)
}

/// `select_backend`, but log a distinct `console.error` when NO backend could be
/// built (GPU init failed AND the Canvas 2D fallback could not bind a context).
/// This turns a previously-silent total failure into a loud, greppable error so a
/// browser test can catch a demo that never renders. wasm32 only.
#[cfg(target_arch = "wasm32")]
async fn select_backend_or_report(
    force_canvas: bool,
    request: &axiom_host::HostPresentationRequest,
    canvas: web_sys::HtmlCanvasElement,
    meshes: &[(u64, Vec<f32>, Vec<u32>)],
    materials: &[(u64, u32, u32, Vec<u8>)],
    max_instances: u32,
) -> Option<LiveBackend> {
    let backend = select_backend(
        force_canvas,
        request,
        canvas,
        meshes,
        materials,
        max_instances,
    )
    .await;
    if backend.is_none() {
        web_sys::console::error_1(&wasm_bindgen::JsValue::from_str(
            "axiom: FATAL — no render backend available (GPU and Canvas2D both failed)",
        ));
    }
    backend
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
