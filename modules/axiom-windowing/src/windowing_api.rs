//! The single windowing facade: assemble a presentation request, drive the loop.

use axiom_host::{
    HostAlphaMode, HostApi, HostColorFormat, HostDeviceProfile, HostError, HostPowerPreference,
    HostPresentMode, HostPresentationRequest,
};
use axiom_kernel::{
    KernelApi, KernelError, KernelErrorCode, KernelErrorScope, KernelResult, Ratio,
};

/// Deterministic kernel `HandleId` raw value for the presentation target.
const TARGET_HANDLE_RAW: u64 = 1;
/// Deterministic kernel `HandleId` raw value for the surface handle.
const SURFACE_HANDLE_RAW: u64 = 2;
/// Deterministic presentation-target label.
const TARGET_LABEL: &str = "axiom-window-surface";

/// Map a host-boundary validation failure into the kernel error model, so the
/// windowing surface reports a single `KernelResult` failure type.
fn host_to_kernel(_: HostError) -> KernelError {
    KernelError::new(
        KernelErrorScope::Id,
        KernelErrorCode::InvalidId,
        "invalid host presentation data for the window surface",
    )
}

/// The deterministic presentation driver for one window.
///
/// It holds the validated [`HostPresentationRequest`] once a surface is
/// configured, plus the fixed-step loop counters `App::run` pumps. Plain data
/// in, replayable state out — no browser or GPU object lives here. The real GPU
/// work is delegated to `axiom-gpu-backend` (the `GpuBackendApi`) on wasm32, which
/// this driver constructs from the presentation request and drives once per
/// animation frame. Two `WindowingApi`s driven with the same calls reach the same
/// observable state.
#[derive(Debug)]
pub struct WindowingApi {
    surface: Option<HostPresentationRequest>,
    next_tick: u64,
    frames_driven: u64,
}

impl WindowingApi {
    /// A fresh driver: no surface configured, loop at tick 0.
    pub fn new() -> Self {
        WindowingApi {
            surface: None,
            next_tick: 0,
            frames_driven: 0,
        }
    }

    /// Assemble and store the validated presentation request for a
    /// `width` x `height` surface. **No browser objects are touched** — this is
    /// pure host-owned data, so it runs and is tested on native exactly as it
    /// will on the web. Fails (leaving the driver unconfigured) when the host
    /// rejects the viewport dimensions.
    pub fn configure_surface(&mut self, width: u32, height: u32) -> KernelResult<()> {
        let host = HostApi::new();
        let kernel = KernelApi::new();

        // The one genuinely fallible step with caller-supplied data: the host
        // rejects a zero/oversized viewport. The remaining steps use fixed,
        // valid constants and so cannot fail (documented at each site). The
        // success arm builds and stores the request; on the viewport error we
        // return it and leave the surface unconfigured — expressed branchlessly
        // through `map`, so this carries no `?`.
        host.viewport(
            width,
            height,
            Ratio::new(1.0).expect("unit scale factor is finite"),
        )
        .map_err(host_to_kernel)
        .map(|viewport| {
            let target = host
                .presentation_target(&kernel, TARGET_HANDLE_RAW, TARGET_LABEL)
                .expect("fixed non-zero target handle and non-empty label are valid");
            let surface = host
                .surface_handle(&kernel, SURFACE_HANDLE_RAW)
                .expect("fixed non-zero surface handle is valid");
            let descriptor = host.surface_descriptor(
                viewport,
                HostPresentMode::Fifo,
                HostAlphaMode::Opaque,
                HostColorFormat::Bgra8UnormSrgb,
            );
            let adapter = host.adapter_request(HostPowerPreference::HighPerformance, true);
            let device = host.device_request(true, HostDeviceProfile::Baseline);
            let request = host
                .presentation_request(target, surface, descriptor, adapter, device)
                .expect("adapter requires a presentation surface, matching the device request");
            self.surface = Some(request);
        })
    }

    /// Whether a surface has been configured.
    pub fn is_surface_configured(&self) -> bool {
        self.surface.is_some()
    }

