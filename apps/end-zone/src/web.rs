//! The `wasm32` browser edge: decode keyboard + touch input into the neutral
//! shapes the app samples ([`KeyToken`]s and a [`TouchInput`]), mount the
//! virtual joystick + action buttons for mobile, drive the windowing render
//! loop (one fixed sim tick per animation frame), and paint the diagnostic
//! overlay. Everything nondeterministic lives here.

use std::cell::RefCell;
use std::rc::Rc;

use axiom_debug_overlay::DebugOverlayApi;
use axiom_input::KeyToken;
use axiom_windowing::WindowingApi;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{Element, KeyboardEvent, PointerEvent};

use crate::app::{EndZoneApp, TouchInput, CANVAS_ID, HEIGHT, WIDTH};
use crate::config::EndZoneConfig;
use crate::scene::LIVE_CAPACITY;

/// The `KeyboardEvent.code` values the app consumes (prevented from
/// scrolling/other browser defaults).
const HANDLED: [&str; 17] = [
    "Space",
    "KeyR",
    "Digit1",
    "Digit2",
    "Digit3",
    "Digit4",
    "Digit5",
    "F1",
    "Enter",
    "KeyW",
    "KeyA",
    "KeyS",
    "KeyD",
    "ArrowUp",
    "ArrowDown",
    "ArrowLeft",
    "ArrowRight",
];

/// Shared touch-control state the DOM listeners write and the frame reads.
#[derive(Debug, Default)]
struct TouchHeld {
    stick_x: f32,
    stick_y: f32,
    /// The pointer id currently driving the joystick, if any.
    stick_pointer: Option<i32>,
    primary_edge: bool,
    reset_edge: bool,
}

impl TouchHeld {
    /// This frame's [`TouchInput`], consuming the one-shot button edges.
    fn take(&mut self) -> TouchInput {
        TouchInput {
            stick_x: self.stick_x,
            stick_y: self.stick_y,
            primary: core::mem::take(&mut self.primary_edge),
            reset: core::mem::take(&mut self.reset_edge),
        }
    }
}

