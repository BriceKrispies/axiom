//! The `wasm32`-only live presentation arm of the windowing facade: the browser
//! run loops, the live backend selection (WebGPU -> WebGL2 -> Canvas 2D), and the
//! DOM-facing helpers. Every item here is `#[cfg(target_arch = "wasm32")]` and the
//! whole module is gated on wasm32 from the parent, so none of it compiles (or is
//! coverage-gated) on native; the deterministic, fully-covered core lives in the
//! parent `windowing_api` module.

use super::WindowingApi;

/// One skinned draw crossing the module boundary as neutral data: `(mesh_id,
/// material_id, mvp, world, colour, joint palette)`. The joint palette is the
/// per-frame column-major matrices the LBS vertex shader blends by weight. This
/// is the flat peer of the runtime's `SkinnedDraw`, carried by value so no engine
/// type crosses the windowing facade. wasm32 only.
#[cfg(target_arch = "wasm32")]
type SkinnedDrawTuple = (u64, u64, [f32; 16], [f32; 16], [f32; 4], Vec<[f32; 16]>);

impl WindowingApi {
    /// Configure the surface **from the canvas it will actually present into**:
    /// the element's laid-out box in CSS pixels, at the page's device pixel
    /// ratio. wasm32 only — it is a DOM measurement, and the arithmetic it feeds
    /// lives in the native-testable core ([`Self::configure_surface_with_scale`]).
    ///
    /// This exists because *declaring* a surface size is guesswork an app cannot
    /// win. A page's canvas is laid out by CSS — `100vw`/`100vh`, an
    /// `aspect-ratio`, a media query — so its real box is not known until the
    /// page has laid out, and its device-pixel count is that box times a ratio
    /// only the browser knows. An app that hands `configure_surface` a
    /// compile-time `WIDTH x HEIGHT` therefore configures a surface of the wrong
    /// size *and* the wrong shape: the backend renders a frame of the declared
    /// aspect, the browser stretches it to fill the element's box, and the world
    /// is squeezed by exactly the ratio between the two aspects — while every
    /// camera resolved against the declared size compounds the error rather than
    /// cancelling it. Nothing above this module can see the discrepancy, because
    /// nothing above this module is allowed to look at the canvas. So this is
    /// where the surface stops being declared and starts being measured.
    ///
    /// Fails (leaving the surface unconfigured) when there is no such canvas, or
    /// when the page has not laid it out to a usable box yet.
    #[cfg(target_arch = "wasm32")]
    pub fn configure_surface_from_canvas(
        &mut self,
        canvas_id: &str,
        profile: axiom_host::HostDeviceProfile,
    ) -> axiom_kernel::KernelResult<()> {
        find_canvas(canvas_id)
            .ok()
            .map(|canvas| canvas.get_bounding_client_rect())
            .map(|box_| (box_.width().round(), box_.height().round()))
            .filter(|(w, h)| (*w >= 1.0) & (*h >= 1.0))
            .zip(web_sys::window().map(|w| w.device_pixel_ratio()))
            .ok_or_else(|| {
                axiom_kernel::KernelError::new(
                    axiom_kernel::KernelErrorScope::Id,
                    axiom_kernel::KernelErrorCode::InvalidId,
                    "no laid-out canvas with that id to measure the surface from",
                )
            })
            .and_then(|((width, height), ratio)| {
                // `devicePixelRatio` arrives from the DOM as an `f64` and this is
                // the boundary where it becomes an engine quantity: `Ratio::new`
                // rejects a non-finite reading here, so a browser that reported
                // NaN can never reach the surface request at all.
                axiom_kernel::Ratio::new(ratio as f32).and_then(|scale| {
                    // The tier is what the app *asked for*; what it gets is what
                    // this display can afford. `devicePixelRatio` is measured one
                    // line above and is exactly the reading that decides it — a
                    // dense panel has already done the oversampling the opt-up
                    // tier was going to pay 4× the fill rate to add. The rule
                    // itself is a pure function in `axiom_host` so it is tested
                    // natively; this line is only where the measurement meets it.
                    // It can only ever lower a tier, so no app is handed more than
                    // it requested.
                    self.configure_surface_with_scale(
                        width as u32,
                        height as u32,
                        scale,
                        profile.afforded_at_scale(scale),
                    )
                })
            })
    }

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
        materials: Vec<axiom_host::MaterialTexture>,
        max_instances: u32,
        mut frame_fn: F,
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
                // The frame's optional SDF raymarch scene (composited over the
                // meshes by both live arms). This is the one public live entry that
                // exposes the SDF arm; `None` on frames with no SDF shapes.
                Option<axiom_host::SdfScene>,
            ) + 'static,
    {
        // Scrub-only (no fork hooks). The forkable variant lives in `run_web_forkable`.
        // This variant's public closure takes only the frame identity; the engine's
        // cadence read-out is dropped here (multi-mesh apps don't surface it).
        // Read the authored render look before `self` moves into the driver.
        let look = self.render_look();
        self.drive_web_multi(
            canvas_id,
            meshes,
            materials,
            max_instances,
            move |tick, _fps_milli, _frame_micros| frame_fn(tick),
            None,
            None,
            // The plain multi-mesh arm submits no skinned bodies.
            Vec::new(),
            None,
            // ...and carries the app-authored render look to bind.
            look,
        )
    }

    /// Like [`Self::run_web_multi`], but the scene ALSO carries **skinned** bodies:
    /// `skinned_meshes` are the bake-once skinning meshes uploaded at bind (the
    /// 20-float pos/norm/uv/col/joints/weights vertex stream), and `skinned_source`
    /// is a shared cell the caller's frame closure writes this frame's skinned draws
    /// into (each `(mesh_id, material_id, mvp, world, colour, joint palette)`) — the
    /// driver reads it just before each present and hands it to the GPU skinning
    /// pass. `ambient` overrides the authored hemisphere ambient for callers that
    /// have their own; `None` keeps the look the app already bound. The frame closure returns the SAME 7-tuple as [`Self::run_web_multi`]
    /// (the skinned draws ride the shared cell, not the tuple), so the nine other
    /// `run_web_multi` callers stay untouched. wasm32 only; consumes the driver.
    #[cfg(target_arch = "wasm32")]
    #[allow(clippy::type_complexity)]
    pub fn run_web_multi_skinned<F>(
        self,
        canvas_id: &str,
        meshes: Vec<(u64, Vec<f32>, Vec<u32>)>,
        materials: Vec<axiom_host::MaterialTexture>,
        skinned_meshes: Vec<(u64, Vec<f32>, Vec<u32>)>,
        max_instances: u32,
        ambient: Option<axiom_host::FrameAmbient>,
        skinned_source: std::rc::Rc<std::cell::RefCell<Vec<SkinnedDrawTuple>>>,
        mut frame_fn: F,
    ) -> Result<(), wasm_bindgen::JsValue>
    where
        F: FnMut(
                u64,
            ) -> (
                [f32; 4],
                Vec<(u32, [f32; 3], [f32; 3], f32)>,
                [f32; 16],
                Vec<(u64, u64, Vec<f32>, u32)>,
                [f32; 16],
                Vec<bool>,
                Option<axiom_host::SdfScene>,
            ) + 'static,
    {
        // An explicit `ambient` still wins for this entry (an app with its own
        // daylight passes it directly); every other part of the look — fog, sky,
        // bloom — comes from the driver. It is an `Option` because most callers
        // want exactly the look they authored: forcing an ambient on everyone
        // would make one app's need every app's argument, and an app that has to
        // name `FrameAmbient` to say "the one I already set" is being asked to
        // depend on the host layer for a no-op.
        let look = ambient
            .into_iter()
            .fold(self.render_look(), |look, ambient| look.with_ambient(ambient));
        self.drive_web_multi(
            canvas_id,
            meshes,
            materials,
            max_instances,
            move |tick, _fps_milli, _frame_micros| frame_fn(tick),
            None,
            None,
            skinned_meshes,
            Some(skinned_source),
            look,
        )
    }

    /// Drive a **backend-comparison** loop with no nested browsing contexts: ONE
    /// shared deterministic sim presented to three canvases, each pinned to a
    /// different backend (WebGPU / WebGL2 / Canvas 2D), so the same frame is
    /// rendered three ways side by side. The engine steps the app once per rAF
    /// tick and presents the identical frame to all three presenters — each bound
    /// with a forced backend and NO scrubber overlay. This supersedes the old
    /// iframe triptych: because it is one instance and one step (not three apps
    /// kept loosely in sync), the three panes can never drift a frame apart. A
    /// pane whose backend is unavailable in this browser simply stays unbound (its
    /// canvas holds its clear colour). wasm32 only; consumes the driver.
    #[cfg(target_arch = "wasm32")]
    pub fn run_web_compare<F>(
        self,
        canvas_ids: [&str; 3],
        meshes: Vec<(u64, Vec<f32>, Vec<u32>)>,
        materials: Vec<axiom_host::MaterialTexture>,
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
                [f32; 16],
                Vec<bool>,
                Option<axiom_host::SdfScene>,
            ) + 'static,
    {
        use std::cell::RefCell;
        use std::rc::Rc;
        use wasm_bindgen::closure::Closure;

        let canvases = [
            find_canvas(canvas_ids[0])?,
            find_canvas(canvas_ids[1])?,
            find_canvas(canvas_ids[2])?,
        ];
        // The three panes, in the cascade's own preference order.
        let backends = [
            axiom_host::BackendKind::GpuPrimary,
            axiom_host::BackendKind::GpuFallback,
            axiom_host::BackendKind::Canvas2d,
        ];
        let request = match self.surface.as_ref() {
            Some(request) => *request,
            None => return Ok(()),
        };
        let meshes = Rc::new(meshes);
        let materials = Rc::new(materials);
        let slots: [Rc<RefCell<Option<LivePresenter>>>; 3] = [
            Rc::new(RefCell::new(None)),
            Rc::new(RefCell::new(None)),
            Rc::new(RefCell::new(None)),
        ];

        // Bind each pane's presenter off-loop (each backend init is async); until a
        // slot resolves, presenting to it is a no-op — the first frames simply
        // don't paint on that pane.
        let look = self.render_look();
        (0..3).for_each(|i| {
            let slot = slots[i].clone();
            let canvas = canvases[i].clone();
            let backend = backends[i];
            let meshes = meshes.clone();
            let materials = materials.clone();
            // Bound per pane (like `backend` above) so each `async move` owns its
            // own copy of the look rather than borrowing the enclosing frame.
            let look = look;
            wasm_bindgen_futures::spawn_local(async move {
                *slot.borrow_mut() = LivePresenter::bind_with(
                    request,
                    canvas,
                    Some(backend),
                    false,
                    (*meshes).clone(),
                    // The backend-comparison view uploads no skinned bodies.
                    Vec::new(),
                    (*materials).clone(),
                    max_instances,
                    // The comparison view binds each pane with the SAME authored render
                    // look, so the three backends are compared under one lighting/fog
                    // setup instead of an engine default none of them was authored for.
                    look,
                    // Three panes, three pinned backends: "which backend is bound"
                    // has no single answer here, so this loop reports none.
                    None,
                )
                .await;
            });
        });

        let windowing = Rc::new(RefCell::new(self));
        let frame_fn = Rc::new(RefCell::new(frame_fn));
        let f: Rc<RefCell<Option<Closure<dyn FnMut()>>>> = Rc::new(RefCell::new(None));
        let g = f.clone();
        let win = windowing.clone();
        let ff = frame_fn.clone();
        let panes = slots.clone();
        *g.borrow_mut() = Some(Closure::wrap(Box::new(move || {
            let tick = win.borrow_mut().step();
            let (clear, lights, light_vp, batches, camera_vp, casters, sdf) =
                (ff.borrow_mut())(tick);
            panes.iter().for_each(|slot| {
                slot.borrow().as_ref().into_iter().for_each(|presenter| {
                    presenter.present(
                        tick,
                        clear,
                        &lights,
                        light_vp,
                        &batches,
                        camera_vp,
                        &casters,
                        sdf.clone(),
                    );
                });
            });
            let next = f.borrow();
            next.as_ref().into_iter().for_each(|cb| {
                let _ = request_animation_frame(cb);
            });
        }) as Box<dyn FnMut()>));
        let initial = g.borrow();
        initial.as_ref().into_iter().for_each(|cb| {
            let _ = request_animation_frame(cb);
        });
        Ok(())
    }

    /// Bind a live presenter to the canvas for a host that owns its **own** frame
    /// loop (e.g. the `@axiom/game` TS SDK, which banks real elapsed time into
    /// fixed ticks itself). It selects the live backend (WebGPU → WebGL2 → Canvas
    /// 2D) and uploads the mesh set `meshes` and material set `materials` once,
    /// then stores the presenter in this driver so [`Self::present_frame`] can
    /// present each frame — the run loops above own the rAF loop; this hands that
    /// ownership to the caller while reusing the identical backend selection,
    /// present, and device-loss-recovery path.
    ///
    /// The backend init is asynchronous (a GPU device request), so this returns
    /// immediately and fills the presenter slot off-loop: until it resolves,
    /// [`Self::present_frame`] is a no-op (the first frames simply don't paint).
    /// Requires a configured surface ([`Self::configure_surface`]); a missing
    /// surface or canvas id leaves the slot empty (presenting stays a no-op).
    /// wasm32 only — the presenter lives entirely behind this facade, never
    /// crossing the module boundary as a second public type.
    #[cfg(target_arch = "wasm32")]
    pub fn bind_present_surface(
        &self,
        canvas_id: &str,
        meshes: Vec<(u64, Vec<f32>, Vec<u32>)>,
        materials: Vec<axiom_host::MaterialTexture>,
        max_instances: u32,
    ) {
        let request = match self.surface.as_ref() {
            Some(request) => *request,
            None => return,
        };
        let canvas = match find_canvas(canvas_id) {
            Ok(canvas) => canvas,
            Err(_) => return,
        };
        let slot = self.presenter.clone();
        let report = self.bound_backend.clone();
        let look = self.render_look();
        wasm_bindgen_futures::spawn_local(async move {
            let presenter =
                // A caller-owned-loop host (the @axiom/game SDK) uploads no skinned
                // bodies, and binds the app-authored render look.
                LivePresenter::bind(
                    request,
                    canvas,
                    meshes,
                    Vec::new(),
                    materials,
                    max_instances,
                    look,
                    report,
                )
                .await;
            *slot.borrow_mut() = presenter;
        });
    }

    /// Present one resolved frame through the bound presenter (a no-op until
    /// [`Self::bind_present_surface`]'s backend init has resolved). The arguments
    /// are the per-frame data a caller-owned loop produces — the same shape the
    /// internal run loops feed the backend; see [`LivePresenter::present`]. wasm32
    /// only.
    #[cfg(target_arch = "wasm32")]
    #[allow(clippy::too_many_arguments)]
    pub fn present_frame(
        &self,
        tick: u64,
        clear: [f32; 4],
        lights: &[(u32, [f32; 3], [f32; 3], f32)],
        light_vp: [f32; 16],
        batches: &[(u64, u64, Vec<f32>, u32)],
        camera_view_proj: [f32; 16],
        casters: &[bool],
        sdf: Option<axiom_host::SdfScene>,
    ) {
        self.presenter
            .borrow()
            .as_ref()
            .into_iter()
            .for_each(|presenter| {
                presenter.present(
                    tick,
                    clear,
                    lights,
                    light_vp,
                    batches,
                    camera_view_proj,
                    casters,
                    sdf.clone(),
                );
            });
    }

    /// Present one resolved **2D** frame through the bound presenter (a no-op until
    /// [`Self::bind_present_surface`]'s backend init has resolved). It hands the
    /// layer-sorted [`axiom_host::Draw2dList`] and its sprite/atlas `textures` to
    /// the backend the cascade selected, so a 2D game rides the identical WebGPU →
    /// WebGL2 → Canvas 2D fallback as a 3D one — the engine, not the app, owns the
    /// pixels. `textures_generation` versions the set so the backend re-uploads it
    /// only on change (see [`LivePresenter::present_2d`]). `clear` is the background
    /// colour. `tick` is the frame's identity the dev scrubber records under (the
    /// 2D peer of the `tick` [`Self::present_frame`] takes), so the overlay's
    /// frames/range/mem/hash grow and Back/Fwd/Live/Rev scrub the recorded 2D
    /// frames. Bind a 2D surface by calling [`Self::bind_present_surface`] with
    /// empty mesh/material sets. wasm32 only.
    #[cfg(target_arch = "wasm32")]
    pub fn present_2d(
        &self,
        tick: u64,
        list: &axiom_host::Draw2dList,
        textures: &[(u64, u32, u32, Vec<u8>)],
        textures_generation: u32,
        clear: [f32; 4],
    ) {
        self.presenter
            .borrow()
            .as_ref()
            .into_iter()
            .for_each(|presenter| {
                presenter.present_2d(tick, list, textures, textures_generation, clear);
            });
    }

    /// Update the bound presenter's 3D mesh set (a no-op until bind resolves, and
    /// until `generation` changes). `generation` versions the set so the backend
    /// re-uploads it only on change — the 3D peer of the `textures_generation` arg
    /// [`Self::present_2d`] takes. A caller-owned 3D loop calls this before
    /// [`Self::present_frame`] on frames where the retained scene registered a mesh
    /// or cleared, so a game's own meshes reach the GPU instead of only the
    /// bind-time (demo) set. wasm32 only.
    #[cfg(target_arch = "wasm32")]
    pub fn update_present_meshes(&self, meshes: &[(u64, Vec<f32>, Vec<u32>)], generation: u32) {
        self.presenter
            .borrow()
            .as_ref()
            .into_iter()
            .for_each(|presenter| presenter.load_meshes(meshes, generation));
    }

    /// Whether a caller-owned loop should keep stepping the game this frame —
    /// `true` when live and focused, `false` while the bound presenter's scrubber
    /// overlay is scrubbing or after focus loss (Escape / blur). `true` when no
    /// surface is bound (a 2D game pays nothing). The host gates its `advance` on
    /// this; [`Self::present_frame`] keeps painting regardless. wasm32 only.
    #[cfg(target_arch = "wasm32")]
    pub fn is_interactive(&self) -> bool {
        self.presenter
            .borrow()
            .as_ref()
            .map(LivePresenter::is_interactive)
            .unwrap_or(true)
    }

    /// The shared multi-mesh web run loop, parameterized by the optional fork
    /// hooks. `run_web_multi` (scrub-only) and `run_web_forkable` (single-mesh,
    /// forkable) both funnel through here. It owns the rAF loop and resolves each
    /// frame (live / scrubbed / paused), then delegates the actual present — and
    /// the off-loop device-loss rebuild — to a [`LivePresenter`], the same backend
    /// path [`Self::present_surface`] hands to a caller-owned loop.
    #[cfg(target_arch = "wasm32")]
    #[allow(clippy::too_many_arguments)]
    fn drive_web_multi<F>(
        self,
        canvas_id: &str,
        meshes: Vec<(u64, Vec<f32>, Vec<u32>)>,
        materials: Vec<axiom_host::MaterialTexture>,
        max_instances: u32,
        frame_fn: F,
        snapshot: Option<crate::frame_scrubber::SnapshotHook>,
        restore: Option<crate::frame_scrubber::RestoreHook>,
        // The bake-once skinned mesh set uploaded at bind (empty for the non-skinned
        // arms), and the per-frame skinned-draw source the soccer arm writes each
        // frame (the joint palettes). `None` on arms that submit no skinned bodies.
        skinned_meshes: Vec<(u64, Vec<f32>, Vec<u32>)>,
        skinned_source: Option<std::rc::Rc<std::cell::RefCell<Vec<SkinnedDrawTuple>>>>,
        // The app-authored render look applied at bind: the hemisphere ambient that
        // fills unlit faces, the atmospheric depth fog distance recedes into, the sky
        // behind the scene, and the bloom bright pixels spill through. It comes from
        // `WindowingApi`, so an app-authored night look actually reaches the live
        // browser render; an unauthored part is that part's exact no-op.
        look: axiom_host::FrameRenderLook,
    ) -> Result<(), wasm_bindgen::JsValue>
    where
        // The closure is handed this frame's identity and the engine's live
        // cadence read-out — `(frame_index, fps_milli, frame_micros)` — so a
        // consumer can surface real timing (e.g. the debug overlay) instead of
        // measuring its own parallel clock. `fps_milli` is fps × 1000 and
        // `frame_micros` is the mean frame time in microseconds (integer-encoded,
        // so no naked float crosses the boundary); both are zero until the first
        // smoothing window fills.
        F: FnMut(
                u64,
                u32,
                u32,
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
                // The frame's optional SDF raymarch scene (composited over the
                // meshes by both live arms). `None` on frames with no SDF shapes.
                Option<axiom_host::SdfScene>,
            ) + 'static,
    {
        use std::cell::RefCell;
        use std::rc::Rc;
        use wasm_bindgen::closure::Closure;

        let canvas = find_canvas(canvas_id)?;
        let frame_fn = Rc::new(RefCell::new(frame_fn));
        let windowing = self;

        wasm_bindgen_futures::spawn_local(async move {
            let request = match windowing.surface.as_ref() {
                Some(request) => *request,
                None => return,
            };
            // The live presenter owns the selected backend, the uploaded scene, and
            // the device-loss rebuild — the same path `present_surface` hands a
            // caller-owned loop. The rAF loop here only resolves each frame and
            // delegates the present.
            let presenter = match LivePresenter::bind(
                request,
                canvas,
                meshes,
                skinned_meshes,
                materials,
                max_instances,
                look,
                windowing.bound_backend.clone(),
            )
            .await
            {
                Some(presenter) => presenter,
                None => return,
            };

            // The app's adaptive render-scale request, read once per frame below.
            // Cloned out before `windowing` moves into the loop, exactly as
            // `bound_backend` is.
            let render_scale = windowing.render_scale.clone();
            let windowing = Rc::new(RefCell::new(windowing));
            // The shared dev frame-scrubber overlay (records each presented frame;
            // re-presents it while scrubbing; forks when hooks are present).
            // `None` if there is no DOM.
            let scrubber = crate::frame_scrubber::FrameScrubber::mount(snapshot, restore);
            let f: Rc<RefCell<Option<Closure<dyn FnMut()>>>> = Rc::new(RefCell::new(None));
            let g = f.clone();
            let win = windowing.clone();
            let ff = frame_fn.clone();
            // The engine's single live-cadence accumulator: fed one wall-clock
            // read per *live* frame (scrub/pause frames don't run the app, so they
            // don't pollute the measured fps), it produces the smoothed read-out
            // handed to the app closure.
            let mut frame_clock = super::FrameClock::default();
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
                                // Recorded scrub frames carry no SDF (a dev-only
                                // path), so the re-presented frame has no marcher.
                                None,
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
                                None,
                            )
                        })
                } else {
                    // Measure this live frame's wall-clock and fold it into the
                    // engine's cadence read-out, then hand the app its frame
                    // identity (`tick`) and the smoothed `(fps_milli, frame_micros)`.
                    let (fps_milli, frame_micros) = frame_clock.record(perf_now_micros());
                    let (clear, lights, light_vp, batches, camera_vp, casters, sdf) =
                        (ff.borrow_mut())(tick, fps_milli, frame_micros);
                    if let Some(s) = scrubber.as_ref() {
                        s.record(tick, clear, &lights, light_vp, &batches);
                    }
                    (clear, lights, light_vp, batches, camera_vp, casters, sdf)
                };
                let (clear, lights, light_vp, batches, camera_vp, casters, sdf) = present;
                // Hand the presenter this frame's skinned draws (the soccer arm wrote
                // them into the shared source during its closure just now); a scrubbed
                // frame carries none, matching the scrub path's dropped camera/casters.
                if let Some(src) = skinned_source.as_ref() {
                    let live_skinned = if scrubbing {
                        Vec::new()
                    } else {
                        src.borrow().clone()
                    };
                    presenter.set_skinned(live_skinned);
                }
                // Apply the app's requested render scale before presenting. The
                // cell is written by whatever the app drives its adaptive loop
                // with (`WindowingApi::render_scale_control`); a scale equal to
                // the one in use costs a comparison, so reading it every frame is
                // the cheap path and there is no separate "did it change" signal
                // for the app to get wrong.
                presenter.set_render_scale(render_scale.get());
                presenter.present(
                    tick, clear, &lights, light_vp, &batches, camera_vp, &casters, sdf,
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
        let materials = vec![axiom_host::MaterialTexture::new(
            DEFAULT_MATERIAL_ID,
            1,
            1,
            vec![255_u8, 255, 255, 255],
        )];
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
                // The single-mesh entry authors no SDF shapes.
                None,
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
        // The closure is handed this frame's identity plus the engine's live
        // cadence read-out — `(frame_index, fps_milli, frame_micros)` — and returns
        // the single-mesh instances plus the camera view-projection and
        // per-instance contact-shadow caster flags (in instance order) the Canvas
        // planar-shadow pass needs.
        F: FnMut(u64, u32, u32) -> ([f32; 4], Vec<f32>, u32, [f32; 16], Vec<bool>) + 'static,
    {
        const SINGLE_MESH_ID: u64 = 0;
        const DEFAULT_MATERIAL_ID: u64 = 0;
        let meshes = vec![(SINGLE_MESH_ID, vertices, indices)];
        let materials = vec![axiom_host::MaterialTexture::new(
            DEFAULT_MATERIAL_ID,
            1,
            1,
            vec![255_u8, 255, 255, 255],
        )];
        const NO_SHADOW: [f32; 16] = [
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ];
        // Read the authored render look before `self` moves into the driver.
        let look = self.render_look();
        self.drive_web_multi(
            canvas_id,
            meshes,
            materials,
            max_instances,
            move |tick, fps_milli, frame_micros| {
                let (clear, instances, count, camera_vp, casters) =
                    frame_fn(tick, fps_milli, frame_micros);
                let lights = vec![(0_u32, [0.4, 0.7, 0.6], [1.0, 1.0, 1.0], 1.0_f32)];
                (
                    clear,
                    lights,
                    NO_SHADOW,
                    vec![(SINGLE_MESH_ID, DEFAULT_MATERIAL_ID, instances, count)],
                    camera_vp,
                    casters,
                    // The forkable single-mesh entry authors no SDF shapes.
                    None,
                )
            },
            Some(snapshot),
            Some(restore),
            // The forkable single-mesh arm submits no skinned bodies.
            Vec::new(),
            None,
            // ...and carries the app-authored render look to bind.
            look,
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
        let preference = backend_preference();

        wasm_bindgen_futures::spawn_local(async move {
            let request = match windowing.surface.as_ref() {
                Some(request) => *request,
                None => return,
            };
            let width = request.descriptor().viewport().physical_width();
            let height = request.descriptor().viewport().physical_height();
            let meshes = vec![(STREAM_MESH_ID, vertices, indices)];
            let (mat_w, mat_h, mat_pixels) = material;
            let materials = vec![axiom_host::MaterialTexture::new(
                STREAM_MATERIAL_ID,
                mat_w,
                mat_h,
                mat_pixels,
            )];
            let backend = match select_backend_or_report(
                preference,
                &request,
                canvas,
                &meshes,
                // The streaming terrain arm submits no skinned bodies.
                &[],
                &materials,
                max_instances,
                // Streaming terrain binds the app-authored render look too.
                windowing.render_look(),
            )
            .await
            {
                Some(backend) => Rc::new(RefCell::new(backend)),
                None => return,
            };
            // Captured for the per-frame present below, which carries it so the
            // Canvas arm can declare the parts of the look it cannot honour.
            let look = windowing.render_look();
            // This arm drives a bare backend rather than a `LivePresenter`, so it
            // announces the cascade's answer itself — an app must be able to ask
            // which backend it got whichever run loop it chose.
            windowing.bound_backend.set(Some(backend.borrow().kind()));
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
                    &[],
                    NO_CAMERA,
                    &[],
                    // Streaming terrain authors no SDF shapes.
                    None,
                    look,
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

    /// Like [`Self::run_web_multi`], but one designated mesh (`stream_mesh_id`) is
    /// **streamed**: on any frame whose closure returns `Some((vertices, indices))`
    /// as its last element, that mesh's geometry is replaced in place before the
    /// frame presents. This is the walkable-world path — a terrain mesh that
    /// slides/regenerates as the camera moves, while the vegetation rides as normal
    /// instanced batches, all under the same WebGPU → WebGL2 → Canvas 2D cascade.
    ///
    /// The frame closure returns the [`Self::run_web_multi`] tuple (clear, lights,
    /// shadow view-proj, instanced batches, camera view-proj, caster flags, SDF
    /// scene) plus the optional new geometry for `stream_mesh_id`. Unlike
    /// `run_web_multi` this leaner path omits the dev frame-scrubber overlay (a
    /// streamed world re-authors its geometry every frame, so a recorded rewind is
    /// not meaningful for it yet). wasm32 only.
    #[allow(clippy::type_complexity)]
    pub fn run_web_multi_streaming<F>(
        self,
        canvas_id: &str,
        meshes: Vec<(u64, Vec<f32>, Vec<u32>)>,
        materials: Vec<axiom_host::MaterialTexture>,
        stream_mesh_id: u64,
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
                [f32; 16],
                Vec<bool>,
                Option<axiom_host::SdfScene>,
                Option<(Vec<f32>, Vec<u32>)>,
            ) + 'static,
    {
        use std::cell::RefCell;
        use std::rc::Rc;
        use wasm_bindgen::closure::Closure;

        let canvas = find_canvas(canvas_id)?;
        let frame_fn = Rc::new(RefCell::new(frame_fn));
        let windowing = self;
        let preference = backend_preference();

        wasm_bindgen_futures::spawn_local(async move {
            let request = match windowing.surface.as_ref() {
                Some(request) => *request,
                None => return,
            };
            let width = request.descriptor().viewport().physical_width();
            let height = request.descriptor().viewport().physical_height();
            let backend = match select_backend_or_report(
                preference,
                &request,
                canvas,
                &meshes,
                // The streaming terrain arm submits no skinned bodies.
                &[],
                &materials,
                max_instances,
                // Streaming terrain binds the app-authored render look too.
                windowing.render_look(),
            )
            .await
            {
                Some(backend) => Rc::new(RefCell::new(backend)),
                None => return,
            };
            // Bare-backend arm, like the streaming loop above: it reports the
            // cascade's answer itself rather than through a `LivePresenter`.
            windowing.bound_backend.set(Some(backend.borrow().kind()));
            // Captured before `windowing` moves into the loop cell below; carried
            // per frame so the Canvas arm can declare the look it cannot honour.
            let look = windowing.render_look();
            let win = Rc::new(RefCell::new(windowing));
            let f: Rc<RefCell<Option<Closure<dyn FnMut()>>>> = Rc::new(RefCell::new(None));
            let g = f.clone();
            let be = backend.clone();
            let ff = frame_fn.clone();
            *g.borrow_mut() = Some(Closure::wrap(Box::new(move || {
                let tick = win.borrow_mut().step();
                let (clear, lights, light_vp, batches, camera_vp, casters, sdf, new_geometry) =
                    (ff.borrow_mut())(tick);
                // Slide the streamed mesh on frames that carry new geometry (an empty
                // option iterates zero times — a combinator, not `if let`).
                new_geometry.into_iter().for_each(|(v, i)| {
                    be.borrow_mut().replace_geometry(stream_mesh_id, &v, &i);
                });
                let _ = be.borrow().present(
                    tick,
                    width,
                    height,
                    clear,
                    &lights,
                    light_vp,
                    &batches,
                    &[],
                    camera_vp,
                    &casters,
                    sdf,
                    look,
                );
                let next = f.borrow();
                next.as_ref().into_iter().for_each(|cb| {
                    let _ = request_animation_frame(cb);
                });
            }) as Box<dyn FnMut()>));
            let initial = g.borrow();
            initial.as_ref().into_iter().for_each(|cb| {
                let _ = request_animation_frame(cb);
            });
        });
        Ok(())
    }
}

/// A live presenter bound to a canvas: the selected backend, the scene it
/// uploaded, and the device-loss rebuild state — everything needed to present one
/// resolved frame on demand. The windowing run loops construct one internally and
/// drive it from their own rAF loop; [`WindowingApi::bind_present_surface`] stores
/// one inside the facade for a caller that owns its frame loop (the TS SDK). Either
/// way the present and the off-loop WebGPU → WebGL2 → Canvas 2D rebuild are this
/// one path. Crate-internal: it never crosses the module boundary as a public
/// type (it lives behind [`WindowingApi`], the module's one facade). wasm32 only.
#[cfg(target_arch = "wasm32")]
pub(crate) struct LivePresenter {
    backend: std::rc::Rc<std::cell::RefCell<LiveBackend>>,
    canvas: web_sys::HtmlCanvasElement,
    request: axiom_host::HostPresentationRequest,
    // Reference-counted so a device-loss rebuild re-uploads the same scene to a
    // fresh backend without re-cloning the (potentially large) vertex/pixel data.
    // The mesh set is in a `RefCell` because a retained scene can register new
    // meshes after bind: `load_meshes` swaps in the current set (so a later
    // device-loss rebuild re-uploads *that*, not the stale bind-time set).
    meshes: std::cell::RefCell<std::rc::Rc<Vec<(u64, Vec<f32>, Vec<u32>)>>>,
    materials: std::rc::Rc<Vec<axiom_host::MaterialTexture>>,
    max_instances: u32,
    width: u32,
    height: u32,
    // The forced backend for this presenter (`?backend=webgpu|webgl2|canvas2d`),
    // or `None` for the auto WebGPU→WebGL2→Canvas2D cascade. Retained so an
    // off-loop device-loss rebuild re-selects the SAME backend the caller pinned.
    preference: Option<axiom_host::BackendKind>,
    // Set while a rebuild is in flight, so a surface that keeps failing every frame
    // spawns exactly one rebuild, not a storm.
    reinitializing: std::rc::Rc<std::cell::Cell<bool>>,
    // The shared dev frame-scrubber overlay (record each presented frame; freeze +
    // re-present recorded frames while scrubbing; freeze on Escape / blur). The SAME
    // overlay the engine's own run loops mount — a caller-owned loop gets it by
    // binding here. `None` if there is no DOM. Scrub-only (no fork hooks).
    scrubber: Option<crate::frame_scrubber::FrameScrubber>,
    // The 2D sprite/atlas-texture generation last uploaded to the backend (see
    // `present_2d`). `u32::MAX` until the first present, so the first 2D frame
    // always uploads the caller's current set — which also re-applies it after an
    // async (re)bind, since a fresh presenter starts here. Bumped by the caller
    // when it adds a texture; the backend re-upload then happens at most once per
    // change, never per frame.
    applied_texture_generation: std::cell::Cell<u32>,
    // The 3D mesh-set generation last uploaded to the backend (the peer of
    // `applied_texture_generation`). `u32::MAX` until the first `load_meshes`, so
    // the first changed set always uploads — which also re-applies the current set
    // after an async (re)bind, since a fresh presenter starts here. Bumped by the
    // caller (the runtime) when it registers a mesh or clears the scene; the
    // backend re-upload then happens at most once per change, never per frame.
    applied_mesh_generation: std::cell::Cell<u32>,
    // This frame's skinned draws (mesh + material ids, mvp/world/colour, joint
    // palette) — set by the run loop before each present and consumed by
    // `present_to_backend`. Empty on apps that submit no skinned bodies.
    #[allow(clippy::type_complexity)]
    pending_skinned:
        std::cell::RefCell<Vec<(u64, u64, [f32; 16], [f32; 16], [f32; 4], Vec<[f32; 16]>)>>,
    // The bake-once skinned mesh set uploaded at bind (the 20-float skinning vertex
    // stream), retained so a device-loss rebuild re-uploads them alongside the
    // ordinary meshes. Empty on apps that submit no skinned bodies.
    skinned_meshes: Vec<(u64, Vec<f32>, Vec<u32>)>,
    // The app-authored render look bound with the backend (ambient, fog, sky,
    // bloom), retained so a device-loss rebuild restores the SAME look — a fresh
    // backend otherwise falls back to the dim engine default. Copy, so cheap to hold.
    look: axiom_host::FrameRenderLook,
    // Where this presenter announces which backend the cascade selected, so
    // `WindowingApi::bound_backend` can answer after the driver has been consumed
    // into the run loop. `None` for a presenter nobody asks about: the
    // backend-comparison loop pins one backend per pane, so "the" bound backend
    // is not a question with an answer there.
    report: Option<super::BackendReport>,
}

// The live backends hold no `Debug`; the presenter is a field of the
// `Debug`-deriving `WindowingApi`, so give it an opaque, data-free `Debug`.
#[cfg(target_arch = "wasm32")]
impl std::fmt::Debug for LivePresenter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LivePresenter")
            .field("width", &self.width)
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}

#[cfg(target_arch = "wasm32")]
impl LivePresenter {
    /// Set this frame's skinned draws — the run loop calls this before each present;
    /// [`Self::present_to_backend`] hands them to the GPU skinning pass. Empty on
    /// apps that submit no skinned bodies.
    #[allow(clippy::type_complexity)]
    pub(crate) fn set_skinned(
        &self,
        skinned: Vec<(u64, u64, [f32; 16], [f32; 16], [f32; 4], Vec<[f32; 16]>)>,
    ) {
        *self.pending_skinned.borrow_mut() = skinned;
    }

    /// Render the scene at `scale` of the device tier's render size.
    ///
    /// A no-op on the software Canvas 2D arm, which owns no render target to
    /// resize: that arm's cost is per-triangle rather than per-fragment, so
    /// resolution is not its dial and pretending otherwise would report a change
    /// that did nothing.
    pub(crate) fn set_render_scale(&self, scale: axiom_host::RenderScale) {
        self.backend.borrow_mut().set_render_scale(scale);
    }

    /// Bind a presenter to `canvas`: select the backend from `?backend=` (else the
    /// WebGPU → WebGL2 → Canvas 2D cascade), upload the mesh set `meshes` and
    /// material set `materials` once, and mount the scrub-only dev overlay so a
    /// caller-owned loop gets the frame slider + Escape/blur freeze the engine's
    /// own run loops have. `report` is the driver's backend-reading cell, which
    /// the selection is announced into (at bind, and again after every rebuild).
    /// `None` if no backend could be built.
    #[allow(clippy::too_many_arguments)]
    async fn bind(
        request: axiom_host::HostPresentationRequest,
        canvas: web_sys::HtmlCanvasElement,
        meshes: Vec<(u64, Vec<f32>, Vec<u32>)>,
        skinned_meshes: Vec<(u64, Vec<f32>, Vec<u32>)>,
        materials: Vec<axiom_host::MaterialTexture>,
        max_instances: u32,
        look: axiom_host::FrameRenderLook,
        report: super::BackendReport,
    ) -> Option<LivePresenter> {
        Self::bind_with(
            request,
            canvas,
            backend_preference(),
            true,
            meshes,
            skinned_meshes,
            materials,
            max_instances,
            look,
            Some(report),
        )
        .await
    }

    /// The general binder [`Self::bind`] funnels through. `preference` forces a
    /// specific backend (`Some`) or takes the cascade (`None`); `with_scrubber`
    /// controls whether the dev frame-scrubber overlay is mounted for this
    /// presenter — the backend-comparison loop pins each backend and mounts NO
    /// scrubber (three stacked overlays over the panes would be noise). Uploads
    /// the scene once and captures the rebuild inputs. `report` is the driver's
    /// backend-reading cell, or `None` for a presenter whose selection nobody can
    /// meaningfully ask about (the comparison loop's pinned panes). `None` if no
    /// backend could be built.
    #[allow(clippy::too_many_arguments)]
    async fn bind_with(
        request: axiom_host::HostPresentationRequest,
        canvas: web_sys::HtmlCanvasElement,
        preference: Option<axiom_host::BackendKind>,
        with_scrubber: bool,
        meshes: Vec<(u64, Vec<f32>, Vec<u32>)>,
        skinned_meshes: Vec<(u64, Vec<f32>, Vec<u32>)>,
        materials: Vec<axiom_host::MaterialTexture>,
        max_instances: u32,
        look: axiom_host::FrameRenderLook,
        report: Option<super::BackendReport>,
    ) -> Option<LivePresenter> {
        use std::cell::{Cell, RefCell};
        use std::rc::Rc;

        let width = request.descriptor().viewport().physical_width();
        let height = request.descriptor().viewport().physical_height();
        let meshes = Rc::new(meshes);
        let materials = Rc::new(materials);
        let backend = select_backend_or_report(
            preference,
            &request,
            canvas.clone(),
            &meshes[..],
            &skinned_meshes[..],
            &materials[..],
            max_instances,
            look,
        )
        .await?;
        // Announce the cascade's answer the moment it exists. Everything after
        // this point — including a rebuild that re-runs the cascade — reports
        // through `Self::report_backend`.
        report
            .iter()
            .for_each(|cell| cell.set(Some(backend.kind())));
        Some(LivePresenter {
            backend: Rc::new(RefCell::new(backend)),
            canvas,
            request,
            meshes: RefCell::new(meshes),
            materials,
            max_instances,
            width,
            height,
            preference,
            reinitializing: Rc::new(Cell::new(false)),
            scrubber: with_scrubber
                .then(|| crate::frame_scrubber::FrameScrubber::mount(None, None))
                .flatten(),
            // No 2D textures applied yet; the first `present_2d` uploads the set.
            applied_texture_generation: Cell::new(u32::MAX),
            // No mesh generation applied yet; the first changed set uploads.
            applied_mesh_generation: Cell::new(u32::MAX),
            pending_skinned: RefCell::new(Vec::new()),
            skinned_meshes,
            look,
            report,
        })
    }

    /// Present one **2D** frame, routed through the dev frame-scrubber overlay
    /// exactly as [`Self::present`] routes a 3D frame. `textures` is the caller's
    /// current sprite/atlas set and `textures_generation` its version; the set is
    /// (re)uploaded to the backend only when the version changed since the last
    /// present — so a steady stream of frames uploads no textures, while a freshly
    /// (re)bound backend re-uploads once. Then, while **live**, the frame is
    /// recorded under `tick` and the live `list` is painted; while **scrubbing**,
    /// the live `list` is ignored and the selected recorded frame is re-presented
    /// instead (the sim is frozen by the host, which gates its `advance` on
    /// [`Self::is_interactive`]). A scrubbed-to frame re-samples the *current*
    /// uploaded texture set (a dev-only simplification, mirroring the 3D scrub
    /// path's reuse of the live camera/casters): recorded frames carry their draw
    /// list, not a snapshot of every sprite atlas.
    pub fn present_2d(
        &self,
        tick: u64,
        list: &axiom_host::Draw2dList,
        textures: &[(u64, u32, u32, Vec<u8>)],
        textures_generation: u32,
        clear: [f32; 4],
    ) {
        (self.applied_texture_generation.get() != textures_generation).then(|| {
            self.backend.borrow_mut().load_2d_textures(textures);
            self.applied_texture_generation.set(textures_generation);
        });
        let scrubbing = self
            .scrubber
            .as_ref()
            .map(|s| !s.is_live())
            .unwrap_or(false);
        match scrubbing {
            true => {
                // Re-present the selected recorded 2D frame; nothing to present if
                // the timeline is empty or the payload was evicted.
                self.scrubber
                    .as_ref()
                    .and_then(|s| s.scrub_2d_frame())
                    .into_iter()
                    .for_each(|(rclear, rlist)| {
                        self.backend.borrow().present_2d(&rlist, rclear);
                    });
            }
            false => {
                // Record this live frame (a no-op while unfocused) then present it.
                self.scrubber
                    .as_ref()
                    .into_iter()
                    .for_each(|s| s.record_2d(tick, clear, list));
                self.backend.borrow().present_2d(list, clear);
            }
        }
    }

    /// Re-upload the 3D mesh set when its `generation` changed since the last
    /// upload — the peer of [`Self::present_2d`]'s `textures_generation` gate. The
    /// caller-owned loop calls this (via windowing's `update_present_meshes`)
    /// whenever the retained scene registers a mesh or clears; the set is
    /// (re)uploaded to the backend at most once per change, and stored so a later
    /// device-loss rebuild re-uploads the CURRENT meshes, not the stale bind-time
    /// set. Without this, only the meshes that existed at bind (the engine's demo
    /// scene) are ever on the GPU, and a game's later meshes render as "unknown".
    pub fn load_meshes(&self, meshes: &[(u64, Vec<f32>, Vec<u32>)], generation: u32) {
        (self.applied_mesh_generation.get() != generation).then(|| {
            self.backend.borrow_mut().load_meshes(meshes);
            *self.meshes.borrow_mut() = std::rc::Rc::new(meshes.to_vec());
            self.applied_mesh_generation.set(generation);
        });
    }

    /// Whether the caller-owned loop should keep stepping the game this frame: true
    /// when live AND focused, false while scrubbing or after focus loss (Escape /
    /// blur / tab hidden). The host gates its `advance` on this so the sim freezes
    /// exactly when the overlay says it should, while [`Self::present`] keeps
    /// painting (the frozen frame, or the scrubbed-to recorded frame).
    pub fn is_interactive(&self) -> bool {
        self.scrubber
            .as_ref()
            .map(|s| s.is_live() & s.is_active())
            .unwrap_or(true)
    }

    /// Present `tick`'s backend frame and recover from an unrecoverable GPU-surface
    /// loss by rebuilding the backend off-loop (re-probing WebGPU → WebGL2 → Canvas
    /// 2D); at most one rebuild is in flight at a time.
    #[allow(clippy::too_many_arguments)]
    fn present_to_backend(
        &self,
        tick: u64,
        clear: [f32; 4],
        lights: &[(u32, [f32; 3], [f32; 3], f32)],
        light_vp: [f32; 16],
        batches: &[(u64, u64, Vec<f32>, u32)],
        camera_view_proj: [f32; 16],
        casters: &[bool],
        sdf: Option<axiom_host::SdfScene>,
    ) {
        let lost = self
            .backend
            .borrow()
            .present(
                tick,
                self.width,
                self.height,
                clear,
                lights,
                light_vp,
                batches,
                &self.pending_skinned.borrow(),
                camera_view_proj,
                casters,
                sdf,
                self.look,
            )
            .is_err();
        if lost && !self.reinitializing.get() {
            self.reinitializing.set(true);
            let be = self.backend.clone();
            let canvas = self.canvas.clone();
            // Rebuild from the CURRENT mesh set (a retained scene may have registered
            // meshes after bind), not the stale bind-time snapshot.
            let meshes = self.meshes.borrow().clone();
            let materials = self.materials.clone();
            let skinned_meshes = self.skinned_meshes.clone();
            let look = self.look;
            let flag = self.reinitializing.clone();
            // The rebuild re-runs the WebGPU → WebGL2 → Canvas 2D cascade, so the
            // page can legitimately come back on a different backend than it went
            // down on. Re-announce it, or every consumer of
            // `WindowingApi::bound_backend` is reading a stale bind-time snapshot.
            let report = self.report.clone();
            let request = self.request;
            let preference = self.preference;
            let max_instances = self.max_instances;
            wasm_bindgen_futures::spawn_local(async move {
                let rebuilt = select_backend(
                    preference,
                    &request,
                    canvas,
                    &meshes[..],
                    &skinned_meshes[..],
                    &materials[..],
                    max_instances,
                    look,
                )
                .await;
                rebuilt.into_iter().for_each(|backend| {
                    report
                        .iter()
                        .for_each(|cell| cell.set(Some(backend.kind())));
                    *be.borrow_mut() = backend;
                });
                flag.set(false);
            });
        }
    }

    /// Present one resolved frame to the bound surface, routed through the scrubber
    /// overlay exactly as the engine's run loops do: while **live**, record this
    /// frame (the clear colour, resolved lights `(kind, geometry-vec, colour,
    /// intensity)`, shadow light view-projection, per-`(mesh, material)` instance
    /// `batches` `[mvp(16), world(16), colour(4)]` per instance, camera
    /// view-projection, per-instance contact-shadow caster flags, and optional SDF
    /// scene) and present it; while **scrubbing**, ignore the live args and
    /// re-present the selected recorded frame (the sim is frozen by the host, which
    /// gates its `advance` on [`Self::is_interactive`]). An unrecoverable
    /// GPU-surface loss (a backgrounded mobile tab whose context was destroyed)
    /// rebuilds the backend off-loop — re-probing WebGPU → WebGL2 → Canvas 2D — and
    /// swaps it in, so play resumes instead of going black; at most one rebuild is
    /// in flight at a time.
    #[allow(clippy::too_many_arguments)]
    pub fn present(
        &self,
        tick: u64,
        clear: [f32; 4],
        lights: &[(u32, [f32; 3], [f32; 3], f32)],
        light_vp: [f32; 16],
        batches: &[(u64, u64, Vec<f32>, u32)],
        camera_view_proj: [f32; 16],
        casters: &[bool],
        sdf: Option<axiom_host::SdfScene>,
    ) {
        let scrubbing = self
            .scrubber
            .as_ref()
            .map(|s| !s.is_live())
            .unwrap_or(false);
        // The identity view-projection a re-presented (scrubbed) frame uses — its
        // recorded args carry no camera/casters, matching the run loop's scrub path.
        const IDENTITY_VP: [f32; 16] = [
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ];
        match scrubbing {
            true => {
                // Re-present the selected recorded frame; nothing to present if the
                // timeline is empty or the payload was evicted.
                self.scrubber
                    .as_ref()
                    .and_then(|s| s.scrub_frame())
                    .into_iter()
                    .for_each(|(rclear, rlights, rlight_vp, rbatches)| {
                        self.present_to_backend(
                            tick,
                            rclear,
                            &rlights,
                            rlight_vp,
                            &rbatches,
                            IDENTITY_VP,
                            &[],
                            None,
                        );
                    });
            }
            false => {
                // Record this live frame (a no-op while unfocused) then present it.
                self.scrubber
                    .as_ref()
                    .into_iter()
                    .for_each(|s| s.record(tick, clear, lights, light_vp, batches));
                self.present_to_backend(
                    tick,
                    clear,
                    lights,
                    light_vp,
                    batches,
                    camera_view_proj,
                    casters,
                    sdf,
                );
            }
        }
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
    /// Render the 3D scene at `scale` of the device tier's render size.
    ///
    /// Only the GPU arm has a resizable off-swapchain target; the Canvas 2D
    /// software rasterizer draws straight into its framebuffer and its cost is
    /// per-triangle, not per-fragment, so a render scale means nothing there.
    fn set_render_scale(&mut self, scale: axiom_host::RenderScale) {
        match self {
            LiveBackend::Gpu(binding) => binding.set_render_scale(scale),
            LiveBackend::Canvas(_) => {}
        }
    }

    /// Which backend this actually is, as the neutral
    /// [`axiom_host::BackendKind`].
    ///
    /// The selection cascade (`?backend=` else WebGPU→WebGL2→Canvas 2D) runs in
    /// here and its *outcome* was previously unreportable: an app could not
    /// discover that it had silently landed on the software rasterizer, which is
    /// exactly the case where an app most wants to adapt — the Canvas 2D
    /// framebuffer is low-resolution, so geometry that is fine on the GPU
    /// degrades into sub-pixel flicker there. Reading the URL back in the app
    /// would not do: it misses every fallback, including the no-parameter page
    /// that ended up on Canvas 2D because the GPU refused a device.
    ///
    /// The GPU arm reports [`axiom_host::BackendKind::GpuPrimary`] for both
    /// WebGPU and the WebGL2 downlevel: they share a code path and a resolution,
    /// and no caller has yet needed to tell them apart. Splitting them later is
    /// additive.
    fn kind(&self) -> axiom_host::BackendKind {
        match self {
            LiveBackend::Gpu(_) => axiom_host::BackendKind::GpuPrimary,
            LiveBackend::Canvas(_) => axiom_host::BackendKind::Canvas2d,
        }
    }

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
        skinned: &[(u64, u64, [f32; 16], [f32; 16], [f32; 4], Vec<[f32; 16]>)],
        camera_view_proj: [f32; 16],
        casters: &[bool],
        sdf: Option<axiom_host::SdfScene>,
        // The app-authored look. The GPU arm bound its sky and bloom at
        // initialise; the Canvas arm carries them on the packet so it can report
        // the three capabilities it drops.
        look: axiom_host::FrameRenderLook,
    ) -> Result<(), wasm_bindgen::JsValue> {
        match self {
            LiveBackend::Gpu(backend) => backend.present_frame_result(
                clear,
                lights,
                light_vp,
                // The camera the GPU sky pass reconstructs each pixel's world ray
                // from. The Canvas arm gets the same matrix on its packet below.
                camera_view_proj,
                batches,
                skinned,
                sdf.as_ref(),
            ),
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
                    sdf,
                    look,
                );
                // CPU-skin the athlete bodies too, so the software fallback renders
                // them the way the GPU skinning pass does (uploaded at bind via
                // `make_canvas`; per-frame palettes ride in on `skinned`).
                let _ = backend.present_packet_skinned(&packet, skinned);
                Ok(())
            }
        }
    }

    /// Present one **2D** frame: rasterize the layer-sorted
    /// [`axiom_host::Draw2dList`] over `clear` through whichever backend won the
    /// cascade — the GPU arm draws it via the live wgpu 2D pipeline (a non-sRGB
    /// swap-chain view, byte-matching the software path), the Canvas arm composites
    /// it on the CPU and blits. So a 2D game rides the identical WebGPU → WebGL2 →
    /// Canvas 2D fallback as a 3D one. Both arms own their surface recovery
    /// internally, so this returns nothing.
    fn present_2d(&self, list: &axiom_host::Draw2dList, clear: [f32; 4]) {
        match self {
            LiveBackend::Gpu(backend) => {
                let _ = backend.present_draw2d(list, clear);
            }
            LiveBackend::Canvas(backend) => backend.present_draw2d(list, clear),
        }
    }

    /// Upload (replacing) the CPU sprite/atlas textures the 2D sprite/text path
    /// samples, as `(texture_id, width, height, RGBA8)` — forwarded to whichever
    /// backend won the cascade (the 2D peer of the 3D material upload).
    fn load_2d_textures(&mut self, textures: &[(u64, u32, u32, Vec<u8>)]) {
        match self {
            LiveBackend::Gpu(backend) => backend.load_draw2d_textures(textures),
            LiveBackend::Canvas(backend) => backend.load_textures(textures),
        }
    }

    /// Replace one mesh's geometry mid-loop (streaming terrain).
    fn replace_geometry(&mut self, mesh_id: u64, vertices: &[f32], indices: &[u32]) {
        match self {
            LiveBackend::Gpu(backend) => backend.replace_geometry(mesh_id, vertices, indices),
            LiveBackend::Canvas(backend) => backend.replace_geometry(mesh_id, vertices, indices),
        }
    }

    /// Re-upload (replacing) the WHOLE mesh set — the 3D peer of
    /// [`Self::load_2d_textures`]. Forwarded to whichever backend won the cascade so
    /// a retained scene that registered meshes after bind renders them all.
    fn load_meshes(&mut self, meshes: &[(u64, Vec<f32>, Vec<u32>)]) {
        match self {
            LiveBackend::Gpu(backend) => backend.load_meshes(meshes),
            LiveBackend::Canvas(backend) => backend.load_meshes(meshes),
        }
    }
}

/// The live wall clock in integer microseconds, read from the browser's
/// high-resolution `performance.now()` (milliseconds, sub-ms precision). This is
/// the module's one nondeterministic time source; it exists only here, in the
/// `wasm32` live arm, and feeds the deterministic [`super::FrameClock`]. `0` if
/// there is no DOM/performance clock (the read-out then reports zero, an honest
/// "not measured"). wasm32 only.
#[cfg(target_arch = "wasm32")]
fn perf_now_micros() -> u64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map(|p| (p.now() * 1000.0) as u64)
        .unwrap_or(0)
}

