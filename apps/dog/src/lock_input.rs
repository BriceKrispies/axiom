//! The DOM half of the camera lock: **one button, built here**, writing the lock
//! on the shared [`OrbitState`] that both the gesture listeners and the frame
//! closure already read.
//!
//! The same split the dial panel and the stage switch have. The button's whole
//! meaning — what the two states are called, which one an unknown URL opens on,
//! what the hint does while one of them holds — is answered natively in
//! `src/camera_lock.rs`; this file turns a click into that value and dresses the
//! page to match.
//!
//! Pressing it does four things, none of which touches geometry or the scene:
//!
//! 1. flips the lock on the orbit, which is what actually stops the camera;
//! 2. hands the canvas's `touch-action` back to the document (or takes it), so
//!    the page stays usable under the finger — see `src/pointer_input.rs`;
//! 3. relabels itself and dims the gesture hint, so the page does not promise a
//!    drag it will not honour;
//! 4. records the choice in the address bar, so the reload the detail dial and a
//!    device rotation both trigger comes back locked.
//!
//! There is no separate lock cell anywhere: the camera is the thing being
//! locked, so the bit lives on the camera. A second copy of it here would be a
//! second source of truth, and the two would drift the first time one of them
//! was written without the other.

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use crate::camera_lock::{CameraLock, LOCK_KEY};
use crate::page_url;
use crate::pointer_input::{self, SharedHerd, SharedOrbit};

/// The DOM id of the button this module builds.
const BUTTON_ID: &str = "lock-button";

/// The two hints the lock swaps between: what the gestures do to the camera,
/// and what they do to the dogs. A page carrying neither simply gets no hint.
const CAMERA_HINT_ID: &str = "dog-hint-camera";
const DOG_HINT_ID: &str = "dog-hint-dogs";

/// Build the lock button inside the element with id `container_id`, governing
/// the camera `orbit` and the gestures on the canvas with id `canvas_id`.
///
/// A page without that element (a harness, a screenshot host) simply gets no
/// button — the scene still runs at whatever lock it was handed.
pub fn install(container_id: &str, canvas_id: &str, orbit: SharedOrbit, herd: SharedHerd) {
    let Some(document) = web_sys::window().and_then(|window| window.document()) else {
        return;
    };
    let Some(container) = document.get_element_by_id(container_id) else {
        return;
    };
    container.set_inner_html("");
    let Some(button) = document
        .create_element("button")
        .ok()
        .and_then(|element| element.dyn_into::<web_sys::HtmlButtonElement>().ok())
    else {
        return;
    };
    button.set_type("button");
    button.set_id(BUTTON_ID);
    let _ = container.append_child(&button);
    listen(&button, canvas_id.to_string(), orbit.clone(), herd);
    // The opening state is the orbit's, not a default typed here: a page opened
    // at `?lock=on` shows a pressed button, and `pointer_input::install` has
    // already handed that page's gestures over.
    mark(orbit.borrow().lock());
}

/// Attach the one `click` handler this button needs.
fn listen(
    button: &web_sys::HtmlButtonElement,
    canvas_id: String,
    orbit: SharedOrbit,
    herd: SharedHerd,
) {
    let handler = Closure::wrap(Box::new(move || {
        let lock = orbit.borrow_mut().toggle_lock();
        // The camera is now holding still, so the canvas stops claiming the
        // gestures it is no longer using. Without this the page would freeze the
        // shot and *still* refuse to scroll over it.
        pointer_input::set_gestures(&canvas_id, lock);
        // Unlocking mid-drag hands the gestures back to the camera, so whatever
        // was in the user's hand is let go rather than left pinned to a point no
        // pointer is going to move again. It walks home from wherever it was.
        herd.borrow_mut().release();
        mark(lock);
        page_url::remember_param(LOCK_KEY, lock.param());
    }) as Box<dyn FnMut()>);
    let _ = button.add_event_listener_with_callback("click", handler.as_ref().unchecked_ref());
    handler.forget();
}

/// Show the state the camera is in: the button's label, its pressed styling and
/// the gesture hint, all three from the one value.
fn mark(lock: CameraLock) {
    let Some(document) = web_sys::window().and_then(|window| window.document()) else {
        return;
    };
    document
        .get_element_by_id(BUTTON_ID)
        .into_iter()
        .for_each(|button| {
            button.set_text_content(Some(lock.label()));
            button.set_class_name(["lock-button", "lock-button on"][usize::from(lock.holds())]);
            let _ = button.set_attribute("aria-pressed", ["false", "true"][usize::from(lock.holds())]);
        });
    // The page carries both hints and shows the one that is currently true: a
    // "drag to orbit" under a locked camera is a lie printed on the page, and so
    // is a "drag a dog" under a free one.
    [
        (CAMERA_HINT_ID, lock.camera_hint_class()),
        (DOG_HINT_ID, lock.dog_hint_class()),
    ]
    .into_iter()
    .for_each(|(id, class)| {
        document
            .get_element_by_id(id)
            .into_iter()
            .for_each(|hint| hint.set_class_name(class));
    });
}
