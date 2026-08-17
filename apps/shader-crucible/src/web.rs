//! **The live browser arm, and why this app had to write its own.**
//!
//! Every other browser app in this repo hands its scene to `App::run`, which
//! drives `axiom-windowing`'s `requestAnimationFrame` loop. The crucible cannot,
//! for two independent reasons — and the second is the finding this whole app
//! exists to surface.
//!
//! **1. `App::run` builds its own scene.** It calls `build()` internally and
//! sizes the live instance buffer from `RunningApp::renderable_count()`, which
//! counts only what the `setup` closure authored. The crucible registers its
//! bodies *after* `build()`, because station 3 needs `add_texture_data` and
//! station 7 needs `add_mesh_data`, both of which live on `RunningApp`.
//!
//! **2. `axiom-windowing`'s loop cannot carry a surface at all.** Its GPU arm
//! calls `GpuBackendApi::present_frame_result`, which takes explicit instance
//! batches and passes `&[]` for the program slice and `0.0` for the surface time
//! — the entry documents this in so many words: *"This entry takes explicit
//! batches, never a packet, so no batch names a surface program."* Windowing also
//! never calls `prepare_surfaces`. So an app that presents through windowing
//! renders **every authored surface as its constant fallback**, whatever it
//! authored, and no amount of app-side care changes that.
//!
//! The only public entry that carries surfaces to real pixels is
//! `GpuBackendApi::present_packet_with_surfaces`, and before this app it had
//! **no caller anywhere in the repository outside tests**. Driving it means
//! owning the canvas, the device handshake and the frame loop, which is what
//! this module does. That is a lot of app code for something an engine loop
//! should do, and that imbalance *is* the report: the live path exists, it works,
//! and nothing in the engine's own presentation stack walks it.
//!
//! ## The barrier, on a target where the device arrives late
//!
//! `crate::preparation` runs the barrier's **catalog** half natively, inside the
//! runtime's preparation phase: it plans, validates, flattens and emits WGSL for
//! every station before the simulation may step. The **device** half — binding
//! each generated program to a real pipeline — needs a `wgpu::Device`, which on
//! wasm exists only after an async `initialize` resolves. So it runs here,
//! immediately after `initialize` and strictly before the first frame is
//! recorded. The invariant that matters is intact: **nothing compiles inside a
//! frame.**

use axiom::prelude::*;
use axiom_gpu_backend::GpuBackendApi;
use axiom_host::FramePacket;
use wasm_bindgen::prelude::*;

use crate::diagnostics::{
    bars, readings, spark, static_readings, FrameHistory, FrameSample, Workload, FLUSH_MS,
};
use crate::frame::packet_of;
use crate::layout::{HEIGHT, WIDTH};
use crate::orbit::OrbitState;
use crate::pointer_input::{self, SharedOrbit};
use crate::preparation::presentation_request;
use crate::report::report;
use crate::scene::crucible_core;
use crate::stations::all_surfaces;

/// The id of the canvas the page provides.
const CANVAS_ID: &str = "shader-crucible-canvas";

/// The panel's root — the element whose `data-on` attribute the page's toggle
/// flips, and which the loop reads before paying for a DOM write.
const PANEL_ID: &str = "diag";

/// The element the page's console interceptor deposits the engine's own
/// `render backend = …` line into. See [`Panel::flush`].
const PANEL_BACKEND_ID: &str = "diag-backend";

/// **The page's diagnostic levers**, read once from the query string.
///
/// These exist because "the phone is slow" is not a finding, it is a symptom,
/// and the only way to turn one into the other is a controlled A/B: change one
/// input, hold the rest, and read the panel. Each lever changes *pixels* or
/// *programs* and nothing else, so a difference between two runs is attributable
/// to one of them.
///
/// * `?back=WxH` — pin the backbuffer to an explicit size. The fill-rate probe:
///   halve it and the fragment stage does a quarter of the work while every
///   draw, batch and command stays identical.
/// * `?surfaces=N` — present with only the first `N` authored surfaces. The
///   shader-cost probe: the frame draws the same geometry at the same
///   resolution, but the draws whose surface is missing take the constant
///   fallback instead of a generated program.
/// * `?adapt=0` — turn the adaptive render scale off, so a run holds one
///   resolution for its whole window.
/// * `?dpr=0` — pin the legacy fixed backbuffer instead of matching the device.
struct Levers {
    back: Option<(u32, u32)>,
    surfaces: Option<usize>,
    adapt: bool,
    device_pixels: bool,
}

