//! In-match touch controls: the virtual joystick (bottom-left) and the
//! SNAP·THROW / PAUSE buttons (bottom-right), pointer-event driven so touch,
//! pen, and mouse all work. Menus never use these — they are shown only
//! while a match is live on a touch device.

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{Element, PointerEvent};

use super::mount_div;

/// Joystick geometry: knob travel radius in CSS pixels.
const STICK_RADIUS: f32 = 52.0;

/// Shared touch-control state the DOM listeners write and the frame reads.
#[derive(Debug, Default)]
pub struct TouchHeld {
    pub stick_x: f32,
    pub stick_y: f32,
    /// The pointer id currently driving the joystick, if any.
    stick_pointer: Option<i32>,
    pub primary_edge: bool,
    pub pause_edge: bool,
}

/// One frame's touch reading (consumes the one-shot button edges).
#[derive(Debug, Clone, Copy, Default)]
pub struct TouchFrame {
    pub stick_x: f32,
    pub stick_y: f32,
    pub primary: bool,
    pub pause: bool,
}

impl TouchHeld {
    pub fn take(&mut self) -> TouchFrame {
        TouchFrame {
            stick_x: self.stick_x,
            stick_y: self.stick_y,
            primary: core::mem::take(&mut self.primary_edge),
            pause: core::mem::take(&mut self.pause_edge),
        }
    }
}

const CONTROL_IDS: [&str; 4] = [
    "end-zone-stick",
    "end-zone-stick-knob",
    "end-zone-btn-primary",
    "end-zone-btn-pause",
];

/// Show/hide the whole control cluster (menus hide it).
pub fn set_controls_visible(visible: bool) {
    let Some(document) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    for id in CONTROL_IDS {
        if let Some(el) = document.get_element_by_id(id) {
            let _ = el
                .unchecked_ref::<web_sys::HtmlElement>()
                .style()
                .set_property("visibility", if visible { "visible" } else { "hidden" });
        }
    }
}

/// Mount the joystick and buttons (hidden until a match runs).
pub fn mount_touch_controls(touch: &Rc<RefCell<TouchHeld>>) {
    let base = mount_div(
        "end-zone-stick",
        "position:fixed;left:26px;bottom:26px;width:144px;height:144px;z-index:50;\
         border-radius:50%;background:rgba(20,32,24,0.45);border:2px solid rgba(255,255,255,0.35);\
         touch-action:none;user-select:none;-webkit-user-select:none;visibility:hidden;",
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
    let pause = mount_div(
        "end-zone-btn-pause",
        &button_style("right:26px;bottom:34px;", "rgba(40,70,150,0.85)"),
        Some("PAUSE"),
    );
    if let Some(pause) = pause {
        install_button(&pause, touch, false);
    }
}

fn button_style(anchor: &str, color: &str) -> String {
    format!(
        "position:fixed;{anchor}z-index:50;min-width:120px;padding:16px 14px;\
         border-radius:14px;background:{color};color:#fff;text-align:center;\
         font:700 15px/1 ui-monospace,Menlo,Consolas,monospace;letter-spacing:0.06em;\
         box-shadow:0 3px 10px rgba(0,0,0,0.45);touch-action:none;user-select:none;\
         -webkit-user-select:none;cursor:pointer;visibility:hidden;"
    )
}

fn knob_style(dx: f32, dy: f32) -> String {
    // `dy` is stick-up-positive and the element is BOTTOM-anchored (a larger
    // `bottom` is higher on screen), so it adds — a minus here mirrors the
    // knob against the finger.
    format!(
        "position:fixed;left:{}px;bottom:{}px;width:60px;height:60px;z-index:51;\
         border-radius:50%;background:rgba(240,244,238,0.85);pointer-events:none;\
         box-shadow:0 2px 8px rgba(0,0,0,0.5);visibility:hidden;",
        26.0 + 42.0 + dx,
        26.0 + 42.0 + dy,
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
                let style = knob_style(sx * STICK_RADIUS, sy * STICK_RADIUS)
                    .replace("visibility:hidden;", "visibility:visible;");
                let _ = knob.set_attribute("style", &style);
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
                    let style =
                        knob_style(0.0, 0.0).replace("visibility:hidden;", "visibility:visible;");
                    let _ = knob.set_attribute("style", &style);
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
        e.stop_propagation();
        let mut held = held.borrow_mut();
        if primary {
            held.primary_edge = true;
        } else {
            held.pause_edge = true;
        }
    });
    let _ =
        button.add_event_listener_with_callback("pointerdown", on_down.as_ref().unchecked_ref());
    on_down.forget();
}