#[wasm_bindgen]
pub fn end_zone_start() {
    console_error_panic_hook::set_once();

    let keys: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    install_key_listeners(&keys);
    let touch: Rc<RefCell<TouchHeld>> = Rc::new(RefCell::new(TouchHeld::default()));
    mount_title();
    mount_touch_controls(&touch);

    let mut overlay = DebugOverlayApi::new();
    overlay.mount_to_body();
    // Open the overlay at boot on keyboard machines; on a touch device it
    // would cover the playfield, so it stays behind the Backquote toggle.
    if !is_touch_device() {
        let _ = overlay.handle_backquote(false, false, false, false, false, false);
    }

    let mut windowing = WindowingApi::new();
    if windowing.configure_surface(WIDTH, HEIGHT).is_err() {
        web_sys::console::error_1(&"end-zone: surface configuration failed".into());
        return;
    }

    let mut app = EndZoneApp::new(EndZoneConfig::default());
    let meshes = app.running().mesh_set();
    let materials = app.running().material_textures();

    let state = Rc::new(RefCell::new((app, overlay)));
    let frame_keys = keys.clone();
    let frame_touch = touch.clone();
    let frame = move |_tick: u64| {
        let mut guard = state.borrow_mut();
        let (app, overlay) = &mut *guard;
        let key_tokens: Vec<KeyToken> = frame_keys
            .borrow()
            .iter()
            .map(|code| KeyToken::new(code))
            .collect();
        let touch_input = frame_touch.borrow_mut().take();
        let outcome = app.frame(&key_tokens, touch_input);

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

fn is_touch_device() -> bool {
    web_sys::window()
        .map(|w| w.navigator().max_touch_points() > 0)
        .unwrap_or(false)
}

/// Track held `KeyboardEvent.code`s (the sim samples them per fixed tick).
fn install_key_listeners(keys: &Rc<RefCell<Vec<String>>>) {
    let down = keys.clone();
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
    let up = keys.clone();
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

/// Create one absolutely-positioned DOM element with an inline style.
fn mount_div(id: &str, style: &str, text: Option<&str>) -> Option<Element> {
    let document = web_sys::window()?.document()?;
    if let Some(existing) = document.get_element_by_id(id) {
        return Some(existing);
    }
    let el = document.create_element("div").ok()?;
    el.set_id(id);
    let _ = el.set_attribute("style", style);
    if let Some(text) = text {
        el.set_text_content(Some(text));
    }
    document.body()?.append_child(&el).ok()?;
    Some(el)
}

/// Joystick geometry: knob travel radius in CSS pixels.
const STICK_RADIUS: f32 = 52.0;

/// Mount the virtual joystick (bottom-left) and the two action buttons
/// (bottom-right), all pointer-event driven so they work with touch, pen,
/// and mouse alike.
fn mount_touch_controls(touch: &Rc<RefCell<TouchHeld>>) {
    let base = mount_div(
        "end-zone-stick",
        "position:fixed;left:26px;bottom:26px;width:144px;height:144px;z-index:30;\
         border-radius:50%;background:rgba(20,32,24,0.45);border:2px solid rgba(255,255,255,0.35);\
         touch-action:none;user-select:none;-webkit-user-select:none;",
        None,
    );
    let knob = mount_div("end-zone-stick-knob", &knob_style(0.0, 0.0), None);
    if let (Some(base), Some(_knob)) = (base, knob) {
        install_stick(&base, touch);
    }

    let primary = mount_div(
        "end-zone-btn-primary",
        &button_style("right:26px;bottom:110px;", "rgba(190,60,40,0.85)"),
        Some("SNAP · THROW"),
    );
    if let Some(primary) = primary {
        install_button(&primary, touch, true);
    }
    let reset = mount_div(
        "end-zone-btn-reset",
        &button_style("right:26px;bottom:34px;", "rgba(40,70,150,0.85)"),
        Some("RESET"),
    );
    if let Some(reset) = reset {
        install_button(&reset, touch, false);
    }
}

fn button_style(anchor: &str, color: &str) -> String {
    format!(
        "position:fixed;{anchor}z-index:30;min-width:120px;padding:16px 14px;\
         border-radius:14px;background:{color};color:#fff;text-align:center;\
         font:700 15px/1 ui-monospace,Menlo,Consolas,monospace;letter-spacing:0.06em;\
         box-shadow:0 3px 10px rgba(0,0,0,0.45);touch-action:none;user-select:none;\
         -webkit-user-select:none;cursor:pointer;"
    )
}

fn knob_style(dx: f32, dy: f32) -> String {
    format!(
        "position:fixed;left:{}px;bottom:{}px;width:60px;height:60px;z-index:31;\
         border-radius:50%;background:rgba(240,244,238,0.85);pointer-events:none;\
         box-shadow:0 2px 8px rgba(0,0,0,0.5);",
        26.0 + 42.0 + dx,
        26.0 + 42.0 - dy,
    )
}

/// Wire the joystick: pointer down captures, movement maps to a clamped
/// `[-1,1]²` vector about the pad center (screen up = downfield), up releases.
fn install_stick(base: &Element, touch: &Rc<RefCell<TouchHeld>>) {
    let apply = |touch: &Rc<RefCell<TouchHeld>>, base: &Element, e: &PointerEvent| {
        let rect = base.get_bounding_client_rect();
        let center_x = rect.left() as f32 + rect.width() as f32 / 2.0;
        let center_y = rect.top() as f32 + rect.height() as f32 / 2.0;
        let dx = (e.client_x() as f32 - center_x) / STICK_RADIUS;
        let dy = (center_y - e.client_y() as f32) / STICK_RADIUS; // screen up = +y
        let len = (dx * dx + dy * dy).sqrt();
        let scale = if len > 1.0 { 1.0 / len } else { 1.0 };
        let (sx, sy) = (dx * scale, dy * scale);
        {
            let mut held = touch.borrow_mut();
            held.stick_x = sx;
            held.stick_y = sy;
        }
        if let Some(document) = web_sys::window().and_then(|w| w.document()) {
            if let Some(knob) = document.get_element_by_id("end-zone-stick-knob") {
                let _ =
                    knob.set_attribute("style", &knob_style(sx * STICK_RADIUS, sy * STICK_RADIUS));
            }
        }
    };

    let down_touch = touch.clone();
    let down_base = base.clone();
    let on_down = Closure::<dyn FnMut(PointerEvent)>::new(move |e: PointerEvent| {
        e.prevent_default();
        down_touch.borrow_mut().stick_pointer = Some(e.pointer_id());
        apply(&down_touch, &down_base, &e);
    });
    let move_touch = touch.clone();
    let move_base = base.clone();
    let on_move = Closure::<dyn FnMut(PointerEvent)>::new(move |e: PointerEvent| {
        let driving = move_touch.borrow().stick_pointer == Some(e.pointer_id());
        if driving {
            e.prevent_default();
            apply(&move_touch, &move_base, &e);
        }
    });
    let up_touch = touch.clone();
    let on_up = Closure::<dyn FnMut(PointerEvent)>::new(move |e: PointerEvent| {
        let mut held = up_touch.borrow_mut();
        if held.stick_pointer == Some(e.pointer_id()) {
            held.stick_pointer = None;
            held.stick_x = 0.0;
            held.stick_y = 0.0;
            drop(held);
            if let Some(document) = web_sys::window().and_then(|w| w.document()) {
                if let Some(knob) = document.get_element_by_id("end-zone-stick-knob") {
                    let _ = knob.set_attribute("style", &knob_style(0.0, 0.0));
                }
            }
        }
    });

    let _ = base.add_event_listener_with_callback("pointerdown", on_down.as_ref().unchecked_ref());
    if let Some(window) = web_sys::window() {
        let _ = window
            .add_event_listener_with_callback("pointermove", on_move.as_ref().unchecked_ref());
        let _ =
            window.add_event_listener_with_callback("pointerup", on_up.as_ref().unchecked_ref());
        let _ = window
            .add_event_listener_with_callback("pointercancel", on_up.as_ref().unchecked_ref());
    }
    on_down.forget();
    on_move.forget();
    on_up.forget();
}

/// Wire one action button: a pointer-down is one debounced edge.
fn install_button(button: &Element, touch: &Rc<RefCell<TouchHeld>>, primary: bool) {
    let held = touch.clone();
    let on_down = Closure::<dyn FnMut(PointerEvent)>::new(move |e: PointerEvent| {
        e.prevent_default();
        let mut held = held.borrow_mut();
        if primary {
            held.primary_edge = true;
        } else {
            held.reset_edge = true;
        }
    });
    let _ =
        button.add_event_listener_with_callback("pointerdown", on_down.as_ref().unchecked_ref());
    on_down.forget();
}

/// The minimal original title + control hints (diagnostic label, not a menu).
fn mount_title() {
    let hint = if is_touch_device() {
        "END ZONE\nstick: move your player · SNAP/THROW · RESET"
    } else {
        "END ZONE — systems showcase\nWASD move · Enter snap/throw · Space start · R reset · 1-5 camera · F1 debug"
    };
    if let Some(el) = mount_div(
        "end-zone-title",
        "position:fixed;top:10px;left:14px;z-index:20;color:#f3f5ef;\
         font:700 18px/1.3 ui-monospace,Menlo,Consolas,monospace;\
         text-shadow:0 2px 6px #000;pointer-events:none;white-space:pre-line;",
        None,
    ) {
        el.set_text_content(Some(hint));
    }
}
