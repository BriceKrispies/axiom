//! Scene authoring expressed as a **startup preparation task**.
//!
//! This is the structural half of the fix for the ordering defect
//! [`RunningApp::realize`] used to carry: it called `Runtime::start()` before it
//! authored the scene, so the runtime reported `Running` for an application
//! whose meshes did not yet exist. Moving the `start()` call further down would
//! have corrected that one line and left the next agent free to move it back.
//!
//! Instead, authoring **is** preparation. [`AuthorTask`] is the first task
//! pushed onto the [`axiom_runtime::PreparationSchedule`] that `realize` hands
//! to `Runtime::prepare`, and `Runtime::start` accepts only `Prepared`. The
//! ordering is therefore no longer a convention a reader has to notice — a
//! `realize` that started before authoring simply could not reach `Running`.
//!
//! The task writes its product into an `Rc<RefCell<Option<_>>>` its constructor
//! captured, because [`axiom_runtime::PreparationTask::prepare`] takes no
//! arguments and returns no data: the runtime owns the *fact* that preparation
//! completed, the caller owns the *data*.

use std::cell::RefCell;
use std::rc::Rc;

use axiom_host::{
    HostAlphaMode, HostApi, HostColorFormat, HostDeviceProfile, HostPowerPreference,
    HostPresentMode, HostPresentationRequest,
};
use axiom_kernel::{KernelApi, Ratio};
use axiom_runtime::{PreparationTask, RuntimeError, RuntimeErrorCode, RuntimeResult};
use axiom_surface::Surface;

use super::{AuthoredScene, RunningApp, SetupFn};

/// The slot an [`AuthorTask`] deposits its realized scene into, shared with the
/// [`RunningApp::realize`] that will build itself from it.
///
/// `Option` rather than a defaultable value is deliberate: a caller that reads
/// the cell before the phase ran finds `None` — an unmistakable absence — rather
/// than a plausible-looking empty scene that would render as a blank world.
pub(super) type AuthoredCell = Rc<RefCell<Option<AuthoredScene>>>;

/// The engine's own preparation task: run the app's setup closure and realize it
/// into a scene, resolved geometry, material colours, and the renderable count.
///
/// Held by the schedule as a `Box<dyn PreparationTask>`, so this type never
/// escapes the umbrella.
pub(super) struct AuthorTask {
    setup: Option<SetupFn>,
    aspect: f32,
    out: AuthoredCell,
}

impl AuthorTask {
    /// A task that will author `setup` at `aspect` and deposit the result in `out`.
    pub(super) fn new(setup: Option<SetupFn>, aspect: f32, out: AuthoredCell) -> Self {
        AuthorTask { setup, aspect, out }
    }
}

impl PreparationTask for AuthorTask {
    /// Author the scene. Infallible today — [`RunningApp::author`] resolves an
    /// absent camera and an absent light to engine defaults rather than
    /// erroring — so this always reports success and the phase advances.
    fn prepare(&mut self) -> RuntimeResult<()> {
        let authored = RunningApp::author(self.setup.take(), self.aspect);
        self.out.borrow_mut().replace(authored);
        Ok(())
    }
}

/// The slot a [`SurfaceTask`] deposits the barrier's product into.
pub(super) type PreparedSurfacesCell = Rc<RefCell<Option<PreparedSurfaces>>>;

/// What the surface barrier produced: the authored set itself, how many distinct
/// programs it lowers to, and what this engine could not honour for it.
#[derive(Debug)]
pub(super) struct PreparedSurfaces {
    /// The authored set, carried forward to the presentation driver.
    pub(super) surfaces: Vec<Surface>,
    /// Distinct compiled programs. Deduplicated by content digest, so two
    /// surfaces that compute the same thing collapse to one and a constant-only
    /// surface costs none at all.
    pub(super) programs: u32,
    /// What the backend cannot honour for this set — empty when everything
    /// lowered. Reported, never silently dropped.
    pub(super) degradations: Vec<axiom_host::FrameFeature>,
}

/// The name the engine's surface-program barrier is scheduled under. It runs
/// after [`AUTHOR_TASK_NAME`](super::AUTHOR_TASK_NAME) and before any task the
/// app contributed.
pub(super) const SURFACE_TASK_NAME: &str = "axiom/surface-programs";

