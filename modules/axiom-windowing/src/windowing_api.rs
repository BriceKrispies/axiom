//! The single windowing facade: assemble a presentation request, drive the loop.

use axiom_host::{
    HostAlphaMode, HostApi, HostColorFormat, HostDeviceProfile, HostError, HostPowerPreference,
    HostPresentMode, HostPresentationRequest,
};
use axiom_kernel::{
    KernelApi, KernelError, KernelErrorCode, KernelErrorScope, KernelResult, Ratio, Seconds,
};
use axiom_surface::{Surface, SurfaceInput};

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

/// The scale factor of a surface whose logical pixels **are** its device pixels
/// — the reading [`WindowingApi::configure_surface`] takes when the caller
/// supplies a size and nothing else.
fn unit_scale() -> Ratio {
    Ratio::new(1.0).expect("unit scale factor is finite")
}

/// **The engine's nominal presentation cadence**, in seconds per driven tick.
///
/// The loop here advances exactly one tick per animation frame, so a tick *is* a
/// presented frame and this is the number that turns the driver's own monotonic
/// counter into the frame's presentation time. It is 60 Hz because that is the
/// cadence the engine's frame vocabulary is written in — `Spin::period_ticks` is
/// "ticks per revolution", and every app that animates counts frames. A display
/// running at another rate changes it with
/// [`WindowingApi::set_tick_duration`]; it is deliberately **not** derived from
/// a measured frame interval, because a wall clock in this number would make a
/// replayed tick produce different pixels.
const DEFAULT_TICK_SECONDS: f32 = 1.0 / 60.0;

/// **The authored surfaces this driver presents with**, and how one frame's
/// instance batches find the program each of them draws with.
///
/// This is the deterministic half of the fix for a driver that could not carry a
/// surface at all: the live arm used to hand the GPU backend an empty program
/// slice and a zero clock on every frame, so every authored surface rendered as
/// its constant fallback no matter what the app wrote. Everything that decides
/// those two values is here, on the native-testable side; the wasm arm only
/// spends them.
///
/// **Nothing is compiled here.** The barrier that compiles a surface's program
/// is `axiom_gpu_backend::GpuBackendApi::prepare_surfaces`, run by the app's
/// `axiom_runtime::PreparationTask` before `RuntimeState::Prepared` and again on
/// the bound device the moment the live backend resolves — strictly before the
/// first frame is recorded. A draw naming a program neither pass prepared renders
/// the constant fallback and is *reported*; it is never a mid-frame compile.
#[derive(Debug, Clone)]
pub(crate) struct SurfaceBinding {
    /// The authored set, as the barrier receives it.
    surfaces: Vec<Surface>,
    /// `(material id, surface program)` for every material that names a surface,
    /// ascending by material id. Materials that name none are absent, so an app
    /// that authored no surface carries an empty table and pays nothing.
    material_programs: Vec<(u64, u64)>,
    /// Whether any authored surface reads the frame clock — derived once from
    /// [`Surface::requirements`] when the set is stored, because a still camera
    /// looking at an animating material is a frame that must still be drawn.
    animates: bool,
    /// Seconds per driven tick. See [`DEFAULT_TICK_SECONDS`].
    tick_seconds: Seconds,
}

