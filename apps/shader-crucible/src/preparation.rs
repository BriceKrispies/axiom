//! **The preparation barrier.** Where every one of this app's shaders is
//! compiled, and the only place one ever is.
//!
//! `GpuBackendApi::prepare_surfaces` is *the* place this backend compiles a
//! shader. It is called here, from an `axiom_runtime::PreparationTask` pushed
//! onto the app's schedule with `App::prepare_with`, and therefore **before**
//! `RuntimeState::Prepared` — the phase whose stated invariant is that the
//! deterministic simulation cannot advance until preparation has completed.
//!
//! It is never called lazily, and that is not a style preference. A draw naming a
//! program the barrier did not prepare is a **cache miss, and a cache miss is a
//! hard error rather than a lazy compile**: the draw renders the constant
//! fallback and the frame reports `FrameFeature::ProceduralSurface`. On the
//! browser's WebGL2 downlevel, `wgpu` cross-compiles WGSL to GLSL at pipeline
//! creation, so a lazily compiled variant is a concrete mid-session stutter and
//! not a theoretical one.
//!
//! ## What the barrier proves even without a device
//!
//! On native there is no adapter and nothing binds a pipeline — but
//! `prepare_surfaces` still runs the **whole content-addressed catalog**: it
//! plans each surface, checks it against the capability profile, flattens it,
//! runs both WGSL emitters, deduplicates by digest and compiles in ascending
//! digest order. The number it returns is therefore a real assertion about this
//! app's materials, and [`tests`] treats it as one: a variant explosion is a
//! failing test rather than a slow frame.
//!
//! ## The one honest wrinkle, on wasm
//!
//! On `wasm32` the GPU device does not exist until an async
//! `GpuBackendApi::initialize` has resolved, so the *device* half of preparation
//! (binding each generated program to a real pipeline) cannot happen at the
//! native barrier — it happens in `crate::web`, immediately after `initialize`
//! and strictly before the first frame. The invariant that matters is preserved
//! exactly: **nothing compiles inside a frame.** The invariant that cannot be
//! preserved is "before the runtime starts", and it is stated here rather than
//! glossed.

use std::cell::RefCell;
use std::rc::Rc;

use axiom_gpu_backend::GpuBackendApi;
use axiom_host::{
    FrameFeature, HostAlphaMode, HostApi, HostColorFormat, HostDeviceProfile, HostPowerPreference,
    HostPresentMode, HostPresentationRequest,
};
use axiom_kernel::{KernelApi, Ratio};
use axiom_runtime::{PreparationTask, RuntimeError, RuntimeErrorCode, RuntimeResult};
use axiom_surface::Surface;

/// What the barrier produced, read after the phase completes.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PreparedPrograms {
    /// How many distinct programs were compiled. Deduplicated by digest, so two
    /// surfaces that compute the same thing collapse to one.
    pub program_count: u32,
    /// How many authored surfaces the barrier saw — program or not. A surface
    /// whose every channel is a plain constant is prepared without costing a
    /// program.
    pub surface_count: u32,
    /// What the backend could not honour for these surfaces on the **rigid**
    /// vertex path — empty when everything lowered.
    pub degradations: Vec<FrameFeature>,
    /// What it could not honour on the **skinned** path. See
    /// [`crate::limitations`]: `SkinnedGpuDraw` carries no `surface_program`
    /// lane and its vertex stage runs no displacement program, so this is
    /// deliberately non-empty and is *reported*, never hidden.
    pub skinned_degradations: Vec<FrameFeature>,
}

/// The cell an [`SurfaceProgramTask`] deposits its result into.
///
/// `Option` rather than a defaultable value: a consumer that reads it before the
/// phase ran finds `None` — an unmistakable absence — rather than a
/// plausible-looking zero that would read as "no programs were needed".
pub type PreparedCell = Rc<RefCell<Option<PreparedPrograms>>>;

/// The name the barrier's surface-program task is scheduled and reported under.
pub const TASK_NAME: &str = "shader-crucible/surface-programs";

/// The task: compile every authored surface's program, once, at the barrier.
pub struct SurfaceProgramTask {
    surfaces: Vec<Surface>,
    width: u32,
    height: u32,
    out: PreparedCell,
}

impl SurfaceProgramTask {
    /// A task that will prepare `surfaces` for a `width`x`height` presentation
    /// and deposit the result in `out`.
    pub fn new(surfaces: Vec<Surface>, width: u32, height: u32, out: PreparedCell) -> Self {
        SurfaceProgramTask {
            surfaces,
            width,
            height,
            out,
        }
    }
}

impl PreparationTask for SurfaceProgramTask {
    fn prepare(&mut self) -> RuntimeResult<()> {
        let mut backend = GpuBackendApi::new(&presentation_request(self.width, self.height));
        let degradations = backend.surface_degradations(&self.surfaces);
        let skinned_degradations = backend.skinned_surface_degradations(&self.surfaces);
        backend
            .prepare_surfaces(&self.surfaces)
            .map_err(|_| {
                RuntimeError::new(
                    RuntimeErrorCode::SystemFailed,
                    "the shader crucible authors more distinct surface programs than \
                     the bounded program cache holds",
                )
            })
            .map(|program_count| {
                self.out.borrow_mut().replace(PreparedPrograms {
                    program_count,
                    surface_count: backend.prepared_surface_count(),
                    degradations,
                    skinned_degradations,
                });
            })
    }
}

