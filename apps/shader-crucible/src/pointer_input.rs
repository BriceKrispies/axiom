//! The browser half of the orbit camera: DOM pointer gestures → calls on
//! [`OrbitState`]. wasm32 only, and deliberately **pure plumbing** — every
//! sensitivity, clamp and sign lives in `src/orbit.rs`, which is native-testable.
//! Nothing here decides what a gesture means; it only measures one.
//!
//! # Why Pointer Events
//!
//! `pointerdown`/`pointermove`/`pointerup`/`pointercancel` are the one browser
//! API that reports mouse, touch and pen contacts in a single shape, so there is
//! exactly one code path for "a finger", "a mouse" and "a stylus" instead of a
//! touch path that drifts out of sync with the mouse path. Pointers are tracked
//! in a `BTreeMap` keyed by `pointerId`, which also makes the two-contact pinch
//! read the same pair in the same order every move.
//!
//! # Why both `preventDefault()` and `touch-action: none`
//!
//! They stop different things and neither is sufficient alone. `touch-action:
//! none` tells the compositor up front that the canvas wants raw pointer
//! streams, which is what suppresses scroll/pinch-zoom takeover *before* the
//! gesture is recognised — but iOS Safari still applies its own double-tap and
//! rubber-band behaviours, and a wheel over the canvas still scrolls the page.
//! `preventDefault()` on the events themselves closes those. The CSS is applied
//! here as well as in the page stylesheet, so the behaviour travels with the
//! code that depends on it.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::{AddEventListenerOptions, Event, HtmlCanvasElement, PointerEvent, WheelEvent};

use crate::orbit::OrbitState;

/// The orbit state shared between the DOM listeners and the per-frame closure.
pub type SharedOrbit = Rc<RefCell<OrbitState>>;

/// `WheelEvent::delta_mode` values, and how many CSS pixels each unit is worth.
/// Chrome reports pixels, Firefox reports lines; a page-mode wheel is rare but
/// real. Normalising here keeps the zoom curve in `orbit.rs` in one unit.
const WHEEL_PIXELS_PER_LINE: f32 = 16.0;
const WHEEL_PIXELS_PER_PAGE: f32 = 400.0;

/// The live gesture: which pointers are down, where each was last seen (client
/// CSS pixels), and whether the gesture that started it asked for pan.
#[derive(Debug, Default)]
struct Gesture {
    points: BTreeMap<i32, (f32, f32)>,
    /// Set on the pointerdown that opened the gesture: a right button or a held
    /// shift means "pan" rather than "orbit". Touch never sets it — two fingers
    /// are how a phone pans.
    pan_modifier: bool,
}