/// The backend the page pinned via `?backend=webgpu|webgl2|canvas2d`, or `None`
/// for the automatic WebGPU→WebGL2→Canvas2D cascade. This is what lets the
/// gallery's backend-comparison view boot the SAME demo three times, each iframe
/// pinned to one backend. Maps to the neutral [`axiom_host::BackendKind`]:
/// `webgpu`→`GpuPrimary`, `webgl2`(or `webgl`)→`GpuFallback`, `canvas2d`→
/// `Canvas2d`. An unknown/absent value is `None` (auto). wasm32 only — the URL is
/// the platform edge, so ordinary control flow is fine here.
#[cfg(target_arch = "wasm32")]
fn backend_preference() -> Option<axiom_host::BackendKind> {
    use axiom_host::BackendKind;
    let search = web_sys::window()
        .and_then(|w| w.location().search().ok())
        .unwrap_or_default();
    let value = search
        .split("backend=")
        .nth(1)
        .map(|rest| rest.split('&').next().unwrap_or(rest));
    match value {
        Some("canvas2d") => Some(BackendKind::Canvas2d),
        Some("webgpu") => Some(BackendKind::GpuPrimary),
        Some("webgl2") | Some("webgl") => Some(BackendKind::GpuFallback),
        _ => None,
    }
}