impl Default for SurfaceBinding {
    fn default() -> Self {
        SurfaceBinding {
            surfaces: Vec::new(),
            material_programs: Vec::new(),
            animates: false,
            tick_seconds: Seconds::finite_or_zero(DEFAULT_TICK_SECONDS),
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
impl SurfaceBinding {
    /// The authored set the preparation barrier compiles — what the live arm
    /// hands `GpuBackendApi::prepare_surfaces` on the bound device.
    pub(crate) fn surfaces(&self) -> &[Surface] {
        &self.surfaces
    }

    /// Whether anything in the set reads the frame clock, and so keeps moving
    /// while every transform on screen holds still.
    pub(crate) fn animates(&self) -> bool {
        self.animates
    }

    /// **The surface program each of `batches` draws with**, in `batches` order —
    /// the slice `axiom_gpu_backend::GpuBackendApi::present_frame_result` takes.
    ///
    /// A batch is one `(mesh, material)` pair, and a material names at most one
    /// surface, so the program is a property of the batch rather than of an
    /// instance inside it — which is why the lane can be recovered from the
    /// material id the batch already carries instead of widening every run
    /// loop's frame tuple.
    ///
    /// An app that authored no surface gets an **empty** slice, not a run of
    /// zeros: the backend reads a missing entry as "no program" already, so the
    /// empty answer is the same pixels and no per-frame allocation at all.
    pub(crate) fn programs_for(&self, batches: &[(u64, u64, Vec<f32>, u32)]) -> Vec<u64> {
        self.material_programs
            .is_empty()
            .then(Vec::new)
            .unwrap_or_else(|| {
                batches
                    .iter()
                    .map(|(_, material_id, _, _)| self.program_of(*material_id))
                    .collect()
            })
    }

    /// The program `material_id` names, or `0` — the engine's built-in fixed
    /// material path — for a material that names no surface.
    fn program_of(&self, material_id: u64) -> u64 {
        self.material_programs
            .binary_search_by_key(&material_id, |(id, _)| *id)
            .ok()
            .and_then(|index| self.material_programs.get(index))
            .map_or(0, |(_, program)| *program)
    }

    /// **The presentation time of frame `tick`** — the clock a time-varying
    /// authored surface samples.
    ///
    /// A pure function of the driver's own monotonic tick, so the same tick
    /// presented twice produces the same displaced geometry byte for byte. The
    /// wall clock this module *does* read (`FrameClock`) feeds the fps read-out
    /// and the adaptive render scale and reaches nothing a pixel depends on.
    pub(crate) fn time_at(&self, tick: u64) -> Seconds {
        Seconds::finite_or_zero(tick as f32 * self.tick_seconds.get())
    }
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
    // The adaptive render scale the live loop applies before each present.
    // Written by the app through `render_scale_control`, read once per frame by
    // the run loop. Shared for the same reason `bound_backend` is: every
    // `run_web_*` consumes the driver, so a caller can only reach it through a
    // view taken before that move.
    render_scale: std::rc::Rc<std::cell::Cell<axiom_host::RenderScale>>,
    // The authored surfaces this driver presents with, plus the material->program
    // table a frame's batches are resolved through and the clock a time-varying
    // surface samples. Read once when a presenter binds and carried into the loop
    // with it; see `SurfaceBinding`.
    surfaces: SurfaceBinding,
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
            render_scale: std::rc::Rc::new(std::cell::Cell::new(
                axiom_host::RenderScale::FULL,
            )),
            surfaces: SurfaceBinding::default(),
        }
    }

    /// **Hand the driver the app's authored surface set** — the one it presents
    /// with, and the one the live arm compiles onto the bound device.
    ///
    /// Before this existed the loop could not carry a surface at all: its GPU arm
    /// passed an empty program slice and a zero clock on every frame, so an app
    /// presenting through `App::run` rendered **every** authored surface as its
    /// constant fallback whatever it had written, and no amount of app-side care
    /// changed that. The only entry that carried surfaces to real pixels took a
    /// packet, and nothing in the engine's own presentation stack walked it.
    ///
    /// The set is what a preparation barrier compiles, so hand it over **before**
    /// a run loop starts. Nothing is compiled by this call: it stores the set,
    /// and derives from `axiom_surface::Surface::requirements` whether anything
    /// in it reads the frame clock — which is what stops the idle-frame gate from
    /// holding a moving material still.
    ///
    /// Pair it with [`Self::set_material_programs`]: that says which material
    /// draws with which of these, and the two are joined by the surface's own
    /// content digest, so they cannot be put out of step by hand.
    pub fn set_surfaces(&mut self, surfaces: Vec<Surface>) {
        self.surfaces.animates = surfaces
            .iter()
            .any(|surface| surface.requirements().inputs().contains(SurfaceInput::TIME));
        self.surfaces.surfaces = surfaces;
    }

    /// The `(material id, surface program)` table a frame's `(mesh, material)`
    /// batches are resolved through — the appearance program each material names,
    /// as `axiom_surface::Surface::digest`.
    ///
    /// A batch is one `(mesh, material)` pair and a material names at most one
    /// surface, so the program is a per-batch value the driver can recover from
    /// the material id the batch already carries. That is why carrying surfaces
    /// did not have to widen every run loop's per-frame tuple.
    ///
    /// Materials that name no surface may be omitted (their program is `0`, the
    /// built-in fixed material path). Order does not matter — the table is sorted
    /// here, so a caller cannot get the lookup wrong by handing it over unsorted.
    pub fn set_material_programs(&mut self, mut programs: Vec<(u64, u64)>) {
        programs.sort_unstable_by_key(|(material_id, _)| *material_id);
        self.surfaces.material_programs = programs;
    }

    /// How many authored surfaces this driver holds.
    pub fn surface_count(&self) -> usize {
        self.surfaces.surfaces().len()
    }

    /// Set the **seconds one driven tick represents** — the cadence that turns
    /// this driver's monotonic tick into [`Self::surface_time`].
    ///
    /// The default is 1/60 s, the cadence the engine's frame vocabulary is
    /// written in. Set it when the loop is driven at another rate, so a
    /// time-varying surface animates at the speed the app authored rather than
    /// at the speed the engine assumed. It is a [`Seconds`] and not a measured
    /// interval on purpose: a wall clock here would make a replayed tick produce
    /// different pixels.
    pub fn set_tick_duration(&mut self, duration: Seconds) {
        self.surfaces.tick_seconds = duration;
    }

    /// The seconds one driven tick represents.
    pub fn tick_duration(&self) -> Seconds {
        self.surfaces.tick_seconds
    }

    /// **The presentation time of frame `tick`** — the clock a time-varying
    /// authored surface samples, and the only sanctioned route by which time
    /// enters an `axiom_field::FieldGraph`.
    ///
    /// A pure function of the tick, so replaying a tick reproduces its pixels
    /// exactly. The wall clock this module does read (the frame-cadence
    /// accumulator) feeds the fps read-out and the adaptive render scale, and
    /// reaches nothing a pixel depends on.
    pub fn surface_time(&self, tick: u64) -> Seconds {
        self.surfaces.time_at(tick)
    }

    /// Assemble and store the validated presentation request for a
    /// `width` x `height` surface, on the mobile-first
    /// [`HostDeviceProfile::Baseline`] device tier. **No browser objects are
    /// touched** — this is pure host-owned data, so it runs and is tested on
    /// native exactly as it will on the web. Fails (leaving the driver
    /// unconfigured) when the host rejects the viewport dimensions.
    pub fn configure_surface(&mut self, width: u32, height: u32) -> KernelResult<()> {
        self.configure_surface_with_profile(width, height, HostDeviceProfile::Baseline)
    }

    /// [`Self::configure_surface`], choosing the **device tier** the surface is
    /// presented under instead of taking the mobile-first default.
    ///
    /// The tier is a property of the surface you are configuring, so it is an
    /// argument here rather than a setter: there is no order in which an app can
    /// ask for a tier and silently have it ignored because the request was
    /// already assembled.
    ///
    /// [`HostDeviceProfile::Baseline`] is the mobile budget — one render sample
    /// per surface pixel, a capped render dimension, the smaller shadow atlas —
    /// and is what [`Self::configure_surface`] keeps giving every existing
    /// caller. [`HostDeviceProfile::ExtendedLimits`] is the opt-up: a larger
    /// shadow atlas and a supersampled render target
    /// ([`HostDeviceProfile::render_supersample`]), which is the engine's only
    /// geometric anti-aliasing. An app whose content is thin, high-contrast,
    /// receding geometry — lane markings, wires, railings — has nothing else to
    /// reach for: at one sample per pixel those edges stair-step, and no
    /// material, light or camera change can alter a sampling rate.
    pub fn configure_surface_with_profile(
        &mut self,
        width: u32,
        height: u32,
        profile: HostDeviceProfile,
    ) -> KernelResult<()> {
        self.configure(width, height, unit_scale(), profile)
    }

    /// [`Self::configure_surface_with_profile`], for a surface whose box is
    /// measured in **logical (device-independent) pixels** at a known **device
    /// pixel ratio** — the shape every real presentation surface actually has.
    ///
    /// This is the honest constructor, and the other two are the special case
    /// `device_pixel_ratio = 1`. A caller that only ever had
    /// [`Self::configure_surface`] had to pick *one* number for a surface that
    /// has two: the box it is laid out in, and the device pixels inside that
    /// box. Passing the device-pixel count as if it were the logical size makes
    /// the host's scale factor a lie; passing the logical size makes the render
    /// target smaller than the display. Either way the surface the engine
    /// configures is not the surface the display shows, and the frame is
    /// resampled — anisotropically, if the two aspects also disagree — on its
    /// way to the screen. The host layer has modelled logical size + scale
    /// factor since [`axiom_host::HostViewport`] existed; this is the windowing
    /// facade finally offering it.
    ///
    /// A non-positive ratio is an error, not a silent fallback: the surface stays
    /// unconfigured and the caller is told, rather than the engine quietly
    /// rendering at a scale the platform did not report.
    ///
    /// The scale is a [`Ratio`] rather than an `f32` because this is public
    /// engine surface, where a naked float is banned — a bare number here does
    /// not say whether it is a ratio, a percentage or a pixel count, and the
    /// caller would have to guess. It also moves the non-finite case out of
    /// reach entirely: `Ratio::new` rejects NaN and infinity at construction, so
    /// this function cannot be *called* with one, where before it accepted one
    /// and returned an error. Non-positive is still checked downstream, by the
    /// host viewport, because zero and negative are finite and so are a
    /// windowing question rather than a kernel one.
    pub fn configure_surface_with_scale(
        &mut self,
        logical_width: u32,
        logical_height: u32,
        device_pixel_ratio: Ratio,
        profile: HostDeviceProfile,
    ) -> KernelResult<()> {
        self.configure(logical_width, logical_height, device_pixel_ratio, profile)
    }

    /// Assemble and store the validated request for a logical `width` x `height`
    /// surface at `scale`. The single place the presentation request is built.
    fn configure(
        &mut self,
        width: u32,
        height: u32,
        scale: Ratio,
        profile: HostDeviceProfile,
    ) -> KernelResult<()> {
        let host = HostApi::new();
        let kernel = KernelApi::new();

        // The one genuinely fallible step with caller-supplied data: the host
        // rejects a zero/oversized viewport or a non-positive scale. The
        // remaining steps use fixed, valid constants and so cannot fail
        // (documented at each site). The success arm builds and stores the
        // request; on the viewport error we return it and leave the surface
        // unconfigured.
        host.viewport(width, height, scale)
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
                let device = host.device_request(true, profile);
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

    /// The configured surface's **device pixel ratio** — the factor between the
    /// box it is laid out in and the device pixels inside it — if any.
    ///
    /// Reported rather than assumed. A caller that sizes anything against the
    /// surface (a camera aspect, a screen-space overlay, a raster budget) needs
    /// to know which of the two sizes it is holding, and the ratio is the only
    /// thing that tells it apart. Always `1` for a surface configured through
    /// [`Self::configure_surface`], which has no second size to report.
    pub fn surface_scale_factor(&self) -> Option<Ratio> {
        self.surface
            .as_ref()
            .map(|r| r.descriptor().viewport().scale_factor())
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

    /// A control the app calls per frame to set the **render scale**: the
    /// fraction of the device tier's render size the 3D scene is rendered at
    /// before the present resolve.
    ///
    /// Taken *before* the driver is consumed by a run loop, for exactly the
    /// reason [`Self::observe_bound_backend`] is: after `run_web_*` there is no
    /// driver left to ask. Pair it with [`axiom_host::RenderScaleController`],
    /// which turns measured frame durations into a scale without reading a clock
    /// of its own — the app's platform edge already measures the frame, and this
    /// is where that measurement earns its keep.
    ///
    /// Setting the same scale repeatedly is free (the backend compares against
    /// the size in use), so the intended shape is to call it unconditionally
    /// every frame rather than tracking changes in the app.
    pub fn render_scale_control(&self) -> impl Fn(axiom_host::RenderScale) + 'static {
        let cell = self.render_scale.clone();
        move |scale| cell.set(scale)
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

    /// A surface whose displacement is driven by `FieldOp::Time` — the one thing
    /// that makes a scene keep moving while every transform on it holds still.
    fn windy() -> Surface {
        use axiom_field::{FieldBuilder, FieldId, FieldOp};
        use axiom_surface::{SurfaceBuilder, SurfaceChannel};
        let (builder, clock) = FieldBuilder::new(FieldId::of_name("windowing/wind"), 1).push(
            FieldOp::Time,
            Vec::new(),
            Vec::new(),
        );
        let (builder, node) = builder.push(
            FieldOp::Compose,
            vec![axiom_recipe::Param::int(3)],
            vec![clock, clock, clock],
        );
        SurfaceBuilder::new()
            .field(SurfaceChannel::Displacement, builder.build(node))
            .build()
            .expect("a vec3 field is a legal displacement")
    }

    /// A surface whose every channel is a plain constant — authored, real, and
    /// reading no clock.
    fn still() -> Surface {
        axiom_surface::SurfaceBuilder::new()
            .build()
            .expect("the default surface is legal")
    }

    /// One `(mesh, material)` batch of one instance.
    fn batch(mesh_id: u64, material_id: u64) -> (u64, u64, Vec<f32>, u32) {
        (mesh_id, material_id, vec![0.0_f32; 40], 1)
    }

    /// **A driver that has been handed no surface carries none** — and carries
    /// them byte-identically to how it did before it could carry any: an empty
    /// program slice, not a run of zeros, so no existing app pays an allocation
    /// per frame for a lane it never uses.
    #[test]
    fn a_driver_with_no_authored_surface_resolves_no_programs_at_all() {
        let driver = WindowingApi::new();
        assert_eq!(driver.surface_count(), 0);
        assert!(driver.surfaces.programs_for(&[batch(1, 1), batch(2, 2)]).is_empty());
        assert!(!driver.surfaces.animates());
        assert!(driver.surfaces.surfaces().is_empty());
    }

    /// **The whole of G12, in one assertion pair**: the driver now holds the
    /// authored set the barrier compiles, and resolves each batch to the program
    /// its material names — where before it passed an empty slice on every frame
    /// and every authored surface rendered as its constant fallback.
    ///
    /// The join is by the surface's own content digest, which is what
    /// `Material::from_surface` puts on the material, so the table and the set
    /// cannot be put out of step by hand.
    #[test]
    fn a_driver_resolves_each_batch_to_the_program_its_material_names() {
        let mut driver = WindowingApi::new();
        let wind = windy();
        let plain = still();
        let wind_program = wind.digest().raw();
        let plain_program = plain.digest().raw();
        assert_ne!(wind_program, plain_program);

        driver.set_surfaces(vec![wind, plain]);
        // Handed unsorted and with a gap — material 2 names no surface at all.
        driver.set_material_programs(vec![(7, plain_program), (1, wind_program)]);
        assert_eq!(driver.surface_count(), 2);

        assert_eq!(
            driver
                .surfaces
                .programs_for(&[batch(10, 7), batch(11, 1), batch(12, 2)]),
            vec![plain_program, wind_program, 0],
            "a material that names no surface takes the built-in fixed path"
        );
        // The order the table was handed over in cannot change the answer.
        assert_eq!(driver.surfaces.program_of(1), wind_program);
        assert_eq!(driver.surfaces.program_of(7), plain_program);
        assert_eq!(driver.surfaces.program_of(9_999), 0);
    }

    /// **A surface that reads the frame clock keeps the loop drawing.** The idle
    /// gate compares one frame's packet against the last presented one, and a
    /// packet cannot carry a material's internal clock — so a wind material on a
    /// parked camera would freeze mid-gust if this were not derived from the
    /// surface's own requirements.
    #[test]
    fn a_time_reading_surface_marks_the_binding_as_animating() {
        let mut still_driver = WindowingApi::new();
        still_driver.set_surfaces(vec![still()]);
        assert!(
            !still_driver.surfaces.animates(),
            "a constant-only surface never changes on its own"
        );

        let mut windy_driver = WindowingApi::new();
        windy_driver.set_surfaces(vec![still(), windy()]);
        assert!(
            windy_driver.surfaces.animates(),
            "one clock-reading surface in the set is enough"
        );

        // And re-authoring back to a still set takes the mark away again — the
        // flag is derived from the set, never accumulated across sets.
        windy_driver.set_surfaces(vec![still()]);
        assert!(!windy_driver.surfaces.animates());
    }

    /// **Surface time is the engine clock, and only the engine clock.** Tick zero
    /// is time zero, tick N is N cadences later, and replaying a tick reproduces
    /// its time exactly — which is what makes a wind-displaced frame replayable.
    #[test]
    fn surface_time_is_a_pure_function_of_the_tick_at_the_declared_cadence() {
        let mut driver = WindowingApi::new();
        // The default cadence is the engine's nominal 60 Hz.
        assert_eq!(driver.tick_duration().get(), 1.0 / 60.0);
        assert_eq!(driver.surface_time(0).get(), 0.0);
        assert_eq!(driver.surface_time(60).get(), 1.0);
        assert_eq!(
            driver.surface_time(90),
            driver.surface_time(90),
            "the same tick is the same time, every replay"
        );

        // A loop driven at another cadence says so, and the clock follows.
        driver.set_tick_duration(Seconds::new(0.5).expect("half a second is finite"));
        assert_eq!(driver.tick_duration().get(), 0.5);
        assert_eq!(driver.surface_time(4).get(), 2.0);
        // Nothing about it reads a wall clock: driving the frame counters does
        // not move the time of a tick that was already asked for.
        let before = driver.surface_time(4);
        (0..10).for_each(|_| {
            driver.step();
        });
        assert_eq!(driver.surface_time(4), before);
    }

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
    fn the_default_surface_is_the_mobile_tier_and_the_opt_up_is_carried() {
        // The plain entry point still asks for the mobile-first tier, so every
        // existing caller's request is unchanged.
        let mut default_tier = WindowingApi::new();
        default_tier.configure_surface(1280, 720).unwrap();
        let baseline = default_tier.presentation_request().expect("configured");
        assert_eq!(
            baseline.device().profile(),
            axiom_host::HostDeviceProfile::Baseline
        );
        // Opting up reaches the request the backend is built from — which is the
        // whole point: the tier decides the shadow atlas AND the supersampled
        // render target, and an app that asked for it must actually get it.
        let mut opted_up = WindowingApi::new();
        opted_up
            .configure_surface_with_profile(
                1280,
                720,
                axiom_host::HostDeviceProfile::ExtendedLimits,
            )
            .expect("valid dimensions");
        let extended = opted_up.presentation_request().expect("configured");
        assert_eq!(
            extended.device().profile(),
            axiom_host::HostDeviceProfile::ExtendedLimits
        );
        // The surface itself is untouched by the tier: same viewport, same
        // composition — only the sampling rate behind it changes.
        assert_eq!(
            extended.descriptor().viewport().physical_width(),
            baseline.descriptor().viewport().physical_width()
        );
        assert_eq!(
            extended.device().profile().render_size(1280, 720),
            (2560, 1440)
        );
    }

    /// A surface configured from a logical box plus a device pixel ratio is
    /// **two** sizes, and the request carries both: the box the display shows,
    /// and the device pixels the backend renders into it. This is the whole
    /// point of the constructor — a 470x836 CSS canvas on a 2x screen is a
    /// 940x1672 render target, and an engine that only ever heard "470x836" (or
    /// only ever heard a declared 1280x720) renders the wrong number of pixels
    /// into the wrong shape.
    #[test]
    fn a_scaled_surface_carries_the_device_pixels_and_the_ratio_that_produced_them() {
        let mut w = WindowingApi::new();
        let dpr = Ratio::new(2.0).expect("2.0 is finite");
        w.configure_surface_with_scale(470, 836, dpr, HostDeviceProfile::ExtendedLimits)
            .expect("a laid-out box at a real device pixel ratio is a valid surface");
        assert_eq!(w.surface_width(), Some(940));
        assert_eq!(w.surface_height(), Some(1672));
        assert_eq!(w.surface_scale_factor().map(Ratio::get), Some(2.0));
        let viewport = w
            .presentation_request()
            .expect("configured")
            .descriptor()
            .viewport();
        assert_eq!(viewport.logical_width(), 470);
        assert_eq!(viewport.logical_height(), 836);
    }

    /// The two older entry points are exactly this constructor at ratio 1, and
    /// they report that ratio rather than leaving it unknowable.
    #[test]
    fn an_unscaled_surface_reports_a_unit_ratio_and_matches_the_scaled_constructor() {
        let mut declared = WindowingApi::new();
        declared.configure_surface(800, 600).expect("valid");
        assert_eq!(declared.surface_scale_factor().map(Ratio::get), Some(1.0));

        let mut scaled = WindowingApi::new();
        scaled
            .configure_surface_with_scale(
                800,
                600,
                Ratio::new(1.0).expect("1.0 is finite"),
                HostDeviceProfile::Baseline,
            )
            .expect("valid");
        assert_eq!(declared.presentation_request(), scaled.presentation_request());
    }

    /// Nothing reports a scale factor before a surface exists.
    #[test]
    fn an_unconfigured_driver_has_no_scale_factor() {
        assert_eq!(WindowingApi::new().surface_scale_factor(), None);
    }

    /// A ratio the platform could not report honestly is an error, not a
    /// silently substituted `1.0`: a surface whose scale is a guess is exactly
    /// the defect this constructor exists to remove.
    ///
    /// The two halves are now enforced in two different places, and that split is
    /// the point of taking a [`Ratio`] rather than an `f32`. **Non-finite** is
    /// unrepresentable: `Ratio::new` rejects NaN and infinity, so the argument
    /// cannot be built and the call cannot be made — checked here at the
    /// constructor, because there is no longer a way to check it at the call.
    /// **Non-positive** is finite and therefore a perfectly good `Ratio`, so it
    /// stays this module's job and is still rejected downstream by the host
    /// viewport, leaving the surface unconfigured.
    #[test]
    fn a_non_finite_or_non_positive_device_pixel_ratio_leaves_the_surface_unconfigured() {
        assert!(Ratio::new(f32::NAN).is_err(), "a NaN scale never becomes a Ratio");
        assert!(
            Ratio::new(f32::INFINITY).is_err(),
            "nor does an infinite one"
        );

        let mut zero = WindowingApi::new();
        assert!(zero
            .configure_surface_with_scale(
                470,
                836,
                Ratio::new(0.0).expect("0.0 is finite, and finite is all a Ratio promises"),
                HostDeviceProfile::Baseline,
            )
            .is_err());
        assert!(!zero.is_surface_configured());
    }

    #[test]
    fn configure_surface_with_profile_rejects_zero_dimensions_too() {
        let mut w = WindowingApi::new();
        assert!(w
            .configure_surface_with_profile(
                800,
                0,
                axiom_host::HostDeviceProfile::ExtendedLimits
            )
            .is_err());
        assert!(!w.is_surface_configured());
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