    /// The configured surface's physical width, if any.
    pub fn surface_width(&self) -> Option<u32> {
        self.surface
            .as_ref()
            .map(|r| r.descriptor().viewport().physical_width())
    }

    /// The configured surface's physical height, if any.
    pub fn surface_height(&self) -> Option<u32> {
        self.surface
            .as_ref()
            .map(|r| r.descriptor().viewport().physical_height())
    }

    /// The validated presentation request, once a surface is configured. This
    /// is a `host`-layer value (nameable across the engine graph, unlike a
    /// module contract type), so a consumer can drive a live presentation
    /// backend and register its surface handle from it.
    pub fn presentation_request(&self) -> Option<&HostPresentationRequest> {
        self.surface.as_ref()
    }

    /// Drive one frame of the fixed-step loop: return the tick to simulate this
    /// frame and advance the counters. Monotonic and browser-free; the web arm
    /// calls this once per animation frame, a native/headless drive in a plain
    /// loop.
    pub fn step(&mut self) -> u64 {
        let tick = self.next_tick;
        self.next_tick += 1;
        self.frames_driven += 1;
        tick
    }

    /// The next tick this driver will hand out.
    pub fn next_tick(&self) -> u64 {
        self.next_tick
    }

    /// How many frames the loop has driven.
    pub fn frames_driven(&self) -> u64 {
        self.frames_driven
    }

    /// Drive the terminal web run loop over a **multi-mesh** scene. Construct the
    /// GPU backend from the configured presentation request, upload the distinct
    /// mesh set `meshes` (`(mesh_id, interleaved position+normal+colour vertices
    /// [10 floats/vertex], triangle indices)`), then present one frame per
    /// `requestAnimationFrame`: each frame the loop owns the monotonic tick
    /// ([`Self::step`]), hands it to `frame_fn`, and delegates the per-mesh
    /// instance batches it returns — `(clear_color, [(mesh_id, [mvp(16),colour(4)]
    /// per instance, count)])` — to the backend. wasm32 only; consumes the driver
    /// into the loop. If no surface is configured or init fails, nothing presents.
    #[cfg(target_arch = "wasm32")]
    pub fn run_web_multi<F>(
        self,
        canvas_id: &str,
        meshes: Vec<(u64, Vec<f32>, Vec<u32>)>,
        max_instances: u32,
        frame_fn: F,
    ) -> Result<(), wasm_bindgen::JsValue>
    where
        F: FnMut(u64) -> ([f32; 4], Vec<(u64, Vec<f32>, u32)>) + 'static,
    {
        use axiom_gpu_backend::GpuBackendApi;
        use std::cell::RefCell;
        use std::rc::Rc;
        use wasm_bindgen::closure::Closure;

        let canvas = find_canvas(canvas_id)?;
        let frame_fn = Rc::new(RefCell::new(frame_fn));
        let windowing = self;

        wasm_bindgen_futures::spawn_local(async move {
            let mut backend = match windowing.surface.as_ref() {
                Some(request) => GpuBackendApi::new(request),
                None => return,
            };
            if backend
                .initialize(canvas, &meshes, max_instances)
                .await
                .is_err()
            {
                return;
            }
            let windowing = Rc::new(RefCell::new(windowing));
            let backend = Rc::new(RefCell::new(backend));
            let f: Rc<RefCell<Option<Closure<dyn FnMut()>>>> = Rc::new(RefCell::new(None));
            let g = f.clone();
            let win = windowing.clone();
            let be = backend.clone();
            let ff = frame_fn.clone();
            *g.borrow_mut() = Some(Closure::wrap(Box::new(move || {
                let tick = win.borrow_mut().step();
                let (clear, batches) = (ff.borrow_mut())(tick);
                let _ = be.borrow().present_frame(clear, &batches);
                let next = f.borrow();
                if let Some(cb) = next.as_ref() {
                    let _ = request_animation_frame(cb);
                }
            }) as Box<dyn FnMut()>));
            let initial = g.borrow();
            if let Some(cb) = initial.as_ref() {
                let _ = request_animation_frame(cb);
            }
        });
        Ok(())
    }

