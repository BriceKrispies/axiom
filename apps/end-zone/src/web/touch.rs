//! In-match touch controls.
//!
//! There is **no virtual joystick**: the running back runs by himself, so there
//! is nothing to steer. What the player needs is four decisions made fast, and
//! on a phone they get two ways to make each one — **flick anywhere**, or **tap
//! the chip**.
//!
//! Both, and not one or the other. The flick is faster and keeps a thumb off
//! the field; the chip is discoverable and can never be mis-recognised as a
//! diagonal. An earlier version of this file shipped only the flick, and the
//! move row was drawn as four button-shaped things that were in fact labels —
//! which is the worst of both, because a phone player taps what looks like a
//! button and concludes the game is broken. Anything drawn as a button IS a
//! button.
//!
//! What lives here is **only the reading of pointer events**. Whether a drag is
//! a deliberate swipe, which axis it took, and which of the four moves that
//! means is decided by [`crate::controls::swipe`], on the deterministic side of
//! the platform boundary, where a native test can drive it. This file knows
//! about `PointerEvent` and nothing about gameplay.
//!
//! The pre-snap play card keeps its taps: the three plays are a menu, and a menu
//! is a thing you touch. One delegated listener reads `data-play` off whichever
//! row was tapped, so the prompt the player is already looking at *is* the
//! button and can never disagree with the keyboard hints — it is the same DOM.

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{Element, PointerEvent};

use crate::controls::swipe::{SwipePhase, SwipeRecognizer, SwipeSample};
use crate::runback::RunbackMove;

use super::mount_div;

/// Shared touch-control state the DOM listeners write and the frame reads.
#[derive(Debug, Default)]
pub struct TouchHeld {
    /// A play row TAPPED since the last frame, if any. A one-shot edge: the
    /// pointer-down IS the whole input, exactly like the keyboard's press.
    play_edge: Option<usize>,
    /// A move committed since the last frame — a tapped chip or a recognised
    /// swipe. One field, because they are one verb.
    move_edge: Option<RunbackMove>,
    pause_edge: bool,
    /// The gesture recogniser, fed by every pointer sample.
    recognizer: SwipeRecognizer,
}

/// One frame's touch reading (consumes the one-shot edges).
#[derive(Debug, Clone, Copy, Default)]
pub struct TouchFrame {
    pub play: Option<usize>,
    pub wanted: Option<RunbackMove>,
    pub pause: bool,
}

impl TouchHeld {
    pub fn take(&mut self) -> TouchFrame {
        TouchFrame {
            play: core::mem::take(&mut self.play_edge),
            wanted: core::mem::take(&mut self.move_edge),
            pause: core::mem::take(&mut self.pause_edge),
        }
    }
}

/// The only mounted touch button: everything else is a tap on the live HUD or a
/// flick anywhere on the field.
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

/// Mount the pause button, the play-card taps, and the swipe surface.
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
    install_pointer(touch);
}

/// One delegated pointer listener for the whole window: it feeds the swipe
/// recogniser and, on a tap that landed on a play row or a move chip, records
/// that instead.
///
/// Delegated rather than a listener per element, and bound to `window` rather
/// than to the HUD root, for two reasons that have both bitten this file before:
///
/// 1. the presenter rewrites the HUD's `innerHTML` whenever the read-out
///    changes, so a listener bound to a row would be destroyed on the next
///    repaint;
/// 2. controls are mounted before the presenter mounts the HUD root, so binding
///    to `#end-zone-hud` silently bound to nothing at all.
///
/// It also means the **whole screen** is the swipe surface, which is what you
/// want on a phone: there is no correct place to put your thumb, so every place
/// is correct.
fn install_pointer(touch: &Rc<RefCell<TouchHeld>>) {
    let Some(window) = web_sys::window() else {
        return;
    };
    for (event, phase) in [
        ("pointerdown", SwipePhase::Down),
        ("pointermove", SwipePhase::Move),
        ("pointerup", SwipePhase::Up),
        ("pointercancel", SwipePhase::Up),
    ] {
        let held = touch.clone();
        let on_event = Closure::<dyn FnMut(PointerEvent)>::new(move |e: PointerEvent| {
            let mut state = held.borrow_mut();
            let down = phase == SwipePhase::Down;
            // A tap on a chip is a press, not the start of a gesture — so it
            // short-circuits the recogniser rather than also dragging it.
            // First press of the frame wins: two chips hit at once is a fumbled
            // input, and honouring the later one would let a stray thumb
            // overwrite a deliberate one.
            if let Some(play) = down.then(|| attribute(&e, "data-play")).flatten() {
                e.prevent_default();
                state.play_edge = state.play_edge.or(Some(play));
                state.recognizer.clear();
                return;
            }
            if let Some(index) = down.then(|| attribute(&e, "data-move")).flatten() {
                e.prevent_default();
                state.move_edge = state.move_edge.or(move_of(index));
                state.recognizer.clear();
                return;
            }
            let recognised = state.recognizer.sample(SwipeSample {
                phase,
                x: e.client_x() as f32,
                y: e.client_y() as f32,
            });
            if let Some(wanted) = recognised {
                e.prevent_default();
                state.move_edge = state.move_edge.or(Some(wanted));
            }
        });
        let _ = window.add_event_listener_with_callback(event, on_event.as_ref().unchecked_ref());
        on_event.forget();
    }
}

/// The index carried by the nearest ancestor of the event's target that has
/// `selector` as an attribute, if any.
///
/// `closest` walks up from whatever child glyph actually took the hit (the big
/// arrow, the key letter, the move name), so a tap anywhere inside a chip
/// counts — which on a phone is the difference between a control that works and
/// one that works only if you hit the 4-pixel gap between two words.
fn attribute(e: &PointerEvent, selector: &str) -> Option<usize> {
    let target = e.target().and_then(|t| t.dyn_into::<Element>().ok())?;
    target
        .closest(&format!("[{selector}]"))
        .ok()
        .flatten()
        .and_then(|el| el.get_attribute(selector))
        .and_then(|value| value.parse::<usize>().ok())
}

/// The move a chip index means. The order is the row's order, which is also
/// [`crate::presentation::hud`]'s — one list, authored once.
fn move_of(index: usize) -> Option<RunbackMove> {
    [
        RunbackMove::JukeLeft,
        RunbackMove::JukeRight,
        RunbackMove::Shoulder,
        RunbackMove::Jump,
    ]
    .get(index)
    .copied()
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
