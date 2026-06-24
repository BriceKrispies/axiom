//! The `wasm32` browser entry: mount the module's overlay and drive it with real,
//! measured diagnostics.
//!
//! This is the harness's whole job and its nondeterministic edge. It owns no
//! overlay logic — it constructs [`DebugOverlayApi`], mounts it, and each
//! animation frame pushes measured values in. `start()` takes no arguments, so
//! both the harness's own page and the gallery's shared shell (which loads this
//! wasm over a running demo) drive it the same way.

use std::cell::RefCell;
use std::rc::Rc;

use axiom_debug_overlay::DebugOverlayApi;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

/// Re-measure fps over at least this many milliseconds before updating the rate.
const FPS_WINDOW_MS: f64 = 250.0;

/// Browser entry: mount the overlay (hidden until `` ` `` is pressed), seed the
/// real backend probe, and start the measured-diagnostics loop.
#[wasm_bindgen]
pub fn start() {
    console_error_panic_hook::set_once();

    let mut overlay = DebugOverlayApi::new();
    overlay.mount_to_body();

    // The live render backend, probed the way the engine selects it.
    let (backend, fallback_count, fallback_reason) = probe_backend();
    overlay.set_backends(&backend, "dev-harness", "none", "none", "none", "none");
    overlay.set_fallback(fallback_count, &fallback_reason);
    // The harness runs no engine sim/GPU/workers, so these are honest zeroes.
    overlay.set_counters(0, 0, 0, 0);

    drive_diagnostics(overlay);
}

/// Probe the live render backend the way the engine does: WebGPU when
/// `navigator.gpu` exists, else the WebGL2 fallback (the engine's real
/// degradation path). Returns `(renderer, fallback_count, fallback_reason)`.
fn probe_backend() -> (String, u32, String) {
    let has_webgpu = web_sys::window()
        .map(|window| window.navigator())
        .and_then(|navigator| js_sys::Reflect::has(&navigator, &JsValue::from_str("gpu")).ok())
        .unwrap_or(false);
    if has_webgpu {
        ("webgpu".to_string(), 0, "none".to_string())
    } else {
        ("webgl2".to_string(), 1, "navigator.gpu absent".to_string())
    }
}

/// Own the overlay for the page's lifetime and push a measured diagnostics frame
/// every animation frame (frame counter, fps + frame time over a short window,
/// and live document visibility).
fn drive_diagnostics(overlay: DebugOverlayApi) {
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
    let mut fps_milli = 0_u32;
    let mut frame_micros = 0_u32;

    *callback.borrow_mut() = Some(Closure::<dyn FnMut()>::new(move || {
        let now = performance.now();
        window_ms += now - last;
        window_frames += 1;
        last = now;
        if window_ms >= FPS_WINDOW_MS {
            // fps × 1000 (integer-encoded, the overlay formats it), and the mean
            // frame time in microseconds.
            fps_milli = (f64::from(window_frames) * 1_000_000.0 / window_ms) as u32;
            frame_micros = (window_ms * 1000.0 / f64::from(window_frames)) as u32;
            window_ms = 0.0;
            window_frames = 0;
        }
        frame += 1;

        let hidden = web_sys::window()
            .and_then(|window| window.document())
            .map(|document| document.hidden())
            .unwrap_or(false);
        let visibility = if hidden { "hidden" } else { "visible" };

        overlay.set_frame(frame, frame, 0, fps_milli, frame_micros);
        overlay.set_visibility(visibility);

        request_animation_frame(scheduler.borrow().as_ref().expect("raf closure set"));
    }));

    request_animation_frame(callback.borrow().as_ref().expect("raf closure set"));
}

fn request_animation_frame(closure: &Closure<dyn FnMut()>) {
    let _ = web_sys::window()
        .expect("a browser window")
        .request_animation_frame(closure.as_ref().unchecked_ref());
}
