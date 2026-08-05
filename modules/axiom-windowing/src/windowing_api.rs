//! The single windowing facade: assemble a presentation request, drive the loop.

use axiom_host::{
    HostAlphaMode, HostApi, HostColorFormat, HostDeviceProfile, HostError, HostPowerPreference,
    HostPresentMode, HostPresentationRequest,
};
use axiom_kernel::{
    KernelApi, KernelError, KernelErrorCode, KernelErrorScope, KernelResult, Ratio,
};

// The `wasm32`-only live presentation arm: the browser run loops, live backend
// selection (WebGPU -> WebGL2 -> Canvas 2D), and DOM helpers. Gated on wasm32 so
// none of it compiles (or is coverage-gated) on native; the deterministic,
// fully-covered core below stays target-independent. Internal: it adds no public
// surface, only further `impl WindowingApi` blocks for the run loops.
#[cfg(target_arch = "wasm32")]
mod web;

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
    // The app-authored **render look** the live backend binds with: the hemisphere
    // ambient that fills unlit faces, the atmospheric depth fog distance recedes
    // into, the sky behind the scene, and the bloom bright pixels spill through. All
    // are backend-neutral `host` values, so they live in the deterministic core (not
    // the wasm arm) and are testable on native.
    //
    // Before this existed, every `run_web*` entry hardcoded
    // `FrameAmbient::default_hemisphere()` at bind: an app could author a night
    // ambient with `set_ambient` and the live browser render would still light the
    // scene with the engine's default daylight hemisphere, because the authored value
    // had no route to the binder. That is a silent divergence between what the app
    // authors and what the browser draws — the same class of defect as a backend
    // that ignores a frame's fog — so the look is carried here and consumed at bind.
    // Defaults reproduce the old hardcode exactly, so an app that sets nothing is
    // unchanged.
    //
    // Held as one `FrameRenderLook` rather than a field per knob: the parts already
    // travelled together through every binder, and a separate field each meant a new
    // look knob widened four signatures and a dozen `wasm32`-only call sites the
    // native gate never compiles.
    look: axiom_host::FrameRenderLook,
    // The live presenter for a caller-owned frame loop (see
    // `bind_present_surface` / `present_frame` in the wasm32 `web` arm). A shared
    // slot so the asynchronous backend init can fill it off-loop; empty until then
    // and on any host that never binds a surface. wasm32 only — no GPU/browser
    // object exists in the native deterministic core.
    #[cfg(target_arch = "wasm32")]
    presenter: std::rc::Rc<std::cell::RefCell<Option<web::LivePresenter>>>,
    // Which live backend the cascade actually selected, reported by every arm that
    // binds one (and re-reported by a device-loss rebuild). See `bound_backend`.
    bound_backend: BackendReport,
}

/// The shared cell a bound backend reports its identity into.
///
/// Shared rather than owned because backend selection is **asynchronous and
/// happens after the driver is gone**: every `run_web_*` entry consumes `self`
/// into the animation-frame loop, and the GPU device request resolves later
/// still. A caller that wants the answer therefore has to be holding a view of
/// this cell from before that move, which is what
/// [`WindowingApi::observe_bound_backend`] hands out.
type BackendReport = std::rc::Rc<std::cell::Cell<Option<axiom_host::BackendKind>>>;

