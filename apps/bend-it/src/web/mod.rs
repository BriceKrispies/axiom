//! The `wasm32` browser edge — the app's platform boundary, and nothing else.
//!
//! It measures the canvas, installs the pointer and key listeners, drives the
//! windowing render loop, and paints the overlay. Every decision about the
//! *game* it makes by asking something else: [`BendIt`] for the frame,
//! [`EditorView`] for what the screen should show.
//!
//! Nothing here is natively testable, which is exactly why nothing here is
//! allowed to be interesting.

pub mod overlay;
pub mod pointer;

use std::cell::RefCell;
use std::rc::Rc;

use axiom_debug_overlay::DebugOverlayApi;
use axiom_host::HostDeviceProfile;
use axiom_input::KeyToken;
use axiom_windowing::WindowingApi;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{Element, KeyboardEvent};

use crate::app::BendIt;
use crate::stroke::GameView;
use crate::scene::LIVE_CAPACITY;
use crate::CANVAS_ID;

use pointer::PointerCapture;

/// Keys whose browser default would fight the game.
const PREVENTED: [&str; 3] = ["Space", "Enter", "F1"];

/// Start the game.
#[wasm_bindgen]
pub fn bend_it_start() {
    console_error_panic_hook::set_once();

    let keys: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    install_key_listeners(&keys);

    let mut windowing = WindowingApi::new();
    // The surface is MEASURED, not declared. A phone's canvas is `100vw x 100vh`
    // at a device pixel ratio only the browser knows; handing the driver a
    // compile-time size configures a surface of the wrong shape and the browser
    // then stretches the frame into the element's box, squeezing the world by the
    // ratio between the two aspects. Nothing above the windowing module can see a
    // canvas, so asking the driver to measure the element it is about to present
    // into is the fix — and it is the same reading the camera then frames from.
    if windowing
        .configure_surface_from_canvas(CANVAS_ID, HostDeviceProfile::Baseline)
        .is_err()
    {
        web_sys::console::error_1(&"bend-it: the canvas is not laid out yet".into());
        return;
    }
    let (surface_w, surface_h) = (
        windowing.surface_width().unwrap_or(720),
        windowing.surface_height().unwrap_or(1280),
    );

    let capture = PointerCapture::install(CANVAS_ID);
    let mut overlay_panel = DebugOverlayApi::new();
    overlay_panel.mount_to_body();

    let game = Rc::new(RefCell::new(BendIt::new(surface_w, surface_h)));
    {
        // Hand the driver the render look the scene authored, so the live backend
        // binds with the same daylight the off-screen capture uses.
        let mut guard = game.borrow_mut();
        let running = guard.running();
        let (ambient, grade) = (running.ambient(), running.postprocess());
        windowing.set_ambient(ambient);
        grade.into_iter().for_each(|g| windowing.set_grade(g));
    }

    let meshes = game.borrow_mut().running().mesh_set();
    let materials = game.borrow_mut().running().material_textures();

    let frame_game = game.clone();
    let frame_keys = keys.clone();
    let frame = move |_tick: u64| {
        let mut guard = frame_game.borrow_mut();
        // The surface can change under us — an orientation change, a resized
        // window — so the size is read every frame rather than latched.
        let (w, h) = surface_size(surface_w as f32, surface_h as f32);
        guard.resize(w, h);

        let tokens: Vec<KeyToken> = frame_keys
            .borrow()
            .iter()
            .map(|code| KeyToken::new(code))
            .collect();
        let contacts = capture
            .as_ref()
            .map(|c| c.samples())
            .unwrap_or_default();
        guard.advance(&tokens, &contacts);
        let view: GameView = guard.view().clone();
        overlay::paint(&view);
        overlay_panel.set_app_rows(&guard.overlay_rows());
        overlay_panel.set_frame(guard.frame_index(), guard.frame_index(), 1, 60_000, 16_666);

        let outcome = guard.present();
        let lights = outcome
            .lights()
            .iter()
            .map(|l| (l.kind(), l.vec(), l.color(), l.intensity()))
            .collect();
        (
            outcome.clear_color(),
            lights,
            outcome.light_view_proj(),
            outcome.mesh_batches(),
            axiom_host::FrameCamera::new(
                outcome.camera_view(),
                outcome.camera_projection(),
                outcome.camera_view_proj(),
            ),
            outcome.mesh_batch_casters(),
            outcome.sdf_scene().cloned(),
        )
    };

    let _ = windowing.run_web_multi(CANVAS_ID, meshes, materials, LIVE_CAPACITY, frame);
}

/// The canvas' physical pixel size right now.
fn surface_size(fallback_w: f32, fallback_h: f32) -> (f32, f32) {
    let measured = web_sys::window().and_then(|window| {
        let scale = window.device_pixel_ratio() as f32;
        window
            .document()?
            .get_element_by_id(CANVAS_ID)
            .map(|canvas| {
                let rect = canvas.get_bounding_client_rect();
                (rect.width() as f32 * scale, rect.height() as f32 * scale)
            })
    });
    measured
        .filter(|(w, h)| (*w >= 1.0) & (*h >= 1.0))
        .unwrap_or((fallback_w, fallback_h))
}

/// Track every held `KeyboardEvent.code`.
fn install_key_listeners(keys: &Rc<RefCell<Vec<String>>>) {
    let down = keys.clone();
    let on_down = Closure::<dyn FnMut(KeyboardEvent)>::new(move |e: KeyboardEvent| {
        let code = e.code();
        PREVENTED.contains(&code.as_str()).then(|| e.prevent_default());
        let mut held = down.borrow_mut();
        (!held.contains(&code)).then(|| held.push(code.clone()));
    });
    let up = keys.clone();
    let on_up = Closure::<dyn FnMut(KeyboardEvent)>::new(move |e: KeyboardEvent| {
        let code = e.code();
        up.borrow_mut().retain(|held| held != &code);
    });
    // A tab switch drops every key, so nothing is stuck down on return.
    let blur = keys.clone();
    let on_blur = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
        blur.borrow_mut().clear();
    });
    if let Some(window) = web_sys::window() {
        let _ = window.add_event_listener_with_callback("keydown", on_down.as_ref().unchecked_ref());
        let _ = window.add_event_listener_with_callback("keyup", on_up.as_ref().unchecked_ref());
        let _ = window.add_event_listener_with_callback("blur", on_blur.as_ref().unchecked_ref());
    }
    on_down.forget();
    on_up.forget();
    on_blur.forget();
}

/// Create (once) one absolutely-positioned overlay element.
pub(crate) fn mount_div(id: &str, style: &str) -> Option<Element> {
    let document = web_sys::window()?.document()?;
    match document.get_element_by_id(id) {
        Some(existing) => Some(existing),
        None => {
            let element = document.create_element("div").ok()?;
            element.set_id(id);
            let _ = element.set_attribute("style", style);
            document.body()?.append_child(&element).ok()?;
            Some(element)
        }
    }
}
