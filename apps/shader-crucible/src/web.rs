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
//! **2. This app needs a diagnostic loop, not a game loop.** The panel below
//! times the frame at three named seams, drives its own redraw gate, and pulls
//! levers (shadows, render scale, adaptive scale, solo) that only an instrument
//! wants. Owning the loop is what buys all of that.
//!
//! ## The finding this app was written to surface, and its resolution
//!
//! Reason 2 used to be reason *one*: **`axiom-windowing`'s loop could not carry a
//! surface at all.** Its GPU arm called `GpuBackendApi::present_frame_result`,
//! which passed `&[]` for the program slice and `0.0` for the surface time, and
//! windowing never called `prepare_surfaces` — so an app presenting through
//! `App::run` rendered **every authored surface as its constant fallback**,
//! whatever it authored, and no amount of app-side care changed that. The only
//! entry that carried surfaces to real pixels was
//! `GpuBackendApi::present_packet_with_surfaces`, and it had no caller anywhere
//! in the repository outside this app and tests.
//!
//! That is fixed in the engine rather than worked around here.
//! `present_frame_result` now takes the program slice and a kernel `Seconds`;
//! `WindowingApi::set_surfaces` / `set_material_programs` hand the driver the
//! authored set and the `(material, program)` table it resolves each frame's
//! batches through; the driver compiles the set onto the device it binds; and
//! `App::surfaces` puts that compilation inside the engine's own
//! `axiom_runtime::PreparationTask`, before `RuntimeState::Prepared`. An app on
//! the normal loop now gets its authored materials.
//!
//! This module is therefore no longer a *report*; it is an instrument that keeps
//! its own loop for reason 2, and it stays the reference implementation of the
//! packet-carrying path.
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
    bars, readings, spark, static_readings, FrameHistory, FrameSample, LoopState, Workload,
    FLUSH_MS,
};
use crate::export::diagnostics_json;
use crate::frame::packet_of_plan;
use crate::layout::{HEIGHT, WIDTH};
use crate::levers::{solo_camera, Levers};
use crate::orbit::OrbitState;
use crate::pointer_input::{self, SharedOrbit};
use crate::preparation::presentation_request;
use crate::redraw::{Redraw, RedrawGate};
use crate::report::report;
use crate::scene::{crucible_core, scene_camera};
use crate::stations::all_surfaces;

/// The id of the canvas the page provides.
const CANVAS_ID: &str = "shader-crucible-canvas";

/// The panel's root — the element whose `data-on` attribute the page's toggle
/// flips, and which the loop reads before paying for a DOM write.
const PANEL_ID: &str = "diag";

// **The live lever state**, shared between the page's buttons and the frame
// loop.
//
// The buttons under the canvas call the `crucible_*` exports at the bottom of
// this file, which write here; the frame loop reads it once per frame and
// applies whatever changed. A `Cell` rather than a `RefCell` because `Levers`
// is `Copy` and a frame must never be able to observe a half-written
// configuration.
//
// `PANEL` is the live panel, so the export button reads the same window the
// page is showing rather than a copy taken at some earlier flush.
//
// The seed is the query string, so a link still carries a configuration — see
// `Levers::from_query`.
thread_local! {
    static LEVERS: std::cell::Cell<Levers> = const { std::cell::Cell::new(Levers::SHIPPING) };

    static PANEL: std::cell::RefCell<Option<std::rc::Rc<std::cell::RefCell<Panel>>>> =
        const { std::cell::RefCell::new(None) };

    // The render scale the loop last handed the backend. Held here because the
    // backend will not tell it back: `render_width()` is the device *tier's*
    // size, decided at initialisation, and the live binding's own `render_size`
    // is not on the facade. So the panel prints what was asked for, labelled as
    // that, and the GPU pass times are what say whether it took.
    static RENDER_SCALE: std::cell::Cell<f32> = const { std::cell::Cell::new(1.0) };
}

/// Hand `scale` to the backend and remember what was asked for.
fn apply_scale(backend: &mut GpuBackendApi, scale: axiom_host::RenderScale) {
    backend.set_render_scale(scale);
    RENDER_SCALE.with(|cell| cell.set(scale.ratio().get()));
}

/// The levers the page currently has pulled.
fn levers() -> Levers {
    LEVERS.with(std::cell::Cell::get)
}

/// Move the levers, and hand the page back the new state to draw its buttons
/// from.
fn set_levers(next: Levers) -> String {
    LEVERS.with(|cell| cell.set(next));
    next.state_json()
}