impl WindowingApi {
    /// A fresh driver: no surface configured, loop at tick 0.
    pub fn new() -> Self {
        WindowingApi {
            surface: None,
            next_tick: 0,
            frames_driven: 0,
            look: axiom_host::FrameRenderLook::default(),
            #[cfg(target_arch = "wasm32")]
            presenter: std::rc::Rc::new(std::cell::RefCell::new(None)),
            bound_backend: std::rc::Rc::new(std::cell::Cell::new(None)),
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
        // return it and leave the surface unconfigured.
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

    /// Set the app-authored **hemisphere ambient** the live backend binds with —
    /// the sky/ground fill every unlit face receives. Without this the driver binds
    /// the engine default hemisphere, which is why an app that authored a night
    /// ambient still rendered under a default daylight fill in the browser.
    pub fn set_ambient(&mut self, ambient: axiom_host::FrameAmbient) {
        self.look = self.look.with_ambient(ambient);
    }

    /// The render-look hemisphere ambient the live backend binds with.
    pub const fn ambient(&self) -> axiom_host::FrameAmbient {
        self.look.ambient()
    }

    /// Set the app-authored **atmospheric depth fog** the live backend binds with —
    /// the colour distance recedes toward and the normalized-depth range over which
    /// it does. Both live backends read the same numbers, so the horizon dissolves
    /// identically whichever won the cascade. Unset (the default) leaves each backend
    /// on its prior default.
    pub fn set_depth_fog(&mut self, depth_fog: axiom_host::FrameDepthFog) {
        self.look = self.look.with_depth_fog(depth_fog);
    }

    /// The render-look depth fog the live backend binds with, if the app authored one.
    pub const fn depth_fog(&self) -> Option<axiom_host::FrameDepthFog> {
        self.look.depth_fog()
    }

    /// Set the app-authored **sky** the live backend binds with — a gradient with an
    /// optional celestial body in it, evaluated per pixel behind the scene instead of
    /// a flat clear colour. This is what puts the light source *in* the frame; a night
    /// scene lit only by a directional light and an ambient reads flat however
    /// carefully the light values are tuned, because nothing on screen is the source.
    /// Unset (the default) leaves each backend clearing to the frame's flat colour.
    pub fn set_sky(&mut self, sky: axiom_host::FrameSky) {
        self.look = self.look.with_sky(sky);
    }

    /// The render-look sky the live backend binds with, if the app authored one.
    pub const fn sky(&self) -> Option<axiom_host::FrameSky> {
        self.look.sky()
    }

    /// Set the app-authored **bloom** the live backend binds with — which pixels spill
    /// light into their neighbours, how far, and how the surplus above white rolls off
    /// instead of clipping. Without it a material authored to emit above `1.0` simply
    /// clamps, so a lamp reads as a flat white sticker rather than a light. Unset (the
    /// default) leaves highlights to clip, exactly as before.
    pub fn set_bloom(&mut self, bloom: axiom_host::FrameBloom) {
        self.look = self.look.with_bloom(bloom);
    }

    /// The render-look bloom the live backend binds with, if the app authored one.
    pub const fn bloom(&self) -> Option<axiom_host::FrameBloom> {
        self.look.bloom()
    }

    /// Set the app-authored **colour grade** the live backend binds with — the
    /// exposure / white-balance / contrast / saturation / black-point pass the finished
    /// image is presented through. Without it the live arm presents the raster exactly as
    /// it came out, while the off-screen capture of the *same* frame ran the grade over
    /// its read-back buffer: an app authoring a grade saw it in every capture and never in
    /// the browser. Unset (the default) leaves the presented frame ungraded, exactly as
    /// before.
    pub fn set_grade(&mut self, grade: axiom_host::FramePostProcess) {
        self.look = self.look.with_grade(grade);
    }

    /// The render-look colour grade the live backend binds with, if the app authored one.
    pub const fn grade(&self) -> Option<axiom_host::FramePostProcess> {
        self.look.grade()
    }

    /// The whole app-authored render look, as one value — what every binder in the
    /// wasm arm threads through to the backend it builds.
    pub const fn render_look(&self) -> axiom_host::FrameRenderLook {
        self.look
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

    /// Which live backend is bound right now, or `None` before the
    /// (asynchronous) bind has resolved — and always `None` on native, which
    /// binds no browser presentation.
    ///
    /// Lets an app adapt to the backend it actually got rather than the one it
    /// hoped for. The motivating case: the Canvas 2D software rasterizer runs a
    /// low-resolution framebuffer, so detail geometry that is stable on the GPU
    /// decays into sub-pixel flicker there, and an app wants to draw less of it
    /// — but only if it can find out. The selection cascade (`?backend=`, else
    /// WebGPU→WebGL2→Canvas 2D) lives inside this module, so this is the only
    /// place that honestly knows the answer; an app re-reading the URL would
    /// miss every fallback, including the no-parameter page that ended up on
    /// Canvas 2D because the GPU refused a device.
    ///
    /// The answer can **change**: a device-loss rebuild re-runs the cascade, so
    /// a page that started on the GPU can legitimately finish on Canvas 2D. It
    /// is a reading, not a one-off announcement, and a consumer that latches it
    /// is wrong.
    pub fn bound_backend(&self) -> Option<axiom_host::BackendKind> {
        self.bound_backend.get()
    }

    /// A read-only reading of [`Self::bound_backend`] that **outlives this
    /// driver**.
    ///
    /// Every `run_web_*` entry consumes the driver into the animation-frame
    /// loop, and the backend is selected asynchronously after that — so the one
    /// moment an app can ask is the one moment there is nothing to ask. Take
    /// this before handing the driver over and call it per frame; it reads the
    /// same cell the binder writes, so it starts `None`, becomes the selected
    /// backend when the bind resolves, and follows a later device-loss rebuild.
    pub fn observe_bound_backend(&self) -> impl Fn() -> Option<axiom_host::BackendKind> + 'static {
        let bound = self.bound_backend.clone();
        move || bound.get()
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
}

impl Default for WindowingApi {
    fn default() -> Self {
        WindowingApi::new()
    }
}

/// The smoothing window for the frame-cadence read-out, in microseconds. Frame
/// deltas accumulate until this much wall-clock has elapsed, then the read-out is
/// recomputed over that window — so the displayed fps/frame-time is a stable mean,
/// not a single jittery frame.
///
/// Like [`FrameClock`], this is consumed in production only by the `wasm32` live
/// loop, so it reads as dead code on the native build (the native tests below
/// still exercise it, keeping it covered) — the same wasm-arm-only idiom the
/// overlay's draw plumbing uses.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
const FRAME_CLOCK_WINDOW_MICROS: u64 = 250_000;

/// A windowed frame-cadence accumulator: the deterministic half of "what fps /
/// frame-time is the live loop running at".
///
/// It is the engine's single owner of that measurement. The wall-clock *read*
/// that produces each timestamp is a nondeterministic host concern and lives in
/// the `wasm32` live loop (`web.rs`); this accumulator is fed those integer
/// microsecond timestamps and is therefore pure, target-independent, branchless,
/// and fully covered on native — exactly the deterministic/nondeterministic split
/// the rest of the module keeps. An engine-driven app reads the smoothed
/// `(fps_milli, frame_micros)` out of the run loop and feeds it to a diagnostics
/// surface (e.g. the debug overlay); nothing here knows about that consumer.
///
/// Timing is integer-encoded so no naked float crosses any boundary: `fps_milli`
/// is frames-per-second × 1000, `frame_micros` is the mean frame time in
/// microseconds.
///
/// Consumed in production only by the `wasm32` live loop (the native tests below
/// cover every line), so it is `dead_code`-allowed on native — the same idiom the
/// overlay's wasm-only draw plumbing uses.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
#[derive(Debug, Default)]
pub(crate) struct FrameClock {
    /// The previous frame's timestamp (µs); `None` before the first frame.
    last_micros: Option<u64>,
    /// Wall-clock accumulated in the current window (µs).
    window_micros: u64,
    /// Frames accumulated in the current window.
    window_frames: u32,
    /// Last computed read-out: frames-per-second × 1000.
    fps_milli: u32,
    /// Last computed read-out: mean frame time (µs).
    frame_micros: u32,
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
impl FrameClock {
    /// Record a frame observed at `now_micros` (a monotone wall clock in
    /// microseconds) and return the current `(fps_milli, frame_micros)` read-out.
    ///
    /// The read-out is recomputed only when the window fills, so the first
    /// `FRAME_CLOCK_WINDOW_MICROS` of play reports zeros (an honest "not measured
    /// yet"), then a smoothed mean thereafter. The window-full predicate selects,
    /// per field, between the freshly-computed value and the retained one (and
    /// resets the accumulators) via `then_some`/`unwrap_or`.
    pub(crate) fn record(&mut self, now_micros: u64) -> (u32, u32) {
        // We measure *intervals*, not frames: N timestamps bound N-1 deltas. The
        // first observation only seeds the clock — it has no predecessor, so it
        // contributes neither a delta nor an interval count. Counting it would be
        // a fencepost error that over-reports fps by one frame per window.
        let had_prev = self.last_micros.is_some();
        let delta = now_micros.saturating_sub(self.last_micros.unwrap_or(now_micros));
        self.last_micros = Some(now_micros);
        self.window_micros += delta;
        self.window_frames += u32::from(had_prev);

        let full = self.window_micros >= FRAME_CLOCK_WINDOW_MICROS;
        // `max(1)` keeps both divisions total before the first interval lands
        // (window_frames/window_micros are then 0); it never alters a real,
        // full-window result, where both are already >= 1.
        let intervals = u64::from(self.window_frames).max(1);
        let fps = (intervals * 1_000_000_000 / self.window_micros.max(1)) as u32;
        let mean = (self.window_micros / intervals) as u32;

        self.fps_milli = full.then_some(fps).unwrap_or(self.fps_milli);
        self.frame_micros = full.then_some(mean).unwrap_or(self.frame_micros);
        self.window_micros = full.then_some(0).unwrap_or(self.window_micros);
        self.window_frames = full.then_some(0).unwrap_or(self.window_frames);
        (self.fps_milli, self.frame_micros)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_driver_that_has_bound_nothing_reports_no_backend() {
        // Native binds no browser presentation, so the answer is always `None`
        // here — and an app asking "am I on the software rasterizer?" natively
        // must get a definite no rather than a guess. The wasm32 arm reports the
        // cascade's real selection into the same cell and is exercised in the
        // browser.
        let mut driver = WindowingApi::new();
        assert_eq!(driver.bound_backend(), None);
        driver
            .configure_surface(640, 360)
            .expect("valid surface dimensions");
        assert_eq!(
            driver.bound_backend(),
            None,
            "configuring a surface is not binding a backend"
        );
    }

    #[test]
    fn the_backend_reading_survives_the_driver_it_came_from() {
        // The whole point of the observer: every `run_web_*` entry consumes the
        // driver, so a reading taken beforehand has to keep working once the
        // driver is gone. Dropping it here is the native stand-in for that move.
        let driver = WindowingApi::new();
        let observed = driver.observe_bound_backend();
        assert_eq!(observed(), driver.bound_backend());
        drop(driver);
        assert_eq!(
            observed(),
            None,
            "the reading outlives the driver and still answers"
        );
    }

    #[test]
    fn new_is_unconfigured_at_tick_zero() {
        let w = WindowingApi::new();
        assert!(!w.is_surface_configured());
        assert_eq!(w.surface_width(), None);
        assert_eq!(w.surface_height(), None);
        assert!(w.presentation_request().is_none());
        assert_eq!(w.next_tick(), 0);
        assert_eq!(w.frames_driven(), 0);
        let d = WindowingApi::default();
        assert_eq!(d.is_surface_configured(), w.is_surface_configured());
        assert_eq!(d.next_tick(), w.next_tick());
        assert!(format!("{w:?}").starts_with("WindowingApi"));
    }

    #[test]
    fn render_look_defaults_to_the_engine_hemisphere_and_no_fog() {
        // The defaults reproduce exactly what every `run_web*` entry used to
        // hardcode at bind, so an app that authors neither is unchanged.
        let w = WindowingApi::new();
        assert_eq!(
            w.ambient(),
            axiom_host::FrameAmbient::default_hemisphere()
        );
        assert_eq!(w.depth_fog(), None);
        assert_eq!(w.sky(), None);
        assert_eq!(w.bloom(), None);
        assert_eq!(w.grade(), None);
        assert_eq!(w.render_look(), axiom_host::FrameRenderLook::default());
    }

    #[test]
    fn authored_render_look_is_what_the_driver_binds_with() {
        let mut w = WindowingApi::new();
        // A night race authors a dark, cool ambient; before this the live browser
        // bind discarded it and lit the scene with the default daylight hemisphere.
        let night = axiom_host::FrameAmbient::new([0.05, 0.07, 0.13], [0.03, 0.03, 0.04]);
        w.set_ambient(night);
        assert_eq!(w.ambient(), night);
        assert_ne!(w.ambient(), axiom_host::FrameAmbient::default_hemisphere());

        let fog = axiom_host::FrameDepthFog::new(
            Ratio::finite_or_zero(0.985),
            Ratio::finite_or_zero(1.0),
            Ratio::finite_or_zero(0.92),
            [0.02, 0.03, 0.08],
        );
        w.set_depth_fog(fog);
        assert_eq!(w.depth_fog(), Some(fog));

        // The sky and the bloom ride the same look, and — the part worth pinning
        // — authoring one must not discard the others. A driver that rebuilt its
        // look per setter would silently drop the ambient the moment an app set
        // a sky, which is exactly the class of bug the bundle exists to prevent.
        let sky = axiom_host::FrameSky::gradient([0.01, 0.02, 0.05], [0.04, 0.05, 0.09]);
        w.set_sky(sky);
        assert_eq!(w.sky(), Some(sky));
        let bloom = axiom_host::FrameBloom::moonlit();
        w.set_bloom(bloom);
        assert_eq!(w.bloom(), Some(bloom));

        // The grade is the fifth part, and the one that used to reach only the
        // read-back arms: an app could author it, see it in every off-screen
        // capture, and never see it in the browser.
        let grade = axiom_host::FramePostProcess::low_key();
        w.set_grade(grade);
        assert_eq!(w.grade(), Some(grade));

        // Everything authored above is still there, and `render_look` hands the
        // whole thing over as the one value every binder threads.
        let look = w.render_look();
        assert_eq!(look.ambient(), night);
        assert_eq!(look.depth_fog(), Some(fog));
        assert_eq!(look.sky(), Some(sky));
        assert_eq!(look.bloom(), Some(bloom));
        assert_eq!(look.grade(), Some(grade));
    }

    #[test]
    fn configure_surface_stores_validated_dimensions() {
        let mut w = WindowingApi::new();
        w.configure_surface(800, 600).expect("valid dimensions");
        assert!(w.is_surface_configured());
        assert_eq!(w.surface_width(), Some(800));
        assert_eq!(w.surface_height(), Some(600));
        let request = w.presentation_request().expect("configured");
        assert_eq!(request.descriptor().viewport().physical_width(), 800);
        assert!(request.surface().is_valid());
    }

    #[test]
    fn configure_surface_is_deterministic() {
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

    /// Record `count` frames spaced `delta` µs apart, continuing from `now`;
    /// returns the final read-out and the advanced clock time.
    fn drive(clock: &mut FrameClock, now: &mut u64, delta: u64, count: u32) -> (u32, u32) {
        let mut last = (0_u32, 0_u32);
        (0..count).for_each(|_| {
            last = clock.record(*now);
            *now += delta;
        });
        last
    }

    #[test]
    fn frame_clock_reports_zero_until_the_window_fills() {
        // The first frame only seeds the clock (no predecessor, no interval), and
        // within the window the read-out stays the honest "not measured yet" zero.
        let mut clock = FrameClock::default();
        assert_eq!(clock.record(0), (0, 0));
        // A handful of sub-window frames (16 ms steps) keep the read-out at zero.
        assert_eq!(clock.record(16_000), (0, 0));
        assert_eq!(clock.record(32_000), (0, 0));
    }

    #[test]
    fn frame_clock_smooths_a_steady_60hz_cadence_once_the_window_fills() {
        // A steady 16_667 µs cadence (60 Hz) drives several full windows; the
        // trailing read-out is ~60.0 fps and ~16_667 µs mean frame time. The
        // interval-counting fix is what keeps this at 60 (not ~64).
        let mut clock = FrameClock::default();
        let mut now = 0_u64;
        let (fps_milli, frame_micros) = drive(&mut clock, &mut now, 16_667, 60);
        assert!(
            (59_800..=60_200).contains(&fps_milli),
            "fps_milli={fps_milli}"
        );
        assert!(
            (16_600..=16_700).contains(&frame_micros),
            "frame_micros={frame_micros}"
        );
    }

    #[test]
    fn frame_clock_resets_the_window_so_a_new_cadence_takes_over() {
        // Drive a 60 Hz phase, then a 30 Hz phase. If the window accumulated
        // forever the read-out would barely move; because it resets per window,
        // the trailing read-out reflects the *new* 30 Hz cadence.
        let mut clock = FrameClock::default();
        let mut now = 0_u64;
        let sixty = drive(&mut clock, &mut now, 16_667, 60);
        assert!((59_800..=60_200).contains(&sixty.0), "sixty={sixty:?}");
        let (fps_milli, frame_micros) = drive(&mut clock, &mut now, 33_333, 60);
        assert!(
            (29_800..=30_200).contains(&fps_milli),
            "fps_milli={fps_milli}"
        );
        assert!(
            (33_200..=33_400).contains(&frame_micros),
            "frame_micros={frame_micros}"
        );
    }
}