    /// Drive the terminal web run loop over a **single mesh** (the
    /// back-compatible shape): equivalent to [`Self::run_web_multi`] with one
    /// uploaded mesh whose instances are the flat `(clear_color, [mvp(16),
    /// colour(4)] per instance, count)` the closure returns. wasm32 only;
    /// consumes the driver into the loop.
    #[cfg(target_arch = "wasm32")]
    pub fn run_web<F>(
        self,
        canvas_id: &str,
        vertices: Vec<f32>,
        indices: Vec<u32>,
        max_instances: u32,
        mut frame_fn: F,
    ) -> Result<(), wasm_bindgen::JsValue>
    where
        F: FnMut(u64) -> ([f32; 4], Vec<f32>, u32) + 'static,
    {
        const SINGLE_MESH_ID: u64 = 0;
        let meshes = vec![(SINGLE_MESH_ID, vertices, indices)];
        self.run_web_multi(canvas_id, meshes, max_instances, move |tick| {
            let (clear, instances, count) = frame_fn(tick);
            (clear, vec![(SINGLE_MESH_ID, instances, count)])
        })
    }

    /// Drive the terminal web run loop **with streaming geometry** (a single
    /// mesh). Identical to [`Self::run_web`] except the per-frame closure ALSO
    /// returns optional new geometry: `(clear_color, [mvp(16), colour(4)] per
    /// instance, count, Option<(vertices, indices)>)`. On the frames where it
    /// returns `Some`, the streamed mesh's buffers are replaced *before* drawing
    /// that frame, so the uploaded mesh follows the player while the camera stays
    /// continuous in world space. wasm32 only; consumes the driver into the loop.
    #[cfg(target_arch = "wasm32")]
    pub fn run_web_streaming<F>(
        self,
        canvas_id: &str,
        vertices: Vec<f32>,
        indices: Vec<u32>,
        max_instances: u32,
        frame_fn: F,
    ) -> Result<(), wasm_bindgen::JsValue>
    where
        F: FnMut(u64) -> ([f32; 4], Vec<f32>, u32, Option<(Vec<f32>, Vec<u32>)>) + 'static,
    {
        use axiom_gpu_backend::GpuBackendApi;
        use std::cell::RefCell;
        use std::rc::Rc;
        use wasm_bindgen::closure::Closure;

        const STREAM_MESH_ID: u64 = 0;
        let canvas = find_canvas(canvas_id)?;
        let frame_fn = Rc::new(RefCell::new(frame_fn));
        let windowing = self;

        wasm_bindgen_futures::spawn_local(async move {
            let mut backend = match windowing.surface.as_ref() {
                Some(request) => GpuBackendApi::new(request),
                None => return,
            };
            let meshes = vec![(STREAM_MESH_ID, vertices, indices)];
            if backend
                .initialize(canvas, &meshes, max_instances)
                .await
                .is_err()
            {
                return;
            }
            let windowing = Rc::new(RefCell::new(windowing));
            let backend = Rc::new(RefCell::new(backend));
            let f: Rc<RefCell<Option<Closure<dyn FnMut()>>>> = Rc::new(RefCell::new(None));
            let g = f.clone();
            let win = windowing.clone();
            let be = backend.clone();
            let ff = frame_fn.clone();
            *g.borrow_mut() = Some(Closure::wrap(Box::new(move || {
                let tick = win.borrow_mut().step();
                let (clear, instances, count, new_geometry) = (ff.borrow_mut())(tick);
                // Slide the streamed mesh on the frames that carry new geometry.
                // The `Option` is consumed with `into_iter().for_each` (a
                // combinator, not `if let`/`match`); an empty option iterates zero
                // times.
                new_geometry.into_iter().for_each(|(v, i)| {
                    be.borrow_mut().replace_geometry(STREAM_MESH_ID, &v, &i);
                });
                let batches = vec![(STREAM_MESH_ID, instances, count)];
                let _ = be.borrow().present_frame(clear, &batches);
                let next = f.borrow();
                if let Some(cb) = next.as_ref() {
                    let _ = request_animation_frame(cb);
                }
            }) as Box<dyn FnMut()>));
            let initial = g.borrow();
            if let Some(cb) = initial.as_ref() {
                let _ = request_animation_frame(cb);
            }
        });
        Ok(())
    }
}

