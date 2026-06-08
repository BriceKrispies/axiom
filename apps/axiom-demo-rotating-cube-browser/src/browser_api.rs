//! The single public facade of the browser app: [`BrowserRotatingCubeApi`].

use axiom_kernel::{KernelResult, Ratio};
use axiom_webgpu::WebGpuApi;

use crate::browser_surface_registry::BrowserSurfaceRegistry;
use crate::cube_slice::{CubeSliceDriver, TickOutcome};
use crate::live_gpu_binding::LiveBindingState;

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
/// the validated `HostPresentationRequest` and the run loop), the deterministic
/// slice driver, the live `WebGpuApi`, and the surface registry (whose real GPU
/// binding is attached only on wasm32). All of that constructs and is tested on
/// native; the `#[wasm_bindgen]` startup methods exist only on wasm32.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
#[derive(Debug)]
pub struct BrowserRotatingCubeApi {
    canvas_id: String,
    windowing: WindowingApi,
    webgpu: WebGpuApi,
    driver: CubeSliceDriver,
    registry: BrowserSurfaceRegistry,
}

impl BrowserRotatingCubeApi {
    /// Build the app for a `width` x `height` canvas identified by `canvas_id`.
    ///
    /// Deterministic and browser-free: `axiom-windowing` assembles the
    /// `HostPresentationRequest`, from which a **live** `WebGpuApi` is created
    /// and the surface handle registered. Any host/webgpu validation failure
    /// surfaces through `KernelResult`.
    pub fn new(canvas_id: &str, width: u32, height: u32) -> KernelResult<Self> {
        let mut windowing = WindowingApi::new();
        windowing.configure_surface(width, height)?;
        let request = windowing
            .presentation_request()
            .expect("surface was just configured");
        let webgpu = WebGpuApi::new_live(request)?;
        let surface_id = request.surface().id().raw();

        let host = HostApi::new();
        let viewport = host
            .viewport(
                width,
                height,
                Ratio::new(1.0).expect("unit scale factor is finite"),
            )
            .expect("viewport dimensions already validated by the request");
        let driver = CubeSliceDriver::new(viewport);

        let mut registry = BrowserSurfaceRegistry::new();
        registry.register(surface_id, width, height);

        Ok(BrowserRotatingCubeApi {
            canvas_id: canvas_id.to_string(),
            windowing,
            webgpu,
            driver,
            registry,
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

    /// The deterministic live-binding lifecycle state for the surface handle.
    /// `pub(crate)`: `LiveBindingState` is an internal type, so this is not
    /// part of the single public facade (kept reachable for tests + the loop).
    pub(crate) fn binding_state(&self) -> LiveBindingState {
        self.registry.state(self.request().surface().id().raw())
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
    /// binding asynchronously, attach it to the surface registry, and run the
    /// requestAnimationFrame loop. Consumes `self` into the loop.
    #[wasm_bindgen]
    pub fn start(self) -> Result<(), JsValue> {
        let canvas = crate::browser_bootstrap::find_canvas(&self.canvas_id)?;
        let width = self.viewport_width();
        let height = self.viewport_height();
        let geometry = self.driver.cube_geometry();
        let max_instances = crate::cube_slice::NUM_CUBES as u32;

        wasm_bindgen_futures::spawn_local(async move {
            let mut app = self;
            match crate::live_gpu_binding::LiveGpuBinding::initialize(
                canvas,
                width,
                height,
                geometry,
                max_instances,
            )
            .await
            {
                Ok(binding) => {
                    app.attach_live_binding(binding);
                    let _ = crate::render_loop::run(app);
                }
                Err(_) => {
                    let surface_id = app.request().surface().id().raw();
                    app.registry.fail(surface_id);
                }
            }
        });
        Ok(())
    }
}

#[cfg(target_arch = "wasm32")]
impl BrowserRotatingCubeApi {
    /// Attach the real wgpu binding to the registry (wasm32 only).
    pub(crate) fn attach_live_binding(
        &mut self,
        binding: crate::live_gpu_binding::LiveGpuBinding,
    ) {
        let surface_id = self.request().surface().id().raw();
        self.registry.attach_binding(surface_id, binding);
    }

    /// Draw + present all cubes for one tick through the bound live binding,
    /// using the engine's per-cube instances and clear colour (wasm32 only).
    pub(crate) fn present_cubes(
        &self,
        instances: &[f32],
        instance_count: u32,
        color: [f32; 4],
    ) -> Result<(), wasm_bindgen::JsValue> {
        match self.registry.binding(self.request().surface().id().raw()) {
            Some(binding) => binding.render_frame(instances, instance_count, color),
            None => Err(wasm_bindgen::JsValue::from_str(
                "no live GPU binding attached for the surface handle",
            )),
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
        // Surface handle registered, but no real device can be acquired on
        // native — the binding stops at SurfaceRegistered.
        assert_eq!(app.binding_state(), LiveBindingState::SurfaceRegistered);
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