impl Levers {
    /// The levers this page load asked for.
    fn from_query() -> Self {
        let query = web_sys::window()
            .map(|window| window.location())
            .and_then(|location| location.search().ok())
            .unwrap_or_default();
        let value = |key: &str| {
            query
                .trim_start_matches('?')
                .split('&')
                .filter_map(|pair| pair.split_once('='))
                .find(|(name, _)| *name == key)
                .map(|(_, value)| value.to_string())
        };
        Levers {
            back: value("back").and_then(|raw| {
                raw.split_once('x').and_then(|(w, h)| {
                    w.parse::<u32>().ok().zip(h.parse::<u32>().ok())
                })
            }),
            surfaces: value("surfaces").and_then(|raw| raw.parse().ok()),
            adapt: value("adapt").as_deref() != Some("0"),
            device_pixels: value("dpr").as_deref() != Some("0"),
        }
    }
}

/// **The backbuffer this page should render into.**
///
/// The canvas is laid out at `min(96vw, 1280px)` with a 2:1 aspect; the app used
/// to pin the backbuffer at the authored 1280x640 whatever that box turned out
/// to be. On a phone those two are not the same number and the mismatch is not
/// free: a 390 CSS-pixel viewport at device-pixel-ratio 3 is 1122 device pixels
/// wide, so a 1280-wide backbuffer shades **1.26 pixels for every pixel the
/// screen can show** and the compositor throws the surplus away.
///
/// So the default is the device's own pixels, clamped to the authored size — the
/// app never asks for *more* fragments than it did before, and on a phone it
/// asks for measurably fewer at identical sharpness. `?back=WxH` overrides it
/// for an A/B and `?dpr=0` restores the old fixed size.
fn backbuffer(levers: &Levers) -> (u32, u32) {
    levers.back.unwrap_or_else(|| {
        let dpr = web_sys::window()
            .as_ref()
            .map_or(1.0, web_sys::Window::device_pixel_ratio);
        let css = web_sys::window()
            .and_then(|window| window.document())
            .and_then(|document| document.get_element_by_id(CANVAS_ID))
            .map(|canvas| canvas.get_bounding_client_rect().width())
            .unwrap_or(0.0);
        let width = (css * dpr).round() as u32;
        // A 2:1 canvas, so the height follows the width; the floor keeps a
        // hidden or not-yet-laid-out canvas from asking for a zero-sized
        // surface, which `configure` rejects.
        let clamped = levers
            .device_pixels
            .then_some(width)
            .unwrap_or(WIDTH)
            .clamp(320, WIDTH);
        (clamped, clamped / 2)
    })
}

