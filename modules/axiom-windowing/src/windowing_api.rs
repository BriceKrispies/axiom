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
/// in, replayable state out — no browser or GPU object lives here. Two
/// `WindowingApi`s driven with the same calls reach the same observable state.
#[derive(Debug)]
pub struct WindowingApi {
    surface: Option<HostPresentationRequest>,
    next_tick: u64,
    frames_driven: u64,
    // The real GPU binding, present only once initialised on wasm32. Its
    // absence is what "not ready" means; native never has one.
    #[cfg(target_arch = "wasm32")]
    live: Option<crate::live_gpu_binding::LiveGpuBinding>,
}

impl WindowingApi {
    /// A fresh driver: no surface configured, loop at tick 0.
    pub fn new() -> Self {
        WindowingApi {
            surface: None,
            next_tick: 0,
            frames_driven: 0,
            #[cfg(target_arch = "wasm32")]
            live: None,
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
        // valid constants and so cannot fail (documented at each site).
        let viewport = host
            .viewport(
                width,
                height,
                Ratio::new(1.0).expect("unit scale factor is finite"),
            )
            .map_err(host_to_kernel)?;
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
        Ok(())
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

    /// Whether a live GPU binding is initialised and could present real pixels.
    /// Always `false` on native (there is no GPU): the run loop simulates but
    /// does not present. On wasm32 it is `true` once [`Self::initialize_live`]
    /// has succeeded.
    pub fn binding_is_ready(&self) -> bool {
        #[cfg(target_arch = "wasm32")]
        {
            return self.live.is_some();
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            false
        }
    }

    /// Present one frame: the engine's per-cube instance floats
    /// (`[mvp(16), colour(4)]` each) and a clear colour. Returns whether real
    /// pixels were drawn — always `false` on native (headless), and on wasm32
    /// `true` when a live binding rendered the frame. This is the uniform seam
    /// `App::run` drives on both targets.
    pub fn present_frame(
        &self,
        clear_color: [f32; 4],
        instances: &[f32],
        instance_count: u32,
    ) -> bool {
        #[cfg(target_arch = "wasm32")]
        {
            if let Some(live) = &self.live {
                return live
                    .render_frame(instances, instance_count, clear_color)
                    .is_ok();
            }
            false
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = (clear_color, instances, instance_count);
            false
        }
    }

    /// Initialise the real wgpu binding from a canvas and the engine's cube
    /// geometry (interleaved position+normal `vertices`, triangle-list
    /// `indices`). wasm32 only; on success later [`Self::present_frame`] calls
    /// draw real pixels. On failure the binding stays absent (not ready).
    #[cfg(target_arch = "wasm32")]
    pub async fn initialize_live(
        &mut self,
        canvas: web_sys::HtmlCanvasElement,
        vertices: &[f32],
        indices: &[u32],
        max_instances: u32,
    ) -> Result<(), wasm_bindgen::JsValue> {
        let request = self
            .surface
            .as_ref()
            .ok_or_else(|| wasm_bindgen::JsValue::from_str("no surface configured"))?;
        let width = request.descriptor().viewport().physical_width();
        let height = request.descriptor().viewport().physical_height();
        let binding = crate::live_gpu_binding::LiveGpuBinding::initialize(
            canvas,
            width,
            height,
            vertices,
            indices,
            max_instances,
        )
        .await?;
        self.live = Some(binding);
        Ok(())
    }

    /// Drive the terminal web run loop. Initialise the live binding from the
    /// canvas (looked up by id) and the engine's cube geometry, then present one
    /// frame per `requestAnimationFrame`: each frame the loop owns the monotonic
    /// tick ([`Self::step`]), hands it to `frame_fn`, and presents the plain
    /// draw data it returns — `(clear_color, [mvp(16), colour(4)] per cube,
    /// count)`. wasm32 only; consumes the driver into the loop. If init fails,
    /// nothing presents (the loop never starts).
    #[cfg(target_arch = "wasm32")]
    pub fn run_web<F>(
        self,
        canvas_id: &str,
        vertices: Vec<f32>,
        indices: Vec<u32>,
        max_instances: u32,
        frame_fn: F,
    ) -> Result<(), wasm_bindgen::JsValue>
    where
        F: FnMut(u64) -> ([f32; 4], Vec<f32>, u32) + 'static,
    {
        use std::cell::RefCell;
        use std::rc::Rc;
        use wasm_bindgen::closure::Closure;

        let canvas = find_canvas(canvas_id)?;
        let frame_fn = Rc::new(RefCell::new(frame_fn));
        let mut windowing = self;

        wasm_bindgen_futures::spawn_local(async move {
            if windowing
                .initialize_live(canvas, &vertices, &indices, max_instances)
                .await
                .is_err()
            {
                return;
            }
            let windowing = Rc::new(RefCell::new(windowing));
            let f: Rc<RefCell<Option<Closure<dyn FnMut()>>>> = Rc::new(RefCell::new(None));
            let g = f.clone();
            let win = windowing.clone();
            let ff = frame_fn.clone();
            *g.borrow_mut() = Some(Closure::wrap(Box::new(move || {
                let tick = win.borrow_mut().step();
                let (clear, instances, count) = (ff.borrow_mut())(tick);
                let _ = win.borrow().present_frame(clear, &instances, count);
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

    let window =
        web_sys::window().ok_or_else(|| wasm_bindgen::JsValue::from_str("no window"))?;
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

    #[test]
    fn native_never_presents_real_pixels() {
        // On native there is no GPU binding: the loop simulates but never
        // presents, so the headless contract is "not ready, present is a no-op".
        let mut w = WindowingApi::new();
        w.configure_surface(640, 480).unwrap();
        assert!(!w.binding_is_ready());
        let instances = [0.0_f32; 20]; // one cube: mvp(16) + colour(4)
        assert!(!w.present_frame([0.1, 0.2, 0.3, 1.0], &instances, 1));
    }
}
