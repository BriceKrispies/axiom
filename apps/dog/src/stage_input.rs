//! The DOM half of the stage switch: **the two buttons are built here, from
//! [`Stage::ALL`]**, and they write the shared stage the frame closure reads.
//!
//! The same split the dial panel has. A button whose label lives in HTML and
//! whose meaning lives in Rust is two sources of truth that drift the first time
//! a stage is renamed, so the page carries one empty container and every control
//! in it is created from the enum: a stage cannot exist without a button in
//! front of it, and a button cannot exist without a stage behind it.
//!
//! Pressing one does exactly three things, none of which touches geometry:
//! it writes the shared [`Stage`] cell the frame closure reads, it re-seeds the
//! orbit camera from that stage's own authored framing, and it records the
//! choice in the address bar so a reload comes back to the stage the user was
//! on. Everything a stage *means* — how many dogs, whether they walk, where the
//! camera opens — is answered natively in `src/stage.rs`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use crate::orbit::OrbitState;
use crate::page_url;
use crate::stage::Stage;

/// The query parameter the chosen stage round-trips through.
pub const STAGE_KEY: &str = "stage";

/// The live stage, shared between the DOM's event handlers and the render loop.
/// A `Cell`, not a `RefCell`: a [`Stage`] is `Copy` and nothing ever holds a
/// borrow of it across a frame.
pub type SharedStage = Rc<Cell<Stage>>;

/// The shared orbit camera the buttons re-seat.
pub type SharedOrbit = Rc<RefCell<OrbitState>>;

/// Build the stage switch inside the element with id `container_id`.
///
/// A page without that element (a harness, a screenshot host) simply gets no
/// switch — the scene still runs on whatever stage it was handed.
pub fn install(container_id: &str, stage: SharedStage, orbit: SharedOrbit) {
    let Some(document) = web_sys::window().and_then(|window| window.document()) else {
        return;
    };
    let Some(container) = document.get_element_by_id(container_id) else {
        return;
    };
    container.set_inner_html("");
    for choice in Stage::ALL {
        let Some(button) = document
            .create_element("button")
            .ok()
            .and_then(|element| element.dyn_into::<web_sys::HtmlButtonElement>().ok())
        else {
            continue;
        };
        button.set_type("button");
        button.set_id(&button_id(choice));
        button.set_text_content(Some(choice.label()));
        let _ = button.set_attribute("data-stage", choice.key());
        let _ = container.append_child(&button);
        listen(&button, choice, stage.clone(), orbit.clone());
    }
    mark(stage.get());
}

/// Attach the one `click` handler this button needs.
fn listen(
    button: &web_sys::HtmlButtonElement,
    choice: Stage,
    stage: SharedStage,
    orbit: SharedOrbit,
) {
    let handler = Closure::wrap(Box::new(move || {
        stage.set(choice);
        // The camera is re-seeded rather than left where the last stage's
        // gestures put it: a 195-unit field shot is uselessly far away from one
        // dog, and a 20-unit close-up is inside the terrain. Each stage opens on
        // its own authored framing and is free from there.
        //
        // The lock rides across the seed. It stops the *user* moving the camera;
        // it does not stop the page choosing which shot to open on, and a locked
        // page that came back unlocked from a stage change would have thrown a
        // choice away silently.
        let lock = orbit.borrow().lock();
        *orbit.borrow_mut() = OrbitState::for_stage(choice).with_lock(lock);
        mark(choice);
        page_url::remember_param(STAGE_KEY, choice.key());
    }) as Box<dyn FnMut()>);
    let _ = button.add_event_listener_with_callback("click", handler.as_ref().unchecked_ref());
    handler.forget();
}

/// Show which stage is up. The class is the page's only styling hook — the
/// switch has no other state.
fn mark(active: Stage) {
    let Some(document) = web_sys::window().and_then(|window| window.document()) else {
        return;
    };
    for choice in Stage::ALL {
        document
            .get_element_by_id(&button_id(choice))
            .into_iter()
            .for_each(|button| {
                button.set_class_name(
                    ["stage-button", "stage-button on"][usize::from(choice == active)],
                );
                let _ = button.set_attribute(
                    "aria-pressed",
                    ["false", "true"][usize::from(choice == active)],
                );
            });
    }
}

/// The DOM id of `stage`'s button.
fn button_id(stage: Stage) -> String {
    format!("stage-button-{}", stage.key())
}