/// **The preparation barrier for authored surfaces**: the one place in an engine
/// app a surface program is compiled.
///
/// It is a `PreparationTask`, so it runs before `RuntimeState::Prepared` — the
/// phase whose stated invariant is that the deterministic simulation cannot
/// advance until preparation has completed. That placement is the whole point.
/// A draw naming a program this barrier did not prepare renders the constant
/// fallback and is *reported*; it is never a lazy mid-frame compile, which on the
/// browser's downlevel GPU path (where WGSL is cross-compiled at pipeline
/// creation) is a concrete stutter rather than a theoretical one.
///
/// Preparation is deterministic: surfaces are deduplicated by content digest and
/// compiled in ascending digest order, so the same set yields the same programs
/// in the same sequence however the app assembled it.
///
/// **On a target where the device arrives late** — the browser, where a GPU
/// device exists only after an asynchronous bind — this half still runs here: it
/// plans, validates, flattens and emits every program's source, and it is what
/// fails loudly when a set needs more distinct programs than the bounded cache
/// holds. The remaining *device* half (binding each generated program to a real
/// pipeline) runs inside `axiom-windowing`'s binder, immediately after the device
/// resolves and strictly before the first frame is recorded. The invariant that
/// matters is intact either way: **nothing compiles inside a frame.**
///
/// An app that authors no surface prepares an empty set — no programs, no
/// degradations, and a frame byte-identical to what it was before any of this
/// existed.
pub(super) struct SurfaceTask {
    surfaces: Vec<Surface>,
    width: u32,
    height: u32,
    out: PreparedSurfacesCell,
}

impl SurfaceTask {
    /// A task that will prepare `surfaces` for a `width` x `height` presentation
    /// and deposit the result in `out`.
    pub(super) fn new(
        surfaces: Vec<Surface>,
        width: u32,
        height: u32,
        out: PreparedSurfacesCell,
    ) -> Self {
        SurfaceTask {
            surfaces,
            width,
            height,
            out,
        }
    }
}

impl PreparationTask for SurfaceTask {
    fn prepare(&mut self) -> RuntimeResult<()> {
        let surfaces = std::mem::take(&mut self.surfaces);
        let mut backend =
            axiom_gpu_backend::GpuBackendApi::new(&barrier_request(self.width, self.height));
        let degradations = backend.surface_degradations(&surfaces);
        let out = &self.out;
        backend
            .prepare_surfaces(&surfaces)
            .map_err(|_| {
                RuntimeError::new(
                    RuntimeErrorCode::SystemFailed,
                    "the app authors more distinct surface programs than the bounded \
                     program cache holds",
                )
            })
            .map(|programs| {
                out.borrow_mut().replace(PreparedSurfaces {
                    surfaces,
                    programs,
                    degradations,
                });
            })
    }
}

/// The validated presentation request the barrier's backend is sized from — the
/// same shape `axiom-windowing` assembles for the live arm. The backend reads
/// only the viewport size from it; no device and no surface is touched here,
/// which is exactly why this half of the barrier runs on native.
fn barrier_request(width: u32, height: u32) -> HostPresentationRequest {
    let host = HostApi::new();
    let kernel = KernelApi::new();
    let viewport = host
        .viewport(
            width,
            height,
            Ratio::new(1.0).expect("unit scale factor is finite"),
        )
        .expect("the app's own window dimensions are valid");
    let target = host
        .presentation_target(&kernel, 1, "axiom-surface-barrier")
        .expect("fixed non-zero target handle and non-empty label are valid");
    let surface = host
        .surface_handle(&kernel, 2)
        .expect("fixed non-zero surface handle is valid");
    let descriptor = host.surface_descriptor(
        viewport,
        HostPresentMode::Fifo,
        HostAlphaMode::Opaque,
        HostColorFormat::Bgra8UnormSrgb,
    );
    let adapter = host.adapter_request(HostPowerPreference::HighPerformance, true);
    let device = host.device_request(true, HostDeviceProfile::Baseline);
    host.presentation_request(target, surface, descriptor, adapter, device)
        .expect("adapter requires a presentation surface, matching the device request")
}
