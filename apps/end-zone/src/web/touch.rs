//! In-match touch controls.
//!
//! There is **no virtual joystick**. The prototype's whole premise is that the
//! player does not steer — the play simulates itself and the player answers one
//! question — so a thumbstick sitting in the corner advertised a verb the game
//! does not have. Without it a touch carrier simply runs on his own AI intent,
//! which is what he does whenever the stick is centred anyway.
//!
//! What touch needs instead is the four answers. Rather than duplicating the
//! decision prompt as a second row of buttons, the prompt **is** the buttons:
//! a single delegated pointer listener on the HUD root reads `data-read` off
//! whichever chip was tapped. One piece of UI, always in the place the player is
//! already looking, and it can never disagree with the keyboard hints because it
//! is the same DOM.
//!
//! Pointer-event driven, so touch, pen, and mouse all work.

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{Element, PointerEvent};

use super::mount_div;

/// Shared touch-control state the DOM listeners write and the frame reads.
#[derive(Debug, Default)]
pub struct TouchHeld {
    /// The read `0..3` currently HELD under a finger, if any. Held rather than
    /// edge-triggered because a read is a wind-up: pressing starts charging and
    /// lifting throws.
    read_held: Option<usize>,
    scramble_edge: bool,
    pause_edge: bool,
}

/// One frame's touch reading (consumes the one-shot edges).
#[derive(Debug, Clone, Copy, Default)]
pub struct TouchFrame {
    pub read: Option<usize>,
    pub scramble: bool,
    pub pause: bool,
}

impl TouchHeld {
    pub fn take(&mut self) -> TouchFrame {
        TouchFrame {
            // The held read is NOT consumed — it persists until the finger
            // lifts, which is what makes the wind-up chargeable.
            read: self.read_held,
            scramble: core::mem::take(&mut self.scramble_edge),
            pause: core::mem::take(&mut self.pause_edge),
        }
    }
}

/// The only mounted touch button left: everything else is the live prompt.
const CONTROL_IDS: [&str; 1] = ["end-zone-btn-pause"];

/// Show/hide the mounted control cluster (menus hide it).
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

/// Mount the pause button and wire the decision prompt as the touch input.
pub fn mount_touch_controls(touch: &Rc<RefCell<TouchHeld>>) {
    let pause = mount_div(
        "end-zone-btn-pause",
        "position:fixed;right:18px;top:64px;z-index:50;min-width:92px;padding:14px 12px;\
         border-radius:14px;background:rgba(40,70,150,0.85);color:#fff;text-align:center;\
         font:700 14px/1 ui-monospace,Menlo,Consolas,monospace;letter-spacing:0.06em;\
         box-shadow:0 3px 10px rgba(0,0,0,0.45);touch-action:none;user-select:none;\
         -webkit-user-select:none;cursor:pointer;visibility:hidden;",
        Some("PAUSE"),
    );
    if let Some(pause) = pause {
        install_pause(&pause, touch);
    }
    install_decision_taps(touch);
}

/// Delegate taps on the decision prompt's chips.
///
/// Two reasons this is delegated rather than a listener per chip, and bound to
/// `window` rather than to the HUD root:
///
/// 1. The presenter rewrites the HUD's `innerHTML` whenever the read-out
///    changes, so a listener bound to a chip would be destroyed on the next
///    repaint — several times a second during a window.
/// 2. Controls are mounted before the presenter mounts the HUD root, so
///    binding to `#end-zone-hud` silently bound to nothing at all. `window`
///    always exists, which removes the ordering dependency entirely rather
///    than papering over it with a reordered boot sequence.
fn install_decision_taps(touch: &Rc<RefCell<TouchHeld>>) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let held = touch.clone();
    let on_down = Closure::<dyn FnMut(PointerEvent)>::new(move |e: PointerEvent| {
        let Some(target) = e.target().and_then(|t| t.dyn_into::<Element>().ok()) else {
            return;
        };
        // `closest` walks up from whatever child glyph took the hit (the big
        // number or the route name), so a tap anywhere on the chip counts.
        let read = target
            .closest("[data-read]")
            .ok()
            .flatten()
            .and_then(|el| el.get_attribute("data-read"))
            .and_then(|value| value.parse::<usize>().ok());
        let scramble = target
            .closest("[data-scramble]")
            .ok()
            .flatten()
            .is_some();
        if read.is_none() && !scramble {
            return;
        }
        e.prevent_default();
        e.stop_propagation();
        let mut state = held.borrow_mut();
        state.read_held = read.or(state.read_held);
        state.scramble_edge |= scramble;
    });
    let _ =
        window.add_event_listener_with_callback("pointerdown", on_down.as_ref().unchecked_ref());
    on_down.forget();

    // Lifting (or losing) the finger is the RELEASE that throws the ball, so
    // `pointercancel` is wired as well: a gesture the browser takes away from
    // us must let the wind-up go rather than leave it stuck charging.
    let up_held = touch.clone();
    let on_up = Closure::<dyn FnMut(PointerEvent)>::new(move |_e: PointerEvent| {
        up_held.borrow_mut().read_held = None;
    });
    let _ = window.add_event_listener_with_callback("pointerup", on_up.as_ref().unchecked_ref());
    let _ =
        window.add_event_listener_with_callback("pointercancel", on_up.as_ref().unchecked_ref());
    on_up.forget();
}

/// Wire the pause button: a pointer-down is one debounced edge.
fn install_pause(button: &Element, touch: &Rc<RefCell<TouchHeld>>) {
    let held = touch.clone();
    let on_down = Closure::<dyn FnMut(PointerEvent)>::new(move |e: PointerEvent| {
        e.prevent_default();
        e.stop_propagation();
        held.borrow_mut().pause_edge = true;
    });
    let _ =
        button.add_event_listener_with_callback("pointerdown", on_down.as_ref().unchecked_ref());
    on_down.forget();
}
