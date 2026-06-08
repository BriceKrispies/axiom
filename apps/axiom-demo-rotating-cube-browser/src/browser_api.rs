//! The single public facade of the browser app: [`BrowserRotatingCubeApi`].

use axiom_kernel::{KernelResult, Ratio};
use axiom_webgpu::WebGpuApi;

use crate::cube_slice::{CubeSliceDriver, TickOutcome};

use axiom_host::{HostApi, HostPresentationRequest};
use axiom_windowing::WindowingApi;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

/// The browser/WASM rotating-cube app — the **only** crate that touches
/// browser/platform APIs.
///
/// It composes the same engine modules as the headless app to produce the
/// **same** deterministic rotating-cube `GpuSubmission`, but feeds it into a
/// *live* `WebGpuApi` and (on wasm32) presents through a real wgpu binding.
///
/// The struct itself is browser-free: it owns the windowing driver (which holds
/// the validated `HostPresentationRequest`, the run loop, and — on wasm32 — the
/// real GPU binding), the deterministic slice driver, and the live `WebGpuApi`.
/// All of that constructs and is tested on native; the `#[wasm_bindgen]` startup
/// methods exist only on wasm32.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
#[derive(Debug)]
pub struct BrowserRotatingCubeApi {
    canvas_id: String,
    windowing: WindowingApi,
    webgpu: WebGpuApi,
    driver: CubeSliceDriver,
}

impl BrowserRotatingCubeApi {
    /// Build the app for a `width` x `height` canvas identified by `canvas_id`.
    ///
    /// Deterministic and browser-free: `axiom-windowing` assembles the
    /// `HostPresentationRequest`, from which a **live** `WebGpuApi` is created.
    /// Any host/webgpu validation failure surfaces through `KernelResult`.
    pub fn new(canvas_id: &str, width: u32, height: u32) -> KernelResult<Self> {
        let mut windowing = WindowingApi::new();
        windowing.configure_surface(width, height)?;
        let request = windowing
            .presentation_request()
            .expect("surface was just configured");
        let webgpu = WebGpuApi::new_live(request)?;

        let host = HostApi::new();
        let viewport = host
            .viewport(
                width,
                height,
                Ratio::new(1.0).expect("unit scale factor is finite"),
            )
            .expect("viewport dimensions already validated by the request");
        let driver = CubeSliceDriver::new(viewport);

        Ok(BrowserRotatingCubeApi {
            canvas_id: canvas_id.to_string(),
            windowing,
            webgpu,
            driver,
        })
    }

    /// The validated presentation request, assembled by `axiom-windowing` and
    /// present for the whole life of the app (a surface is configured at
    /// construction).
    fn request(&self) -> &HostPresentationRequest {
        self.windowing
            .presentation_request()
            .expect("a surface is configured at construction")
    }

    /// The canvas element id this app will look for.
    pub fn canvas_id(&self) -> &str {
        &self.canvas_id
    }

    /// The validated host presentation request driving the live backend.
    pub fn presentation_request(&self) -> &HostPresentationRequest {
        self.request()
    }

    pub fn viewport_width(&self) -> u32 {
        self.request().descriptor().viewport().physical_width()
    }

    pub fn viewport_height(&self) -> u32 {
        self.request().descriptor().viewport().physical_height()
    }

    pub fn presentation_target_label(&self) -> &str {
        self.request().target().label()
    }

    pub fn surface_handle_id(&self) -> u64 {
        self.request().surface().id().raw()
    }

    /// Whether the backend is a live `WebGpuApi`.
    pub fn is_live(&self) -> bool {
        self.webgpu.is_live()
    }

    /// Whether the live backend has a bound presentation request.
    pub fn has_presentation_request(&self) -> bool {
        self.webgpu.has_presentation_request()
    }

    /// Whether the live GPU binding is ready to present (always false on native;
    /// true on wasm32 once the binding has initialised).
    pub(crate) fn binding_is_ready(&self) -> bool {
        self.windowing.binding_is_ready()
    }

    /// The next tick index this app will drive.
    pub fn next_tick(&self) -> u64 {
        self.windowing.next_tick()
    }