/// Author the scene, compile every station's program, bind the GPU, and drive
/// the frame loop.
pub fn start() {
    let (app, prepared) = crucible_core();
    log(&report(prepared.borrow().as_ref()));

    let levers = Levers::from_query();
    let (width, height) = backbuffer(&levers);
    let canvas = match canvas(width, height) {
        Some(canvas) => canvas,
        None => {
            error(&format!(
                "shader-crucible: no canvas with id `{CANVAS_ID}` on the page"
            ));
            return;
        }
    };

    let meshes = app.mesh_set();
    // The triangle count of every registered mesh, keyed by id — read once, here,
    // from the same set the backend uploads. The panel multiplies it by the
    // frame's draws instead of re-walking geometry per frame, because a
    // diagnostics panel that walks vertex data every frame is the very defect
    // this app went looking for.
    let mesh_triangles: std::collections::BTreeMap<u64, u64> = meshes
        .iter()
        .map(|(id, _, indices)| (*id, indices.len() as u64 / 3))
        .collect();
    let materials = app.material_textures();
    let look = axiom_host::FrameRenderLook::lit_by(app.ambient());
    // The whole authored set is always *prepared* — the barrier's numbers stay
    // the barrier's numbers. `?surfaces=N` narrows only what is handed to the
    // present, so the A/B changes which draws get a generated program and
    // nothing else about the frame.
    let surfaces = all_surfaces();
    let presented: Vec<Surface> = surfaces
        .iter()
        .take(levers.surfaces.unwrap_or(usize::MAX))
        .cloned()
        .collect();
    // One instance per body plus headroom; the crucible's scene is fixed, so
    // this is a constant rather than a growing allocation.
    let max_instances = 64;

    // The camera the page drives. `OrbitState::framed` seeds itself from the
    // scene's own authored eye/target, so the first frame is the shot this app
    // has always opened on; every later frame is whatever the user's gestures
    // have made of it. The listeners go on now — before the async device
    // handshake — so a drag during the wait is not silently dropped.
    let orbit: SharedOrbit = std::rc::Rc::new(std::cell::RefCell::new(OrbitState::framed()));
    pointer_input::install(CANVAS_ID, std::rc::Rc::clone(&orbit));

    log(&format!(
        "shader-crucible: backbuffer {width}x{height} (authored {WIDTH}x{HEIGHT}), \
         presenting {} of {} surfaces, adaptive scale {}",
        presented.len(),
        surfaces.len(),
        levers.adapt
    ));
    let mut backend = GpuBackendApi::new(&presentation_request(width, height));
    wasm_bindgen_futures::spawn_local(async move {
        match backend
            .initialize(canvas, &meshes, &[], &materials, max_instances, look, None)
            .await
        {
            Ok(()) => {}
            Err(err) => {
                web_sys::console::error_2(
                    &JsValue::from_str("shader-crucible: the GPU backend would not bind:"),
                    &err,
                );
                return;
            }
        }
        // **The device half of the barrier.** Every station's program is compiled
        // onto this device here, before a single frame is recorded.
        match backend.prepare_surfaces(&surfaces) {
            Ok(count) => log(&format!(
                "shader-crucible: the barrier bound {count} surface programs to the device"
            )),
            Err(_) => {
                error("shader-crucible: the surface set does not fit the program cache");
                return;
            }
        }
        drive(
            app,
            backend,
            presented,
            orbit,
            Panel::new(mesh_triangles),
            (width, height),
            levers.adapt,
        );
    });
}