/// **The backend capability profile a shadow setting implies.**
///
/// Clearing `RenderCapability::Shadows` zeroes the `CAP_SHADOWS` bit the
/// fragment stage tests before it uses the PCF result. See [`crate::levers`] for
/// what this does and does not remove — notably it does *not* remove the 25
/// `textureSampleCompare` taps, because the shader selects on the result and
/// `select` evaluates both arms.
fn profile_for(shadows: bool) -> axiom_host::BackendCapabilityProfile {
    let all = axiom_host::BackendCapabilityProfile::all();
    shadows
        .then_some(all)
        .unwrap_or_else(|| all.without(axiom_host::RenderCapability::Shadows))
}

/// **The render-scale ladder's floor** — 0.50 linear, a quarter of the
/// fragments.
///
/// Obtained by driving a throwaway `RenderScaleController` down its ladder,
/// because `RenderScale` exposes no constructor but `FULL` and the ladder's
/// rungs are private. `observe` is a pure function of the durations it is shown,
/// so feeding it a run of hopeless frames is arithmetic, not a simulation of
/// one — but it is a workaround, and the engine change that would retire it is
/// `RenderScale::of(Ratio)` (or a named `HALF`). This is called once, at
/// startup, and the result is held.
fn floor_scale() -> axiom_host::RenderScale {
    let mut controller = axiom_host::RenderScaleController::for_display();
    (0..4096).for_each(|_| {
        controller.observe(1_000_000_000);
    });
    controller.scale()
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

    let query = web_sys::window()
        .map(|window| window.location())
        .and_then(|location| location.search().ok())
        .unwrap_or_default();
    let levers = Levers::from_query(&query);
    LEVERS.with(|cell| cell.set(levers));
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
        levers.presented_surfaces(),
        surfaces.len(),
        levers.adaptive
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
        let panel = std::rc::Rc::new(std::cell::RefCell::new(Panel::new(mesh_triangles)));
        PANEL.with(|slot| slot.borrow_mut().replace(std::rc::Rc::clone(&panel)));
        drive(app, backend, surfaces, orbit, panel, (width, height));
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
/// ## The adaptive render scale is wired, and deliberately OFF by default
///
/// `axiom_host::RenderScaleController` is the engine's answer to "the GPU cannot
/// keep up": handed each frame's measured duration, it returns a resolution to
/// render the next one at, dropping a rung after eight consecutive over-budget
/// frames and climbing back when there is headroom. `axiom-windowing`'s loop
/// wires it for every app that presents through windowing; this app cannot
/// present through windowing (see the module docs — it wants an instrument's
/// loop, not a game's), so it hand-rolls the loop.
///
/// **It defaults to off here because this app is a diagnostic instrument, and on
/// an instrument an adaptive resolution is a lie.** Its whole job is to make the
/// cost of a procedural surface visible; a controller that quietly renders fewer
/// pixels until the frame fits converts a measurable per-pixel cost into a
/// number that looks fine and says nothing. It masks precisely the signal the
/// panel exists to show — and it did: with it on, a heavy layered material read
/// as a comfortable frame at a resolution nobody had asked for.
///
/// A shipping game should turn it on. `?adapt=1` does, so the two behaviours can
/// be compared side by side, which is itself a thing worth being able to see.
///
/// The duration fed in is the **frame gap**, not the main-thread time: the
/// controller defends a presentation interval, and the interval is what the
/// display and the GPU jointly decide. Feeding it the CPU span would tell it the
/// frame is comfortable at 2 ms while the page renders at 12 fps.
/// ## The levers, applied on change and never per frame
///
/// The loop reads the shared lever state once a frame and pushes only what
/// moved. That is not a micro-optimisation: `set_render_scale` reallocates the
/// scene colour target, its depth buffer and the bloom chain, and
/// `set_capability_profile` re-derives the shader's capability word — paying
/// either every frame would put the instrument's own cost inside the
/// measurement, which is the one thing this panel may not do.
/// ## The frame is not drawn unless it would look different
///
/// This loop used to call `app.render` unconditionally on every callback, which on
/// a still camera looking at a static station is the whole frame — scene walk,
/// packet, submission and the entire fragment bill — paid sixty times a second to
/// put an identical image on the screen. [`crate::redraw`] holds the rule and the
/// derivation; [`RedrawGate::decide`] is asked once, before anything is built, and
/// its answer is the only thing standing between a callback and a frame.
///
/// Two consequences are worth stating here, where the loop is:
///
/// * **The `requestAnimationFrame` chain is never broken.** A skipped callback
///   still schedules the next one, so a gesture, a lever or a station's clock is
///   noticed on the very next frame with no wake-up path to get wrong. What a
///   skipped callback costs is a handful of comparisons; what it saves is
///   everything else.
/// * **The tick counter advances only on a frame that is drawn.** Engine time is
///   still `tick / 60` and tick *N* still produces exactly the pixels it always
///   did — see [`crate::frame::time_at`] — but the ticks this page reaches are now
///   always the contiguous prefix `0..=N`, which is what makes "replay this run"
///   mean something. `crate::redraw`'s module docs argue the alternative.
fn drive(
    mut app: RunningApp,
    mut backend: GpuBackendApi,
    surfaces: Vec<Surface>,
    orbit: SharedOrbit,
    panel: std::rc::Rc<std::cell::RefCell<Panel>>,
    backbuffer: (u32, u32),
) {
    let held: std::rc::Rc<std::cell::RefCell<Option<Closure<dyn FnMut()>>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));
    let scheduler = std::rc::Rc::clone(&held);
    let tick = std::cell::Cell::new(0_u64);
    let reported = std::cell::Cell::new(false);
    let mut scale = axiom_host::RenderScaleController::for_display();
    let floor = floor_scale();
    // The gate reads each authored surface's own requirements once, here, to learn
    // which of them bind the frame clock — so "station 5 animates" is a fact the
    // app derives from the graphs rather than a body number written down twice.
    let mut gate = RedrawGate::new(&surfaces);
    // What the backend has actually been told, so a lever that has not moved
    // costs nothing. Seeded from the page's opening configuration, which
    // `start` has already applied by construction.
    let mut applied = levers();
    backend.set_capability_profile(profile_for(applied.shadows));
    applied
        .half_res
        .then(|| apply_scale(&mut backend, floor));

    *held.borrow_mut() = Some(Closure::wrap(Box::new(move || {
        let mut panel = panel.borrow_mut();
        let entered = panel.now();
        let current = levers();
        // Only what moved. Each of these is a reallocation or a re-derivation;
        // neither belongs in a steady frame. (The `surfaces` lever needs nothing
        // here: it cuts the packet's own `surface_program` lane, which
        // `packet_of_plan` does per frame for free. See
        // `PacketPlan::program_of` for why narrowing the presented slice — what
        // this app did before — measured nothing at all.)
        (current.shadows != applied.shadows).then(|| {
            backend.set_capability_profile(profile_for(current.shadows));
        });
        ((current.half_res, current.adaptive) != (applied.half_res, applied.adaptive)).then(|| {
            apply_scale(
                &mut backend,
                current
                    .half_res
                    .then_some(floor)
                    .unwrap_or(axiom_host::RenderScale::FULL),
            );
        });
        applied = current;
        // The previous frame's measured interval decides this frame's
        // resolution. Diagnostics-clock in, pixels out — nothing the simulation
        // reads is touched. The adaptive controller owns the scale while it is
        // on, so the half-res lever is inert alongside it and the panel's
        // `render target` row shows which of the two is speaking. Turning it on
        // also holds the loop open (see `Redraw::Held`): it is a closed loop over
        // the measured frame interval, and an idle page has no interval to feed
        // it.
        current.adaptive.then(|| {
            let observed = scale.observe((panel.last_gap_ms() * 1.0e6) as u64);
            apply_scale(&mut backend, observed);
        });
        // **The framing this callback would produce**, resolved before anything
        // is built — because it is half of the question "would this frame look
        // different?". A solo'd station uses the fixed framing every solo shares,
        // so two solo readings differ only in which shader is on the pixels;
        // otherwise it is wherever the user's last gesture left the eye.
        let camera = current
            .solo
            .map(solo_camera)
            .unwrap_or_else(|| orbit.borrow().camera_transform());
        let redraw = gate.decide(camera, current);
        // **The frame, or the decision not to build one.** Everything below the
        // `if` is the cost this app used to pay unconditionally.
        if redraw.draws() {
            let now = tick.get();
            tick.set(now + 1);
            // Re-author the camera before the tick that reads it. `set_camera`
            // reuses the existing camera node in place, so a moving camera costs
            // no allocation and leaks no scene nodes. Nothing else about the
            // frame changes: the same twelve bodies, the same surfaces.
            app.set_camera(scene_camera(), camera);
            let outcome = app.render(now);
            let rendered = panel.now();
            let packet = packet_of_plan(&outcome, backbuffer.0, backbuffer.1, current.plan());
            let packed = panel.now();
            let drew = backend.present_packet_with_surfaces(&packet, &surfaces);
            let presented_at = panel.now();
            // Report what the FIRST frame could not honour, once — a draw naming
            // a program the barrier did not prepare renders the constant fallback
            // and is reported here rather than silently looking wrong.
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
                [entered, rendered, packed, presented_at],
                &packet,
                &backend,
                redraw,
            );
        } else {
            panel.skip(entered, redraw);
        }
        // Dropped before the next frame is asked for, so nothing holds the
        // panel's borrow across a callback the export button could land in.
        drop(panel);
        // **Always.** The chain is what notices the next gesture; a skipped
        // callback is a few comparisons, and stopping it would mean owning a
        // wake-up path for the pointer listeners, the wheel, every lever button
        // and the page's own visibility — four chances to leave the app frozen
        // in exchange for microseconds.
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
    /// **When the previous frame's callback was entered — if that callback drew.**
    ///
    /// `None` before the first frame and after every skipped one, which is the
    /// same condition and deliberately so: a frame gap is a *cadence*, and a
    /// cadence exists only between two consecutive drawn frames. The interval
    /// across an idle is however long the user sat still, and averaging that into
    /// a frame-time distribution would report a resting page as a stalling one.
    /// So a frame that follows a skip is presented and counted and simply
    /// contributes no sample — which is also what the first frame, whose
    /// predecessor is the several-second device handshake, has always needed.
    last_entry: Option<f64>,
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
    /// **The workload of the most recent flushed frame.**
    ///
    /// Held rather than recomputed because the export button has no packet and
    /// no backend of its own, and because a reading taken from the same frame
    /// the page is showing is the only one that can be compared with it. It is
    /// refreshed on the flush cadence — five times a second — whether or not the
    /// panel is visible, so hiding the panel does not blind the export.
    last_workload: Workload,
    /// Whether the panel's fixed prose has been written yet.
    stated: bool,
    /// What each element currently displays, so a value that has not moved is
    /// not re-written into the DOM.
    shown: std::collections::BTreeMap<&'static str, String>,
    /// **What the frame loop is doing**, and how old the readings beside it are.
    /// The panel's answer to "has this needle stopped, or has it broken?".
    loop_state: LoopState,
    /// The page clock when the last frame was actually submitted, for the age of
    /// the image on the screen.
    last_present: f64,
}

impl Panel {
    /// A panel bound to the page's clock, holding no frames yet.
    fn new(mesh_triangles: std::collections::BTreeMap<u64, u64>) -> Self {
        Panel {
            clock: web_sys::window().and_then(|window| window.performance()),
            history: FrameHistory::new(),
            last_entry: None,
            last_flush: 0.0,
            carried_panel_ms: 0.0,
            mesh_triangles,
            last_workload: Workload::default(),
            stated: false,
            shown: std::collections::BTreeMap::new(),
            loop_state: LoopState::default(),
            last_present: 0.0,
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

    /// Record one drawn frame's four timestamps and, at most five times a second,
    /// write the panel.
    ///
    /// A frame with no drawn predecessor — the first one, or the first after an
    /// idle — is counted and flushed but contributes **no sample**; see
    /// [`Panel::last_entry`] for why a gap across an idle is not a cadence.
    fn record(
        &mut self,
        marks: [f64; 4],
        packet: &FramePacket,
        backend: &GpuBackendApi,
        redraw: Redraw,
    ) {
        let [entered, rendered, packed, presented] = marks;
        let spans = FrameSample {
            gap_ms: 0.0,
            render_ms: rendered - entered,
            packet_ms: packed - rendered,
            present_ms: presented - packed,
            panel_ms: self.carried_panel_ms,
        };
        self.last_entry.map(|previous| {
            self.history.push(FrameSample {
                gap_ms: entered - previous,
                ..spans
            });
        });
        self.last_entry = Some(entered);
        self.last_present = presented;
        self.loop_state = LoopState {
            reason: redraw.reason(),
            drawing: true,
            held_ms: 0.0,
            drawn: self.loop_state.drawn + 1,
            skipped: self.loop_state.skipped,
        };
        (presented - self.last_flush >= FLUSH_MS).then(|| {
            self.last_flush = presented;
            self.last_workload = self.workload(packet, backend);
            self.flush();
        });
        self.carried_panel_ms = self.now() - presented;
    }

    /// **A callback that drew nothing**, because nothing that decides a pixel had
    /// moved.
    ///
    /// It records no sample — there is no frame to measure — and it keeps
    /// [`Workload`] exactly as it was, because the workload describes the image
    /// that is *on the screen* and that image has not changed. What it does do is
    /// keep flushing on the panel's own 5 Hz cadence, so the loop row and the age
    /// of the readings stay current while everything above them stands still.
    /// A panel that froze along with the frames could not tell the reader the
    /// difference between an idle app and a hung one.
    fn skip(&mut self, entered: f64, redraw: Redraw) {
        self.last_entry = None;
        self.loop_state = LoopState {
            reason: redraw.reason(),
            drawing: false,
            held_ms: entered - self.last_present,
            drawn: self.loop_state.drawn,
            skipped: self.loop_state.skipped + 1,
        };
        (entered - self.last_flush >= FLUSH_MS).then(|| {
            self.last_flush = entered;
            self.flush();
        });
    }

    /// The window the panel is showing — what the export reports.
    fn history(&self) -> &FrameHistory {
        &self.history
    }

    /// The workload of the most recently flushed frame.
    fn workload_snapshot(&self) -> &Workload {
        &self.last_workload
    }

    /// What the frame loop is doing, and how old the window above is.
    fn loop_snapshot(&self) -> &LoopState {
        &self.loop_state
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
    fn flush(&mut self) {
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
                    readings(&self.history, &self.last_workload, &self.loop_state)
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
        // **The backend's own counts.** This app used to re-derive them by
        // rebuilding `frame_packet_adapter`'s sort key from the packet by hand,
        // which was correct only for as long as nobody changed the grouping.
        // `packet_batch_counts` is the adapter answering for itself.
        let (batches, pipelines) = backend.packet_batch_counts(packet);
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
        // **The per-pass GPU time, when the adapter can give one.** Never this
        // frame's — resolving a timestamp query set completes on a later task —
        // so the reading carries the frame it belongs to.
        let timing = backend.gpu_pass_timing();
        let gpu_passes: Vec<(String, f64)> = timing
            .passes()
            .into_iter()
            .map(|(name, seconds)| (name.to_string(), f64::from(seconds.get()) * 1000.0))
            .collect();
        Workload {
            draws: packet.draws().len(),
            batches: batches as usize,
            programs_used: pipelines as usize,
            triangles,
            lights: packet.lights().len(),
            backbuffer: (backend.width(), backend.height()),
            render_target: (backend.render_width(), backend.render_height()),
            render_scale: f64::from(RENDER_SCALE.with(std::cell::Cell::get)),
            css,
            dpr,
            prepared_programs: backend.prepared_program_count(),
            prepared_surfaces: backend.prepared_surface_count(),
            // Named rather than merely "restricted": the shadows lever is the
            // one thing in this app that clears a capability bit, and a reading
            // that says only "restricted" cannot be told apart from a device
            // whose backend dropped something on its own.
            profile: {
                let profile = backend.capability_profile();
                (profile == axiom_host::BackendCapabilityProfile::all())
                    .then(|| "gpu/all".to_string())
                    .unwrap_or_else(|| format!("gpu/no-shadows (0x{:x})", profile.bits()))
            },
            degraded: degradations
                .is_empty()
                .then(|| "none".to_string())
                .unwrap_or_else(|| format!("{degradations:?}")),
            // **Which graphics API `wgpu` actually bound.** This was the one
            // fact the app could not ask the engine for: the binding printed it
            // to the console and kept no accessor, so the page wrapped
            // `console.log` and scraped the line back out of it.
            // `GpuBackendApi::bound_backend()` now reports it, and the wrapper
            // is gone from `web/index.html`.
            backend: backend
                .bound_backend()
                .map(|kind| format!("{kind:?}"))
                .unwrap_or_else(|| "unbound".to_string()),
            gpu_available: timing.is_available(),
            gpu_reason: timing.unavailable_reason().to_string(),
            gpu_total_ms: f64::from(timing.total().get()) * 1000.0,
            gpu_frame: timing.frame().raw(),
            gpu_passes,
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

// ---------------------------------------------------------------------------
// The buttons under the canvas.
//
// Every lever is a function here, and the page's job is only to call one and
// paint the state it hands back. That split is deliberate: the *meaning* of a
// lever — what it removes, whether it needs a reload, what the button should say
// — is app logic and lives in `crate::levers` where native tests can reach it.
// The page owns markup and a click handler and nothing else, so the two can
// never disagree about what a button does.
//
// Each of these returns `Levers::state_json()`, so a click is one call and the
// page never has to ask a second time.
// ---------------------------------------------------------------------------

/// The current lever state, for the page's first paint.
#[wasm_bindgen]
pub fn crucible_levers() -> String {
    levers().state_json()
}

/// **Return every runtime lever to the shipping configuration.**
///
/// The one button the user asked for by name. The two backbuffer levers cannot
/// move without a reload, so `reload_required` in the returned state tells the
/// page whether to reload itself as well — it does, so nobody ever has to edit
/// a URL to get back to normal.
#[wasm_bindgen]
pub fn crucible_reset() -> String {
    set_levers(Levers {
        // The backbuffer levers are held: this call cannot change them, and
        // reporting them as reset when they are not would be a lie the page
        // would then act on.
        device_pixels: levers().device_pixels,
        back: levers().back,
        ..Levers::SHIPPING
    })
}

/// Draw the twelve caption meshes, or not — 12 of the frame's 25 draws.
#[wasm_bindgen]
pub fn crucible_toggle_captions() -> String {
    let current = levers();
    set_levers(Levers {
        captions: !current.captions,
        ..current
    })
}

/// Carry the scene's light projection, or the identity — see [`crate::levers`].
#[wasm_bindgen]
pub fn crucible_toggle_shadows() -> String {
    let current = levers();
    set_levers(Levers {
        shadows: !current.shadows,
        ..current
    })
}

/// Step the `surfaces` lever to its next stop: all, none, 3, 6.
#[wasm_bindgen]
pub fn crucible_cycle_surfaces() -> String {
    set_levers(levers().cycled_surfaces())
}

/// Step the solo control through `ALL, body 1 .. body 12`.
#[wasm_bindgen]
pub fn crucible_step_solo(delta: i32) -> String {
    set_levers(levers().stepped_solo(delta))
}

/// Render at the scale ladder's floor — a quarter of the fragments, every draw
/// and every command identical.
#[wasm_bindgen]
pub fn crucible_toggle_half_res() -> String {
    let current = levers();
    set_levers(Levers {
        half_res: !current.half_res,
        ..current
    })
}

/// **Hold the frame loop open**, redrawing the identical frame every callback.
///
/// The one lever that adds work. The app idles when nothing that decides a pixel
/// has moved (see [`crate::redraw`]), which is right — and which also means a still
/// camera on a static station gives the panel no frames to measure. This is how a
/// steady-state reading is taken on that configuration. It changes no pixel and no
/// draw; only how often the same frame is submitted.
#[wasm_bindgen]
pub fn crucible_toggle_force() -> String {
    let current = levers();
    set_levers(Levers {
        force: !current.force,
        ..current
    })
}

/// Hand the resolution to the adaptive controller, or take it back.
#[wasm_bindgen]
pub fn crucible_toggle_adaptive() -> String {
    let current = levers();
    set_levers(Levers {
        adaptive: !current.adaptive,
        ..current
    })
}

/// **The whole reading, as one JSON object** — see [`crate::export`].
///
/// Returned to the page, which puts it on the clipboard *and* prints it to the
/// console, because the clipboard needs a permission a phone may not grant and a
/// reading that cannot leave the device is not a reading anybody can act on.
///
/// Before the first flush there is no workload to report; the object still comes
/// back, carrying the lever state and an empty window, rather than nothing.
#[wasm_bindgen]
pub fn crucible_diagnostics() -> String {
    let captured = web_sys::window()
        .and_then(|window| window.performance())
        .map_or(0.0, |clock| clock.now());
    PANEL
        .with(|slot| slot.borrow().clone())
        .map(|panel| {
            let panel = panel.borrow();
            diagnostics_json(
                panel.history(),
                panel.workload_snapshot(),
                panel.loop_snapshot(),
                &levers(),
                captured,
            )
        })
        .unwrap_or_else(|| {
            diagnostics_json(
                &FrameHistory::new(),
                &Workload::default(),
                &LoopState::default(),
                &levers(),
                captured,
            )
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