    /// Drive one deterministic tick: produce the rotating-cube `GpuSubmission`
    /// and submit it through the live backend. Browser-free and testable.
    /// `pub(crate)`: returns the internal `TickOutcome`, so it is not part of
    /// the single public facade (used by tests + the wasm render loop).
    pub(crate) fn advance_tick(&mut self) -> TickOutcome {
        let tick = self.windowing.step();
        self.driver.drive_tick(&self.webgpu, tick)
    }
}

// --- wasm32 startup surface ---------------------------------------------------

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl BrowserRotatingCubeApi {
    /// JS constructor: `new BrowserRotatingCubeApi(canvasId, width, height)`.
    #[wasm_bindgen(constructor)]
    pub fn new_for_canvas(
        canvas_id: String,
        width: u32,
        height: u32,
    ) -> Result<BrowserRotatingCubeApi, JsValue> {
        console_error_panic_hook::set_once();
        BrowserRotatingCubeApi::new(&canvas_id, width, height)
            .map_err(|e| JsValue::from_str(e.message()))
    }

    /// Boot the visible slice: locate the canvas, initialise the real wgpu
    /// binding asynchronously through `axiom-windowing`, then run the
    /// requestAnimationFrame loop. Consumes `self` into the loop.
    #[wasm_bindgen]
    pub fn start(self) -> Result<(), JsValue> {
        let canvas = crate::browser_bootstrap::find_canvas(&self.canvas_id)?;
        let geometry = self.driver.cube_geometry();
        let max_instances = crate::cube_slice::NUM_CUBES as u32;

        wasm_bindgen_futures::spawn_local(async move {
            let mut app = self;
            match app
                .windowing
                .initialize_live(canvas, &geometry.vertices, &geometry.indices, max_instances)
                .await
            {
                // The binding is now owned by windowing; drive the loop.
                Ok(()) => {
                    let _ = crate::render_loop::run(app);
                }
                // Init failed: windowing has no live binding, so nothing
                // presents — the loop is simply not started.
                Err(_) => {}
            }
        });
        Ok(())
    }
}

#[cfg(target_arch = "wasm32")]
impl BrowserRotatingCubeApi {
    /// Draw + present all cubes for one tick through `axiom-windowing`'s live
    /// binding, using the engine's per-cube instances and clear colour (wasm32
    /// only).
    pub(crate) fn present_cubes(
        &self,
        instances: &[f32],
        instance_count: u32,
        color: [f32; 4],
    ) -> Result<(), wasm_bindgen::JsValue> {
        if self.windowing.present_frame(color, instances, instance_count) {
            Ok(())
        } else {
            Err(wasm_bindgen::JsValue::from_str(
                "no live GPU binding ready to present",
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boots_and_builds_a_live_backend_from_deterministic_data() {
        let app = BrowserRotatingCubeApi::new("axiom-cube-canvas", 800, 600).unwrap();
        assert_eq!(app.canvas_id(), "axiom-cube-canvas");
        assert!(app.is_live());
        assert!(app.has_presentation_request());
        assert_eq!(app.viewport_width(), 800);
        assert_eq!(app.viewport_height(), 600);
        assert_eq!(app.presentation_target_label(), "axiom-window-surface");
        // No real device can be acquired on native, so the live binding never
        // becomes ready — the app simulates but never presents.
        assert!(!app.binding_is_ready());
    }

    #[test]
    fn advancing_ticks_produces_the_deterministic_cube_submission() {
        let mut app = BrowserRotatingCubeApi::new("axiom-cube-canvas", 800, 600).unwrap();
        let f0 = app.advance_tick();
        assert_eq!(f0.gpu_command_count, 13); // three cubes
        assert_eq!(f0.cubes.len(), 3);
        assert!(!f0.presented); // no real device bound natively
        assert_eq!(app.next_tick(), 1);
    }

    #[test]
    fn invalid_dimensions_fail_through_kernel_result() {
        assert!(BrowserRotatingCubeApi::new("axiom-cube-canvas", 0, 600).is_err());
    }
}