/// The frame loop: one deterministic tick, one packet carrying every draw's
/// `surface_program` and the frame's engine time, one present.
///
/// ## The three spans, and why they are the right three
///
/// The loop below is timed at exactly the seams the app's frame *is*: the
/// simulation and scene walk (`app.render`), the packet translation
/// ([`packet_of`], which also billboards the captions), and the submission
/// (`present_packet_with_surfaces`, which batches, packs instances and records
/// the command buffer). There is no fourth thing this frame does, so the three
/// spans sum to the app's whole main-thread cost and any gap between that sum
/// and the frame interval belongs to something that is not this app's main
/// thread.
///
/// **`performance.now()` is a wall clock and never touches the simulation.** The
/// tick counter below is the only input `app.render` gets, and
/// [`crate::frame::time_at`] derives `EvalContext::time` from that tick alone.
/// A frame replayed at any wall-clock speed produces the same pixels; the
/// measurements are a side channel that the deterministic path cannot read.
/// ## The adaptive render scale, and why this app is the reason it exists
///
/// `axiom_host::RenderScaleController` is the engine's answer to "the GPU cannot
/// keep up": it is handed each frame's measured duration and returns a
/// resolution to render the next one at, dropping a rung after eight
/// consecutive over-budget frames and climbing back when there is headroom.
/// `axiom-windowing`'s loop wires it for every app that presents through
/// windowing. This app cannot present through windowing (see the module docs),
/// so it hand-rolled the loop — and hand-rolled it without the controller. On a
/// device with fragment headroom that omission is invisible; on one without it,
/// it is the difference between a frame that adapts and a frame that is simply
/// late, forever.
///
/// The duration fed in is the **frame gap**, not the main-thread time: the
/// controller defends a presentation interval, and the interval is what the
/// display and the GPU jointly decide. Feeding it the CPU span would tell it the
/// frame is comfortable at 2 ms while the page renders at 12 fps.
fn drive(
    mut app: RunningApp,
    mut backend: GpuBackendApi,
    surfaces: Vec<Surface>,
    orbit: SharedOrbit,
    mut panel: Panel,
    backbuffer: (u32, u32),
    adapt: bool,
) {
    let held: std::rc::Rc<std::cell::RefCell<Option<Closure<dyn FnMut()>>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));
    let scheduler = std::rc::Rc::clone(&held);
    let tick = std::cell::Cell::new(0_u64);
    let reported = std::cell::Cell::new(false);
    let mut scale = axiom_host::RenderScaleController::for_display();

    *held.borrow_mut() = Some(Closure::wrap(Box::new(move || {
        let now = tick.get();
        tick.set(now + 1);
        let entered = panel.now();
        // The previous frame's measured interval decides this frame's
        // resolution. Diagnostics-clock in, pixels out — nothing the simulation
        // reads is touched.
        adapt.then(|| {
            let observed = scale.observe((panel.last_gap_ms() * 1.0e6) as u64);
            backend.set_render_scale(observed);
        });
        // Re-author the camera before the tick that reads it. `set_camera`
        // reuses the existing camera node in place, so a moving camera costs no
        // allocation and leaks no scene nodes. Nothing else about the frame
        // changes: the same twelve bodies, the same surfaces, seen from
        // wherever the user's last gesture left the eye.
        orbit.borrow().apply(&mut app);
        let outcome = app.render(now);
        let rendered = panel.now();
        let packet = packet_of(&outcome, backbuffer.0, backbuffer.1);
        let packed = panel.now();
        let drew = backend.present_packet_with_surfaces(&packet, &surfaces);
        let presented = panel.now();
        // Report what the FIRST frame could not honour, once — a draw naming a
        // program the barrier did not prepare renders the constant fallback and
        // is reported here rather than silently looking wrong.
        (!reported.get()).then(|| {
            reported.set(true);
            let degraded = backend.frame_degradations(&packet);
            log(&format!(
                "shader-crucible: first frame drew={drew}, degraded={degraded:?}, \
                 programs={} for {} surfaces",
                backend.prepared_program_count(),
                backend.prepared_surface_count()
            ));
        });
        panel.record(
            [entered, rendered, packed, presented],
            &packet,
            &backend,
        );
        schedule(&scheduler);
    }) as Box<dyn FnMut()>));
    schedule(&held);
}

/// **The measuring half of the diagnostics panel.**
///
/// It holds the browser clock, the rolling window, and the handful of DOM
/// handles the flush writes through. The arithmetic and the markup live in
/// [`crate::diagnostics`], which names no browser API and is therefore driven by
/// native tests; this type is the part that can only exist on a page.
struct Panel {
    /// The page's high-resolution clock. `None` on a page with no
    /// `performance` — every span then reads zero and the panel says so by
    /// showing a zero CPU beside a real frame gap, rather than pretending.
    clock: Option<web_sys::Performance>,
    /// The window of measured frames.
    history: FrameHistory,
    /// When the previous frame's callback was entered, for the frame gap.
    last_entry: f64,
    /// When the DOM was last written.
    last_flush: f64,
    /// **The previous frame's panel cost.**
    ///
    /// A frame cannot record what its own bookkeeping cost until that
    /// bookkeeping has finished, so each sample carries the *previous* frame's
    /// panel cost. Over a rolling window the distribution is identical — it is
    /// the same measurements shifted by one frame — and the alternative is a
    /// second `push` per frame purely to correct an off-by-one that no
    /// percentile can see.
    carried_panel_ms: f64,
    /// Triangles per registered mesh id, read once at startup.
    mesh_triangles: std::collections::BTreeMap<u64, u64>,
    /// Whether the panel's fixed prose has been written yet.
    stated: bool,
    /// What each element currently displays, so a value that has not moved is
    /// not re-written into the DOM.
    shown: std::collections::BTreeMap<&'static str, String>,
}

impl Panel {
    /// A panel bound to the page's clock, holding no frames yet.
    fn new(mesh_triangles: std::collections::BTreeMap<u64, u64>) -> Self {
        Panel {
            clock: web_sys::window().and_then(|window| window.performance()),
            history: FrameHistory::new(),
            last_entry: 0.0,
            last_flush: 0.0,
            carried_panel_ms: 0.0,
            mesh_triangles,
            stated: false,
            shown: std::collections::BTreeMap::new(),
        }
    }

