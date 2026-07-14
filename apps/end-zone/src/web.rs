//! The `wasm32` browser edge: decode keyboard events into neutral key tokens,
//! drive the windowing render loop (one fixed sim tick per animation frame),
//! and paint the diagnostic overlay. Everything nondeterministic lives here;
//! the sim only ever sees the sampled `DeviceFrame`.

use std::cell::RefCell;
use std::rc::Rc;

use axiom_debug_overlay::DebugOverlayApi;
use axiom_input::KeyToken;
use axiom_windowing::WindowingApi;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::KeyboardEvent;

use crate::app::{EndZoneApp, CANVAS_ID, HEIGHT, WIDTH};
use crate::config::EndZoneConfig;
use crate::scene::LIVE_CAPACITY;

/// The `KeyboardEvent.code` values the diagnostics consume.
const HANDLED: [&str; 8] = [
    "Space", "KeyR", "Digit1", "Digit2", "Digit3", "Digit4", "Digit5", "F1",
];

#[wasm_bindgen]
pub fn end_zone_start() {
    console_error_panic_hook::set_once();

    let held: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    install_key_listeners(&held);
    mount_title();

    let mut overlay = DebugOverlayApi::new();
    overlay.mount_to_body();
    // Open the overlay at boot so the diagnostic rows are visible (the same
    // public chord the Backquote key routes through).
    let _ = overlay.handle_backquote(false, false, false, false, false, false);

    let mut windowing = WindowingApi::new();
    if windowing.configure_surface(WIDTH, HEIGHT).is_err() {
        web_sys::console::error_1(&"end-zone: surface configuration failed".into());
        return;
    }

    let mut app = EndZoneApp::new(EndZoneConfig::default());
    let meshes = app.running().mesh_set();
    let materials = app.running().material_textures();

    let state = Rc::new(RefCell::new((app, overlay)));
    let frame_held = held.clone();
    let frame = move |_tick: u64| {
        let mut guard = state.borrow_mut();
        let (app, overlay) = &mut *guard;
        let keys: Vec<KeyToken> = frame_held
            .borrow()
            .iter()
            .map(|code| KeyToken::new(code))
            .collect();
        let outcome = app.frame(&keys);

        overlay.set_frame(app.frame_index(), app.frame_index(), 1, 60_000, 16_666);
        overlay.set_app_rows(&app.overlay_rows());

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
            outcome.camera_view_proj(),
            outcome.mesh_batch_casters(),
            outcome.sdf_scene().cloned(),
        )
    };

    let _ = windowing.run_web_multi(CANVAS_ID, meshes, materials, LIVE_CAPACITY, frame);
}

/// Track held `KeyboardEvent.code`s (the sim samples them per fixed tick).
fn install_key_listeners(held: &Rc<RefCell<Vec<String>>>) {
    let down = held.clone();
    let on_down = Closure::<dyn FnMut(KeyboardEvent)>::new(move |e: KeyboardEvent| {
        let code = e.code();
        if HANDLED.contains(&code.as_str()) {
            e.prevent_default();
            let mut set = down.borrow_mut();
            if !set.contains(&code) {
                set.push(code);
            }
        }
    });
    let up = held.clone();
    let on_up = Closure::<dyn FnMut(KeyboardEvent)>::new(move |e: KeyboardEvent| {
        let code = e.code();
        up.borrow_mut().retain(|held_code| held_code != &code);
    });
    if let Some(window) = web_sys::window() {
        let _ =
            window.add_event_listener_with_callback("keydown", on_down.as_ref().unchecked_ref());
        let _ = window.add_event_listener_with_callback("keyup", on_up.as_ref().unchecked_ref());
    }
    on_down.forget();
    on_up.forget();
}

/// The minimal original title + control hints (diagnostic label, not a menu).
fn mount_title() {
    let document = match web_sys::window().and_then(|w| w.document()) {
        Some(d) => d,
        None => return,
    };
    if document.get_element_by_id("end-zone-title").is_some() {
        return;
    }
    if let Ok(el) = document.create_element("div") {
        el.set_id("end-zone-title");
        let _ = el.set_attribute(
            "style",
            "position:fixed;top:10px;left:14px;z-index:20;color:#f3f5ef;\
             font:700 22px/1.25 ui-monospace,Menlo,Consolas,monospace;\
             text-shadow:0 2px 6px #000;pointer-events:none;",
        );
        el.set_text_content(Some(
            "END ZONE — systems showcase\nSpace start · R reset · 1-5 camera · F1 debug",
        ));
        if let Some(body) = document.body() {
            let _ = body.append_child(&el);
        }
    }
}
