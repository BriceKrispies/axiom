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
use wasm_bindgen::prelude::*;

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

/// Author the scene, compile every station's program, bind the GPU, and drive
/// the frame loop.
pub fn start() {
    let (app, prepared) = crucible_core();
    log(&report(prepared.borrow().as_ref()));

    let canvas = match canvas() {
        Some(canvas) => canvas,
        None => {
            error(&format!(
                "shader-crucible: no canvas with id `{CANVAS_ID}` on the page"
            ));
            return;
        }
    };

    let meshes = app.mesh_set();
    let materials = app.material_textures();
    let look = axiom_host::FrameRenderLook::lit_by(app.ambient());
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

    let mut backend = GpuBackendApi::new(&presentation_request(WIDTH, HEIGHT));
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
        drive(app, backend, surfaces, orbit);
    });
}

/// The frame loop: one deterministic tick, one packet carrying every draw's
/// `surface_program` and the frame's engine time, one present.
fn drive(
    mut app: RunningApp,
    backend: GpuBackendApi,
    surfaces: Vec<Surface>,
    orbit: SharedOrbit,
) {
    let held: std::rc::Rc<std::cell::RefCell<Option<Closure<dyn FnMut()>>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));
    let scheduler = std::rc::Rc::clone(&held);
    let tick = std::cell::Cell::new(0_u64);
    let reported = std::cell::Cell::new(false);

    *held.borrow_mut() = Some(Closure::wrap(Box::new(move || {
        let now = tick.get();
        tick.set(now + 1);
        // Re-author the camera before the tick that reads it. `set_camera`
        // reuses the existing camera node in place, so a moving camera costs no
        // allocation and leaks no scene nodes. Nothing else about the frame
        // changes: the same twelve bodies, the same surfaces, seen from
        // wherever the user's last gesture left the eye.
        orbit.borrow().apply(&mut app);
        let outcome = app.render(now);
        let packet = packet_of(&outcome, WIDTH, HEIGHT);
        let drew = backend.present_packet_with_surfaces(&packet, &surfaces);
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
        schedule(&scheduler);
    }) as Box<dyn FnMut()>));
    schedule(&held);
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
fn canvas() -> Option<web_sys::HtmlCanvasElement> {
    web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.get_element_by_id(CANVAS_ID))
        .and_then(|element| element.dyn_into::<web_sys::HtmlCanvasElement>().ok())
        .map(|canvas| {
            canvas.set_width(WIDTH);
            canvas.set_height(HEIGHT);
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
