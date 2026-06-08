//! The per-frame render loop (wasm32 only).
//!
//! The run-loop counters now live in `axiom_windowing::WindowingApi` (the app
//! drives ticks through it). This module is just the wasm32
//! `requestAnimationFrame` driver: it advances one deterministic tick per frame
//! and clears+presents through the live GPU binding.

// ---------------------------------------------------------------------------
// wasm32 requestAnimationFrame driver — never compiled on native.
// ---------------------------------------------------------------------------

#[cfg(target_arch = "wasm32")]
pub(crate) use wasm_loop::run;

#[cfg(target_arch = "wasm32")]
mod wasm_loop {
    use std::cell::RefCell;
    use std::rc::Rc;

    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::{JsCast, JsValue};

    use crate::browser_api::BrowserRotatingCubeApi;

    fn request_animation_frame(callback: &Closure<dyn FnMut()>) -> Result<(), JsValue> {
        let window = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
        window
            .request_animation_frame(callback.as_ref().unchecked_ref())
            .map(|_| ())
    }

    /// Drive the app once per animation frame: each frame produces the
    /// deterministic rotating-cube `GpuSubmission` (recorded through the live
    /// `WebGpuApi`) and clears+presents the canvas through the bound live
    /// binding. The cube draw itself is not executed — see VISIBLE_SLICE.md.
    pub fn run(app: BrowserRotatingCubeApi) -> Result<(), JsValue> {
        let app = Rc::new(RefCell::new(app));

        let f: Rc<RefCell<Option<Closure<dyn FnMut()>>>> = Rc::new(RefCell::new(None));
        let g = f.clone();
        let app_for_frame = app.clone();

        *g.borrow_mut() = Some(Closure::wrap(Box::new(move || {
            let outcome = app_for_frame.borrow_mut().advance_tick();
            let instances = outcome.instance_floats();
            let _ = app_for_frame.borrow().present_cubes(
                &instances,
                outcome.cubes.len() as u32,
                outcome.clear_color,
            );

            if let Some(cb) = f.borrow().as_ref() {
                let _ = request_animation_frame(cb);
            }
        }) as Box<dyn FnMut()>));

        let initial = g.borrow();
        request_animation_frame(initial.as_ref().unwrap())
    }
}
