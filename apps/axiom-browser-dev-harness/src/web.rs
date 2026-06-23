//! The `wasm32` browser entry: mount the debug overlay over the page and drive a
//! stub-diagnostics `requestAnimationFrame` loop so the read-out is alive the
//! moment it's toggled on.
//!
//! This is the harness's nondeterministic edge. It owns no overlay logic — it
//! mounts [`DebugOverlayController`] and feeds it
//! [`BrowserDiagnosticsSnapshot`]s built from a measured frame counter + fps.
//! A real app would mount the same controller and feed *its* host diagnostics in
//! through the identical [`DebugOverlayController::update_diagnostics`] seam.

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use crate::browser_diagnostics::BrowserDiagnosticsSnapshot;
use crate::debug_overlay::DebugOverlayController;

/// Re-measure fps over at least this many milliseconds before updating the
/// displayed rate (a short smoothing window).
const FPS_WINDOW_MS: f64 = 250.0;

/// Browser entry: mount the overlay (hidden until `` ` `` is pressed) and start
/// the stub-diagnostics loop.
#[wasm_bindgen]
pub fn start() {
    console_error_panic_hook::set_once();
    let mut controller = DebugOverlayController::new();
    controller.mount_to_body();
    drive_stub_diagnostics(controller);
}

/// Own `controller` for the life of the page and push a fresh stub snapshot every
/// animation frame. The closure forms the usual self-rescheduling RAF cycle; it
/// (and the controller, and thus its keyboard listeners) lives until the page is
/// torn down.
fn drive_stub_diagnostics(controller: DebugOverlayController) {
    let performance = web_sys::window()
        .expect("a browser window")
        .performance()
        .expect("performance clock");

    let callback: Rc<RefCell<Option<Closure<dyn FnMut()>>>> = Rc::new(RefCell::new(None));
    let scheduler = callback.clone();

    let mut frame: u64 = 0;
    let mut last = performance.now();
    let mut window_ms = 0.0_f64;
    let mut window_frames = 0_u32;
    let mut fps = 0.0_f64;
    let mut frame_ms = 0.0_f64;

    *callback.borrow_mut() = Some(Closure::<dyn FnMut()>::new(move || {
        let now = performance.now();
        window_ms += now - last;
        window_frames += 1;
        last = now;
        if window_ms >= FPS_WINDOW_MS {
            fps = f64::from(window_frames) * 1000.0 / window_ms;
            frame_ms = window_ms / f64::from(window_frames);
            window_ms = 0.0;
            window_frames = 0;
        }
        frame += 1;

        let snapshot = BrowserDiagnosticsSnapshot::stub().with_frame(frame, frame, fps, frame_ms);
        controller.update_diagnostics(snapshot);

        request_animation_frame(scheduler.borrow().as_ref().expect("raf closure set"));
    }));

    request_animation_frame(callback.borrow().as_ref().expect("raf closure set"));
}

fn request_animation_frame(closure: &Closure<dyn FnMut()>) {
    let _ = web_sys::window()
        .expect("a browser window")
        .request_animation_frame(closure.as_ref().unchecked_ref());
}
