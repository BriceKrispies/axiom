//! Unified pointer capture: mouse, touch and pen, reduced to the one neutral
//! shape the game reads.
//!
//! PointerEvent is the browser API that reports all three contact types
//! identically, so there is exactly one listener set and the game above cannot
//! tell them apart. Two details are the whole reason this exists rather than
//! being borrowed:
//!
//! * **A hovering mouse is not a contact.** `pointermove` fires constantly with
//!   no button held, and a trajectory editor that treated that as a drag would
//!   deform the shot every time the cursor crossed the panel. A mouse is only
//!   down when `buttons` says so.
//! * **Every way a gesture can end has to end it.** `pointerup` is the polite
//!   one; `pointercancel` (the browser took the pointer for a system gesture)
//!   and `pointerout` are the ones that otherwise leave a curve welded to a
//!   finger that is no longer on the glass.
//!
//! Positions are converted to **physical surface pixels** — the canvas'
//! backing-store scale, not CSS pixels — because that is the space the camera
//! and the overlay both work in, so the device pixel ratio is handled once, here.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use axiom::prelude::Vec2;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{Element, PointerEvent};

/// The live set of pressed contacts, keyed by pointer id. A `BTreeMap` keeps the
/// sample order stable across frames however the contacts arrived.
type Contacts = Rc<RefCell<BTreeMap<i32, Vec2>>>;

/// Installed pointer listeners, and the contacts they maintain.
pub struct PointerCapture {
    contacts: Contacts,
    _closures: Vec<Closure<dyn FnMut(PointerEvent)>>,
}

impl core::fmt::Debug for PointerCapture {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PointerCapture")
            .field("pressed", &self.contacts.borrow().len())
            .finish()
    }
}

impl PointerCapture {
    /// Install listeners on the element with this id.
    pub fn install(element_id: &str) -> Option<PointerCapture> {
        let element: Element = web_sys::window()?
            .document()?
            .get_element_by_id(element_id)?;
        let contacts: Contacts = Rc::new(RefCell::new(BTreeMap::new()));
        let mut closures: Vec<Closure<dyn FnMut(PointerEvent)>> = Vec::new();

        let press = {
            let contacts = contacts.clone();
            let element = element.clone();
            move |event: PointerEvent| {
                // A mouse with no button held is a hover, not a contact. Touch
                // and pen report `buttons == 1` while they are on the glass, so
                // the same test covers all three.
                match event.buttons() == 0 {
                    true => {
                        contacts.borrow_mut().remove(&event.pointer_id());
                    }
                    false => {
                        contacts
                            .borrow_mut()
                            .insert(event.pointer_id(), physical(&element, &event));
                    }
                }
                // The page must not scroll, select or fire a browser gesture
                // while the player is drawing a shot. This is scoped to the
                // canvas' own events; nothing outside this element is touched.
                event.prevent_default();
            }
        };
        let release = {
            let contacts = contacts.clone();
            move |event: PointerEvent| {
                contacts.borrow_mut().remove(&event.pointer_id());
            }
        };

        ["pointerdown", "pointermove"].into_iter().for_each(|name| {
            let cb = Closure::wrap(Box::new(press.clone()) as Box<dyn FnMut(PointerEvent)>);
            let _ = element.add_event_listener_with_callback(name, cb.as_ref().unchecked_ref());
            closures.push(cb);
        });
        ["pointerup", "pointercancel", "pointerout", "pointerleave"]
            .into_iter()
            .for_each(|name| {
                let cb = Closure::wrap(Box::new(release.clone()) as Box<dyn FnMut(PointerEvent)>);
                let _ = element.add_event_listener_with_callback(name, cb.as_ref().unchecked_ref());
                closures.push(cb);
            });

        Some(PointerCapture {
            contacts,
            _closures: closures,
        })
    }

    /// This frame's contacts, as the neutral `(position, is_down)` samples the
    /// input module folds into a snapshot.
    pub fn samples(&self) -> Vec<(Vec2, bool)> {
        self.contacts
            .borrow()
            .values()
            .map(|p| (*p, true))
            .collect()
    }
}

/// A pointer event's position in physical surface pixels.
fn physical(element: &Element, event: &PointerEvent) -> Vec2 {
    let rect = element.get_bounding_client_rect();
    let scale = web_sys::window()
        .map(|w| w.device_pixel_ratio())
        .unwrap_or(1.0);
    Vec2::new(
        ((event.client_x() as f64 - rect.left()) * scale) as f32,
        ((event.client_y() as f64 - rect.top()) * scale) as f32,
    )
}