impl Default for WindowingApi {
    fn default() -> Self {
        WindowingApi::new()
    }
}

/// Locate the `<canvas>` element by id. wasm32 only.
#[cfg(target_arch = "wasm32")]
fn find_canvas(canvas_id: &str) -> Result<web_sys::HtmlCanvasElement, wasm_bindgen::JsValue> {
    use wasm_bindgen::{JsCast, JsValue};

    let window = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
    let document = window
        .document()
        .ok_or_else(|| JsValue::from_str("no document"))?;
    let element = document
        .get_element_by_id(canvas_id)
        .ok_or_else(|| JsValue::from_str("canvas element not found by id"))?;
    element
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .map_err(|_| JsValue::from_str("element is not an HtmlCanvasElement"))
}

/// Schedule the next animation frame. wasm32 only.
#[cfg(target_arch = "wasm32")]
fn request_animation_frame(
    callback: &wasm_bindgen::closure::Closure<dyn FnMut()>,
) -> Result<(), wasm_bindgen::JsValue> {
    use wasm_bindgen::JsCast;

    let window = web_sys::window().ok_or_else(|| wasm_bindgen::JsValue::from_str("no window"))?;
    window
        .request_animation_frame(callback.as_ref().unchecked_ref())
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_is_unconfigured_at_tick_zero() {
        let w = WindowingApi::new();
        assert!(!w.is_surface_configured());
        assert_eq!(w.surface_width(), None);
        assert_eq!(w.surface_height(), None);
        assert!(w.presentation_request().is_none());
        assert_eq!(w.next_tick(), 0);
        assert_eq!(w.frames_driven(), 0);
        // Default matches new (compared through observable state), and the
        // driver is Debug-printable.
        let d = WindowingApi::default();
        assert_eq!(d.is_surface_configured(), w.is_surface_configured());
        assert_eq!(d.next_tick(), w.next_tick());
        assert!(format!("{w:?}").starts_with("WindowingApi"));
    }

    #[test]
    fn configure_surface_stores_validated_dimensions() {
        let mut w = WindowingApi::new();
        w.configure_surface(800, 600).expect("valid dimensions");
        assert!(w.is_surface_configured());
        assert_eq!(w.surface_width(), Some(800));
        assert_eq!(w.surface_height(), Some(600));
        // The assembled request is exposed for a live backend to consume.
        let request = w.presentation_request().expect("configured");
        assert_eq!(request.descriptor().viewport().physical_width(), 800);
        assert!(request.surface().is_valid());
    }

    #[test]
    fn configure_surface_is_deterministic() {
        // Same inputs reach the same observable state.
        let mut a = WindowingApi::new();
        let mut b = WindowingApi::new();
        a.configure_surface(1280, 720).unwrap();
        b.configure_surface(1280, 720).unwrap();
        assert_eq!(a.surface_width(), b.surface_width());
        assert_eq!(a.surface_height(), b.surface_height());
        assert_eq!(a.is_surface_configured(), b.is_surface_configured());
    }

    #[test]
    fn configure_surface_rejects_zero_dimensions_and_stays_unconfigured() {
        let mut w = WindowingApi::new();
        assert!(w.configure_surface(0, 600).is_err());
        assert!(!w.is_surface_configured());
        assert_eq!(w.surface_width(), None);
    }

    #[test]
    fn step_yields_monotonic_ticks_and_advances_counters() {
        let mut w = WindowingApi::new();
        assert_eq!(w.step(), 0);
        assert_eq!(w.step(), 1);
        assert_eq!(w.step(), 2);
        assert_eq!(w.next_tick(), 3);
        assert_eq!(w.frames_driven(), 3);
    }
}