/// Reconstruct the backend-neutral frame packet from the per-`(mesh, material)`
/// instance batches the run loop produces: each 40-float instance is
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
    sdf: Option<axiom_host::SdfScene>,
    // The app-authored render look. The software backend honours none of its sky
    // or bloom — but the packet carries them anyway, because that is what lets it
    // REPORT the drops instead of silently ignoring them.
    look: axiom_host::FrameRenderLook,
) -> axiom_host::FramePacket {
    use axiom_host::{
        FrameCamera, FrameDrawItem, FrameFeatureSet, FrameLight, FramePacket, FrameViewport,
    };
    let mut draws = Vec::new();
    let mut object_id: u64 = 0;
    for (mesh_id, material_id, floats, count) in batches {
        for i in 0..*count {
            // 40 floats per instance: mvp(16), world(16), colour(4),
            // emissive(3)+specular(1) — the layout `FrameOutcome::mesh_batches`
            // packs (its `INSTANCE_FLOATS`).
            let off = (i as usize) * 40;
            let mvp: [f32; 16] = floats[off..off + 16].try_into().unwrap_or([0.0; 16]);
            let world: [f32; 16] = floats[off + 16..off + 32].try_into().unwrap_or([0.0; 16]);
            let color: [f32; 4] = floats[off + 32..off + 36].try_into().unwrap_or([1.0; 4]);
            let emissive: [f32; 3] = floats[off + 36..off + 39].try_into().unwrap_or([0.0; 3]);
            // The emissive vec4's fourth lane: the material's specular strength.
            let specular = floats.get(off + 39).copied().unwrap_or(0.0);
            // The caster flags arrive in the same instance order the batches expand
            // (see `FrameOutcome::mesh_batch_casters`); a missing flag defaults off.
            let casts = casters.get(object_id as usize).copied().unwrap_or(false);
            draws.push(
                FrameDrawItem::new(
                    object_id,
                    *mesh_id,
                    *material_id,
                    world,
                    mvp,
                    color,
                    casts,
                )
                .with_emissive(emissive)
                .with_specular(axiom_kernel::Ratio::finite_or_zero(specular)),
            );
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
    let packet = FramePacket::new(
        tick,
        tick,
        FrameViewport::new(width, height),
        clear,
        camera,
        draws,
        frame_lights,
        light_vp,
        features,
    );
    // Attach the frame's SDF scene (zero-or-one, via the Option iterator — no
    // `if`), so the Canvas software marcher composites it over the meshes...
    let packet = sdf.into_iter().fold(packet, |p, scene| p.with_sdf(scene));
    // ...and the look's sky and bloom, the same way. The Canvas backend cannot
    // realize either, and attaching them is precisely how it learns there was
    // something to declare: its `report` reads them back off the packet and
    // enumerates them as degraded features.
    let packet = look.sky().into_iter().fold(packet, |p, s| p.with_sky(s));
    look.bloom().into_iter().fold(packet, |p, b| p.with_bloom(b))
}

/// Select the live backend without poisoning the canvas. `preference` decides:
///
/// * `Some(Canvas2d)` — bind the software rasterizer directly (never give the
///   canvas a GPU context).
/// * `Some(GpuPrimary)` / `Some(GpuFallback)` — bind exactly that GPU API
///   (WebGPU-only / WebGL2-only) and, on failure, return `None` **without**
///   silently degrading to Canvas 2D — the comparison view wants an honest
///   per-backend result.
/// * `None` — the historical cascade: try the GPU (WebGPU→WebGL2) and fall back
///   to Canvas 2D only when it has no device.
///
/// wasm32 only.
#[cfg(target_arch = "wasm32")]
#[allow(clippy::too_many_arguments)]
async fn select_backend(
    preference: Option<axiom_host::BackendKind>,
    request: &axiom_host::HostPresentationRequest,
    canvas: web_sys::HtmlCanvasElement,
    meshes: &[(u64, Vec<f32>, Vec<u32>)],
    skinned_meshes: &[(u64, Vec<f32>, Vec<u32>)],
    materials: &[axiom_host::MaterialTexture],
    max_instances: u32,
    look: axiom_host::FrameRenderLook,
) -> Option<LiveBackend> {
    use axiom_host::BackendKind;
    if matches!(preference, Some(BackendKind::Canvas2d)) {
        return make_canvas(request, &canvas, meshes, skinned_meshes).map(LiveBackend::Canvas);
    }
    // A forced GPU tier binds exactly that API and must not fall back to Canvas2D.
    let forced_gpu = matches!(
        preference,
        Some(BackendKind::GpuPrimary | BackendKind::GpuFallback)
    );
    let mut gpu = axiom_gpu_backend::GpuBackendApi::new(request);
    // Clone the canvas handle so a GPU failure leaves the element available for
    // the Canvas 2D fallback. Log the GPU error rather than swallowing it, so the
    // (common, expected) GPU→Canvas2D fallback is diagnosable in the console / by
    // the Playwright suite instead of being silent.
    match gpu
        .initialize(
            canvas.clone(),
            meshes,
            skinned_meshes,
            materials,
            max_instances,
            // An app that authored no part of the look binds that part's no-op
            // (zero-strength fog, no sky, no bloom), so an unauthored frame is
            // byte-identical to one from before the look existed.
            look,
            preference,
        )
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
    // A forced GPU tier does not degrade; only the auto cascade reaches Canvas 2D.
    if forced_gpu {
        return None;
    }
    make_canvas(request, &canvas, meshes, skinned_meshes).map(LiveBackend::Canvas)
}

/// `select_backend`, but log a distinct `console.error` when NO backend could be
/// built (GPU init failed AND the Canvas 2D fallback could not bind a context).
/// This turns a previously-silent total failure into a loud, greppable error so a
/// browser test can catch a demo that never renders. wasm32 only.
#[cfg(target_arch = "wasm32")]
#[allow(clippy::too_many_arguments)]
async fn select_backend_or_report(
    preference: Option<axiom_host::BackendKind>,
    request: &axiom_host::HostPresentationRequest,
    canvas: web_sys::HtmlCanvasElement,
    meshes: &[(u64, Vec<f32>, Vec<u32>)],
    skinned_meshes: &[(u64, Vec<f32>, Vec<u32>)],
    materials: &[axiom_host::MaterialTexture],
    max_instances: u32,
    look: axiom_host::FrameRenderLook,
) -> Option<LiveBackend> {
    let backend = select_backend(
        preference,
        request,
        canvas,
        meshes,
        skinned_meshes,
        materials,
        max_instances,
        look,
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
    skinned_meshes: &[(u64, Vec<f32>, Vec<u32>)],
) -> Option<axiom_canvas2d_backend::Canvas2dBackendApi> {
    let mut backend = axiom_canvas2d_backend::Canvas2dBackendApi::new(request);
    backend.load_meshes(meshes);
    // The bake-once skinned athlete bodies, CPU-skinned each frame by the software
    // backend (peer of the GPU arm's skinned mesh upload at bind).
    backend.load_skinned_meshes(skinned_meshes);
    // The default is the **High** tier; `?quality=ultralow|low|medium|high` (or
    // 0..3) overrides it for testing and perf comparison.
    //
    // It used to be Low, which was a defensible default when this backend was
    // only ever a crash-guard for a device with no working GPU — but it is also
    // what made the software arm look like a different, worse game rather than
    // the same game rendered simply. At Low the internal framebuffer is 240×150,
    // so lane markings and distant scenery project to under a pixel and shimmer
    // or vanish; at High it is 426×266 and the frame reads as deliberate chunky
    // low-poly instead of broken.
    //
    // The cost is real and lands on the weakest devices: this is a CPU
    // rasterizer, and High is ~2× the pixels of Low per frame. If that proves
    // too slow on a phone, the fix is a *device-tier-aware* default (the host
    // already resolves a `HostDeviceProfile`), not a flat return to Low — a
    // desktop falling back to Canvas 2D because of a driver bug should not be
    // held to a phone's pixel budget.
    backend.set_quality_level(canvas_quality_level().unwrap_or(HIGH_QUALITY_TIER));
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

/// The Canvas 2D internal-resolution tier the software arm binds with when the
/// page does not ask for one — the top of `CanvasQualityPreset`'s `0..3` range.
#[cfg(target_arch = "wasm32")]
const HIGH_QUALITY_TIER: u8 = 3;

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