/// Install every listener the orbit camera needs on the canvas with `canvas_id`,
/// driving `orbit`. Does nothing if the page has no such canvas.
///
/// The closures are `forget()`-ed: they must outlive this call for the lifetime
/// of the page, and the page's lifetime *is* the app's lifetime here — the run
/// loop never returns, so there is no later moment at which tearing them down
/// would mean anything.
pub fn install(canvas_id: &str, orbit: SharedOrbit) {
    let Some(canvas) = canvas_element(canvas_id) else {
        return;
    };
    // Belt and braces with the page stylesheet; see the module docs.
    let _ = canvas.style().set_property("touch-action", "none");

    let gesture: Rc<RefCell<Gesture>> = Rc::new(RefCell::new(Gesture::default()));

    on_pointer(&canvas, "pointerdown", {
        let canvas = canvas.clone();
        let gesture = gesture.clone();
        move |event: PointerEvent| {
            event.prevent_default();
            // Keep receiving moves after the drag leaves the canvas box.
            let _ = canvas.set_pointer_capture(event.pointer_id());
            let mut gesture = gesture.borrow_mut();
            let opening = gesture.points.is_empty();
            gesture
                .points
                .insert(event.pointer_id(), client_position(&event));
            if opening {
                gesture.pan_modifier = event.button() == 2 || event.shift_key();
            }
        }
    });

    on_pointer(&canvas, "pointermove", {
        let canvas = canvas.clone();
        let gesture = gesture.clone();
        let orbit = orbit.clone();
        move |event: PointerEvent| {
            let mut gesture = gesture.borrow_mut();
            if !gesture.points.contains_key(&event.pointer_id()) {
                return;
            }
            event.prevent_default();
            // Drags are measured in canvas heights, so the same swipe means the
            // same rotation on a phone and on a 1280px desktop canvas.
            let scale = 1.0 / css_height(&canvas);
            let before: Vec<(f32, f32)> = gesture.points.values().copied().collect();
            let now = client_position(&event);
            gesture.points.insert(event.pointer_id(), now);
            let after: Vec<(f32, f32)> = gesture.points.values().copied().collect();
            let mut orbit = orbit.borrow_mut();
            match after.as_slice() {
                // One contact: orbit, or pan under a right button / held shift.
                [_] => {
                    let (dx, dy) = (
                        (now.0 - before[0].0) * scale,
                        (now.1 - before[0].1) * scale,
                    );
                    if gesture.pan_modifier {
                        orbit.pan(dx, dy);
                    } else {
                        orbit.orbit(dx, dy);
                    }
                }
                // Two contacts: the change in their separation is a zoom and the
                // travel of their midpoint is a pan — both, from the one move,
                // because a real pinch is never purely one or the other.
                [q0, q1] => {
                    let (p0, p1) = (before[0], before[1]);
                    let (was, is) = (separation(p0, p1), separation(*q0, *q1));
                    if was > 1.0 && is > 1.0 {
                        orbit.zoom_by(was / is);
                    }
                    let (mx, my) = (midpoint(p0, p1), midpoint(*q0, *q1));
                    orbit.pan((my.0 - mx.0) * scale, (my.1 - mx.1) * scale);
                }
                _ => {}
            }
        }
    });

    for name in ["pointerup", "pointercancel"] {
        on_pointer(&canvas, name, {
            let canvas = canvas.clone();
            let gesture = gesture.clone();
            move |event: PointerEvent| {
                let _ = canvas.release_pointer_capture(event.pointer_id());
                let mut gesture = gesture.borrow_mut();
                gesture.points.remove(&event.pointer_id());
                if gesture.points.is_empty() {
                    gesture.pan_modifier = false;
                }
            }
        });
    }

    // The wheel listener must be explicitly non-passive: a passive listener's
    // `preventDefault()` is ignored and the page scrolls out from under the
    // zoom.
    let wheel = Closure::wrap(Box::new({
        let orbit = orbit.clone();
        move |event: WheelEvent| {
            event.prevent_default();
            orbit.borrow_mut().zoom_by_wheel(wheel_pixels(&event));
        }
    }) as Box<dyn FnMut(WheelEvent)>);
    let options = AddEventListenerOptions::new();
    options.set_passive(false);
    let _ = canvas.add_event_listener_with_callback_and_add_event_listener_options(
        "wheel",
        wheel.as_ref().unchecked_ref(),
        &options,
    );
    wheel.forget();

    // Right-drag is a pan, so the context menu must not open on top of it.
    let menu =
        Closure::wrap(Box::new(|event: Event| event.prevent_default()) as Box<dyn FnMut(Event)>);
    let _ = canvas.add_event_listener_with_callback("contextmenu", menu.as_ref().unchecked_ref());
    menu.forget();
}

/// The canvas element with `id`, if the page has one.
fn canvas_element(id: &str) -> Option<HtmlCanvasElement> {
    web_sys::window()?
        .document()?
        .get_element_by_id(id)?
        .dyn_into::<HtmlCanvasElement>()
        .ok()
}

/// Register one `PointerEvent` listener and leak its closure for the page's life.
fn on_pointer(canvas: &HtmlCanvasElement, name: &str, handler: impl FnMut(PointerEvent) + 'static) {
    let closure = Closure::wrap(Box::new(handler) as Box<dyn FnMut(PointerEvent)>);
    let _ = canvas.add_event_listener_with_callback(name, closure.as_ref().unchecked_ref());
    closure.forget();
}

/// A pointer's position in client CSS pixels. Deltas are all this code needs, so
/// there is no reason to convert into the canvas's own space.
fn client_position(event: &PointerEvent) -> (f32, f32) {
    (event.client_x() as f32, event.client_y() as f32)
}

/// The canvas's laid-out CSS height, floored at 1 so it is always a legal
/// divisor.
fn css_height(canvas: &HtmlCanvasElement) -> f32 {
    canvas.get_bounding_client_rect().height().max(1.0) as f32
}

/// The distance between two contacts, in CSS pixels.
fn separation(a: (f32, f32), b: (f32, f32)) -> f32 {
    ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt()
}

/// The point halfway between two contacts.
fn midpoint(a: (f32, f32), b: (f32, f32)) -> (f32, f32) {
    ((a.0 + b.0) * 0.5, (a.1 + b.1) * 0.5)
}

/// A wheel event's vertical travel in CSS pixels, whatever unit it arrived in.
fn wheel_pixels(event: &WheelEvent) -> f32 {
    let per_unit = match event.delta_mode() {
        WheelEvent::DOM_DELTA_LINE => WHEEL_PIXELS_PER_LINE,
        WheelEvent::DOM_DELTA_PAGE => WHEEL_PIXELS_PER_PAGE,
        _ => 1.0,
    };
    event.delta_y() as f32 * per_unit
}