    /// The page clock, in milliseconds. **Diagnostics only** — nothing the
    /// simulation reads is derived from this.
    fn now(&self) -> f64 {
        self.clock.as_ref().map_or(0.0, web_sys::Performance::now)
    }

    /// The most recent frame's measured interval — what the adaptive render
    /// scale is fed. Zero before the first frame, which the controller reads as
    /// a comfortable frame and therefore never acts on.
    fn last_gap_ms(&self) -> f64 {
        self.history.newest().map_or(0.0, |sample| sample.gap_ms)
    }

    /// Record one frame's four timestamps and, at most five times a second,
    /// write the panel.
    fn record(&mut self, marks: [f64; 4], packet: &FramePacket, backend: &GpuBackendApi) {
        let [entered, rendered, packed, presented] = marks;
        // The first frame has no predecessor to measure a gap against; it
        // reports the frame's own main-thread time so the very first bar is a
        // real duration rather than a several-second startup interval that would
        // dominate the sparkline's scale for two seconds.
        let gap_ms = (self.last_entry > 0.0)
            .then(|| entered - self.last_entry)
            .unwrap_or(presented - entered);
        self.last_entry = entered;
        self.history.push(FrameSample {
            gap_ms,
            render_ms: rendered - entered,
            packet_ms: packed - rendered,
            present_ms: presented - packed,
            panel_ms: self.carried_panel_ms,
        });
        (presented - self.last_flush >= FLUSH_MS).then(|| {
            self.last_flush = presented;
            self.flush(packet, backend);
        });
        self.carried_panel_ms = self.now() - presented;
    }