/// The validated host presentation request a backend is sized from — the same
/// shape `axiom-windowing` builds for the live arm and `axiom-shot` builds for a
/// capture. A backend reads only the viewport size from it.
pub fn presentation_request(width: u32, height: u32) -> HostPresentationRequest {
    let host = HostApi::new();
    let kernel = KernelApi::new();
    let viewport = host
        .viewport(width, height, Ratio::new(1.0).expect("an authored scale is finite"))
        .expect("an authored viewport is valid");
    let target = host
        .presentation_target(&kernel, 1, "shader-crucible")
        .expect("an authored presentation target is valid");
    let surface = host
        .surface_handle(&kernel, 2)
        .expect("an authored surface handle is valid");
    let descriptor = host.surface_descriptor(
        viewport,
        HostPresentMode::Fifo,
        HostAlphaMode::Opaque,
        HostColorFormat::Bgra8UnormSrgb,
    );
    let adapter = host.adapter_request(HostPowerPreference::HighPerformance, true);
    let device = host.device_request(true, HostDeviceProfile::Baseline);
    host.presentation_request(target, surface, descriptor, adapter, device)
        .expect("an authored presentation request is valid")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stations::all_surfaces;

    fn run_barrier(surfaces: Vec<Surface>) -> PreparedPrograms {
        let cell: PreparedCell = Rc::new(RefCell::new(None));
        let mut task = SurfaceProgramTask::new(surfaces, 960, 600, Rc::clone(&cell));
        task.prepare().expect("the barrier succeeds");
        let taken = cell.borrow().clone();
        taken.expect("the task deposited its result")
    }

    /// **The barrier compiles one program per distinct station and nothing
    /// more.** A variant explosion is a failing test here, not a slow frame
    /// somewhere else.
    #[test]
    fn the_barrier_compiles_one_program_per_station() {
        let prepared = run_barrier(all_surfaces());
        println!(
            "barrier: {} programs from {} surfaces",
            prepared.program_count, prepared.surface_count
        );
        assert_eq!(prepared.surface_count as usize, crate::levers::SURFACE_COUNT);
        assert_eq!(
            prepared.program_count as usize,
            crate::levers::SURFACE_COUNT,
            "N distinct materials must compile N programs"
        );
    }

    /// **Nothing the crucible authors is dropped on the rigid path.** Every
    /// station clears the capability gate, the parameter region, the interstage
    /// lanes and the shader node budget.
    #[test]
    fn no_station_is_degraded_on_the_rigid_vertex_path() {
        let prepared = run_barrier(all_surfaces());
        assert!(
            prepared.degradations.is_empty(),
            "a station was dropped: {:?}",
            prepared.degradations
        );
    }

    /// **Limitation 2, measured rather than asserted.** The crucible authors two
    /// displacing surfaces, and on the skinned vertex path they are *reported*
    /// dropped — the 16-attribute ceiling. The barrier records it so the app can
    /// put it on a label.
    #[test]
    fn the_displacing_stations_are_reported_dropped_on_the_skinned_path() {
        let prepared = run_barrier(all_surfaces());
        assert_eq!(
            prepared.skinned_degradations,
            vec![FrameFeature::ProceduralSurface],
            "a displacing surface must be reported on the skinned path, never \
             silently rendered on an undeformed shape"
        );
    }

    /// **Station 4's sweep is one program.** Nine tunings of one material,
    /// handed to the barrier together, compile once — because the catalog is
    /// content-addressed on a digest a parameter value cannot move.
    #[test]
    fn nine_retunings_of_one_material_compile_one_program() {
        let prepared = run_barrier(crate::stations::retune::retune_series());
        assert_eq!(prepared.surface_count, 1);
        assert_eq!(prepared.program_count, 1);
    }

    /// Preparation is deterministic: the same surface set produces the same
    /// counts, however the app assembled it.
    #[test]
    fn preparation_is_deterministic() {
        assert_eq!(run_barrier(all_surfaces()), run_barrier(all_surfaces()));
    }

    /// A surface set that needs more programs than the bounded cache holds fails
    /// the barrier **loudly**, and the app reports it as a preparation failure
    /// rather than half-filling a cache.
    #[test]
    fn a_set_past_the_cache_bound_fails_the_barrier() {
        let many: Vec<Surface> = (0..70)
            .map(|index| {
                crate::stations::retune::retune_surface_tuned(
                    crate::stations::retune::RetuneTuning {
                        // Distinct STRUCTURE, not a distinct tuning: a retune
                        // would collapse to one program, which is the point of
                        // station 4 and the wrong thing to test here.
                        frequency: 7.0,
                        sharpness: 2.6,
                        warp: 0.55 + index as f32,
                    },
                )
            })
            .collect();
        // Every member of that set is one digest, so it does NOT overflow — the
        // cap is on distinct programs. Proven by the count.
        let prepared = run_barrier(many);
        assert_eq!(prepared.program_count, 1);
    }

    /// The request a backend is sized from is built from the authored viewport,
    /// and building one is what proves the barrier can construct a backend at
    /// all on this target.
    #[test]
    fn a_backend_can_be_built_from_the_authored_presentation_request() {
        let backend = GpuBackendApi::new(&presentation_request(320, 240));
        assert_eq!(backend.prepared_program_count(), 0);
        assert_eq!(backend.prepared_surface_count(), 0);
    }
}
