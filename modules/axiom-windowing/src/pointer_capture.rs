//! Unified pointer capture (mouse + touch + pen) for the browser run loop.
//!
//! The platform-edge half of mobile-first input: it installs PointerEvent
//! listeners on the canvas — the *one* browser API that reports mouse, touch,
//! and pen contacts in a single shape — and maintains the set of currently-down
//! pointers. Each frame the run loop reads [`PointerCapture::samples`] (neutral
//! `(x, y, down)` triples in physical surface pixels) and hands them to the
//! deterministic synthesizer in the `axiom-input` module, which turns them into
//! a movement vector and look deltas. This module owns only the DOM plumbing; it
//! makes no control decisions. wasm32 only — the live platform arm, so ordinary
//! control flow is fine here.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::{HtmlCanvasElement, PointerEvent};

/// The live set of active (down) pointers, keyed by the browser pointer id. A
/// `BTreeMap` keeps iteration order stable (deterministic sample order) across
/// frames regardless of touch arrival order.
type Pointers = Rc<RefCell<BTreeMap<i32, (f32, f32)>>>;

/// An installed pointer capture: holds the shared pointer set and keeps the
/// event-listener closures alive for as long as the capture is in scope.
pub struct PointerCapture {
    pointers: Pointers,
    // Kept alive so the listeners stay registered; never read directly.
    _closures: Vec<Closure<dyn FnMut(PointerEvent)>>,
}

impl std::fmt::Debug for PointerCapture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PointerCapture")
            .field("active", &self.pointers.borrow().len())
            .finish()
    }
}

impl PointerCapture {
    /// Install pointer listeners on `canvas` and begin tracking down pointers.
    pub fn install(canvas: &HtmlCanvasElement) -> PointerCapture {
        let pointers: Pointers = Rc::new(RefCell::new(BTreeMap::new()));
        let mut closures = Vec::new();

        // down / move: record or update this pointer's physical-pixel position.
        let track = {
            let pointers = pointers.clone();
            let canvas = canvas.clone();
            move |ev: PointerEvent| {
                let (x, y) = physical_position(&canvas, &ev);
                pointers.borrow_mut().insert(ev.pointer_id(), (x, y));
            }
        };
        // up / cancel / leave: the pointer is no longer down.
        let release = {
            let pointers = pointers.clone();
            move |ev: PointerEvent| {
                pointers.borrow_mut().remove(&ev.pointer_id());
            }
        };

        for name in ["pointerdown", "pointermove"] {
            let cb = Closure::wrap(Box::new(track.clone()) as Box<dyn FnMut(PointerEvent)>);
            let _ = canvas
                .add_event_listener_with_callback(name, cb.as_ref().unchecked_ref());
            closures.push(cb);
        }
        for name in ["pointerup", "pointercancel", "pointerleave"] {
            let cb = Closure::wrap(Box::new(release.clone()) as Box<dyn FnMut(PointerEvent)>);
            let _ = canvas
                .add_event_listener_with_callback(name, cb.as_ref().unchecked_ref());
            closures.push(cb);
        }

        PointerCapture {
            pointers,
            _closures: closures,
        }
    }

    /// A snapshot of the currently-down pointers as `(x, y, down)` in physical
    /// surface pixels. Every returned pointer is down (`true`); the flag is part
    /// of the neutral shape `axiom_input::TouchControls::update` consumes, so a
    /// future hover source could report `false` through the same path.
    pub fn samples(&self) -> Vec<(f32, f32, bool)> {
        self.pointers
            .borrow()
            .values()
            .map(|(x, y)| (*x, *y, true))
            .collect()
    }
}

/// Convert a pointer event's client coordinates to physical surface pixels: CSS
/// position within the canvas rect, scaled by the canvas backing-store ratio
/// (`canvas.width / rect.width`), so it matches the viewport the renderer draws.
fn physical_position(canvas: &HtmlCanvasElement, ev: &PointerEvent) -> (f32, f32) {
    let rect = canvas.get_bounding_client_rect();
    let css_w = rect.width().max(1.0);
    let css_h = rect.height().max(1.0);
    let sx = (canvas.width() as f64) / css_w;
    let sy = (canvas.height() as f64) / css_h;
    let x = ((ev.client_x() as f64) - rect.left()) * sx;
    let y = ((ev.client_y() as f64) - rect.top()) * sy;
    (x as f32, y as f32)
}