    /// Write the panel into the page — unless the page has toggled it off, in
    /// which case the measuring continues and only the DOM write is skipped, so
    /// hiding the panel really does stop it costing anything.
    ///
    /// **Values, never markup.** The page's skeleton already holds an element
    /// for every reading; a flush sets text and two polygon attributes on
    /// elements that already exist. It rebuilt the body from an HTML string
    /// until an A/B of the page's own `requestAnimationFrame` cadence caught
    /// that costing a dropped frame per flush — the measurement is recorded in
    /// [`crate::diagnostics::Reading`]'s docs. A value whose text has not
    /// changed since the last flush is not written at all, so a still panel
    /// dirties nothing.
    fn flush(&mut self, packet: &FramePacket, backend: &GpuBackendApi) {
        let document = web_sys::window().and_then(|window| window.document());
        let on = document
            .as_ref()
            .and_then(|document| document.get_element_by_id(PANEL_ID))
            .and_then(|root| root.get_attribute("data-on"))
            .is_some_and(|value| value == "1");
        document
            .filter(|_| on)
            .map(|document| {
                let put = |id: &str, text: &str| {
                    document
                        .get_element_by_id(id)
                        .map(|element| element.set_text_content(Some(text)));
                };
                // The prose that never changes is written once and then left
                // alone; re-stating it five times a second would be five
                // pointless layout invalidations.
                (!self.stated).then(|| {
                    self.stated = true;
                    static_readings()
                        .iter()
                        .for_each(|(id, text)| put(id, text));
                });
                // Collected before anything is written, so the read of what is
                // currently shown is finished before the record of it is
                // updated — and so only the values that actually moved cost a
                // DOM write.
                let changed: Vec<(&'static str, String)> =
                    readings(&self.history, &self.workload(packet, backend))
                        .into_iter()
                        .filter(|(id, value)| self.shown.get(id) != Some(value))
                        .collect();
                changed.into_iter().for_each(|(id, value)| {
                    put(id, &value);
                    self.shown.insert(id, value);
                });
                bars(&self.history).iter().for_each(|(id, percent)| {
                    document.get_element_by_id(id).map(|element| {
                        element.set_attribute("style", &format!("width:{percent:.1}%"))
                    });
                });
                let (gap_area, main_area) = spark(&self.history);
                [("d-spark-gap", gap_area), ("d-spark-main", main_area)]
                    .iter()
                    .for_each(|(id, points)| {
                        document
                            .get_element_by_id(id)
                            .map(|element| element.set_attribute("points", points));
                    });
            });
    }

    /// What this frame asked the GPU to do, read off the packet the backend was
    /// just handed.
    fn workload(&self, packet: &FramePacket, backend: &GpuBackendApi) -> Workload {
        // Exactly the key `frame_packet_adapter::frame_packet_to_batches` sorts
        // on. Recomputing it here rather than asking the backend is app-side
        // arithmetic over the same packet, and it is why the count is labelled
        // derived; the backend has the real number and does not expose it.
        let batches: std::collections::BTreeSet<(u64, u64, u64)> = packet
            .draws()
            .iter()
            .map(|draw| (draw.surface_program(), draw.mesh_id(), draw.material_id()))
            .collect();
        let programs: std::collections::BTreeSet<u64> = packet
            .draws()
            .iter()
            .map(|draw| draw.surface_program())
            .filter(|program| *program != 0)
            .collect();
        let triangles = packet
            .draws()
            .iter()
            .map(|draw| {
                self.mesh_triangles
                    .get(&draw.mesh_id())
                    .copied()
                    .unwrap_or(0)
            })
            .sum();
        let window = web_sys::window();
        let dpr = window.as_ref().map_or(1.0, web_sys::Window::device_pixel_ratio);
        let document = window.as_ref().and_then(web_sys::Window::document);
        let css = document
            .as_ref()
            .and_then(|document| document.get_element_by_id(CANVAS_ID))
            .map(|canvas| {
                let rect = canvas.get_bounding_client_rect();
                (rect.width(), rect.height())
            })
            .unwrap_or((0.0, 0.0));
        let degradations = backend.frame_degradations(packet);
        Workload {
            draws: packet.draws().len(),
            batches: batches.len(),
            programs_used: programs.len(),
            triangles,
            lights: packet.lights().len(),
            backbuffer: (backend.width(), backend.height()),
            render_target: (backend.render_width(), backend.render_height()),
            css,
            dpr,
            prepared_programs: backend.prepared_program_count(),
            prepared_surfaces: backend.prepared_surface_count(),
            profile: [
                "restricted",
                "gpu/all",
            ][usize::from(
                backend.capability_profile() == axiom_host::BackendCapabilityProfile::all(),
            )]
            .to_string(),
            degraded: degradations
                .is_empty()
                .then(|| "none".to_string())
                .unwrap_or_else(|| format!("{degradations:?}")),
            // **The one fact this app cannot ask the engine for.** `wgpu` picks
            // WebGPU or falls back to WebGL2 inside
            // `live_gpu_binding.rs:302`, which *prints* the answer to the console
            // and keeps no accessor for it. The page intercepts that line and
            // parks it in an element; the panel reads it back from there. It is
            // still the engine's own report of what it bound — see the module
            // docs for the accessor that would make this indirection
            // unnecessary.
            backend: document
                .as_ref()
                .and_then(|document| document.get_element_by_id(PANEL_BACKEND_ID))
                .and_then(|element| element.text_content())
                .filter(|text| !text.is_empty())
                .unwrap_or_else(|| "unreported".to_string()),
        }
    }
}

/// Ask the browser for the next frame.
fn schedule(held: &std::rc::Rc<std::cell::RefCell<Option<Closure<dyn FnMut()>>>>) {
    held.borrow().as_ref().map(|closure| {
        web_sys::window().map(|window| {
            window.request_animation_frame(closure.as_ref().unchecked_ref())
        })
    });
}

/// The page's canvas, sized to the app's authoring resolution.
fn canvas(width: u32, height: u32) -> Option<web_sys::HtmlCanvasElement> {
    web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.get_element_by_id(CANVAS_ID))
        .and_then(|element| element.dyn_into::<web_sys::HtmlCanvasElement>().ok())
        .map(|canvas| {
            canvas.set_width(width);
            canvas.set_height(height);
            canvas
        })
}

/// One console line.
fn log(text: &str) {
    web_sys::console::log_1(&JsValue::from_str(text));
}

/// One console error.
fn error(text: &str) {
    web_sys::console::error_1(&JsValue::from_str(text));
}
