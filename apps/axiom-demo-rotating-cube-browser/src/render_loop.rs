//! The per-frame render loop.
//!
//! The deterministic half tracks the tick/frame counters and is testable on
//! native. The wasm32 half drives `requestAnimationFrame`, advancing one
//! deterministic tick per frame and clearing+presenting through the live GPU
//! binding.

/// Deterministic render-loop bookkeeping: which tick is next and how many
/// frames have been presented. Browser-free and testable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RenderLoopState {
    next_tick: u64,
    frames_driven: u64,
}

impl RenderLoopState {
    pub const fn new() -> Self {
        RenderLoopState {
            next_tick: 0,
            frames_driven: 0,
        }
    }

    /// Consume the next tick index and advance the counters.
    pub fn step(&mut self) -> u64 {
        let tick = self.next_tick;
        self.next_tick += 1;
        self.frames_driven += 1;
        tick
    }

    pub const fn next_tick(&self) -> u64 {
        self.next_tick
    }

    pub const fn frames_driven(&self) -> u64 {
        self.frames_driven
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_yields_monotonic_ticks() {
        let mut s = RenderLoopState::new();
        assert_eq!(s.step(), 0);
        assert_eq!(s.step(), 1);
        assert_eq!(s.step(), 2);
        assert_eq!(s.next_tick(), 3);
        assert_eq!(s.frames_driven(), 3);
    }
}

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
