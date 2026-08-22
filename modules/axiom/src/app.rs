//! `App`: the engine entry point an app builds and runs.
//!
//! [`App::build`] realizes the builder into a [`RunningApp`] — the headless core
//! that composes runtime stepping, the host frame boundary, the scene,
//! resources, and the render pipeline into one deterministic per-frame outcome
//! via [`RunningApp::tick`]. [`App::run`] is the terminal entry built on top: it
//! configures the surface and drives the per-frame loop through the windowing
//! backend (the `requestAnimationFrame` loop on the web). Nothing here touches a
//! platform surface or a wall clock — the platform loop lives in `axiom-windowing`.

use axiom_frame::{FrameApi, FrameBuilder};
use axiom_host::{
    FrameAmbient, FrameDepthFog, FramePostProcess, HostApi, HostLifecycleSignal, HostStepDriver,
    HostViewport,
};
use axiom_kernel::{
    BinaryReader, BinaryWriter, DeterministicRng, KernelError, KernelErrorCode, KernelErrorScope,
    KernelResult, Ratio, Reflect, SchemaVersion,
};
use axiom_math::{MathApi, Vec3};
use axiom_render_pipeline::RenderPipelineApi;
use axiom_runtime::{PreparationSchedule, PreparationTask, Runtime, RuntimeConfig};
use axiom_scene::SceneApi;
use axiom_webgpu::WebGpuApi;
#[cfg(target_arch = "wasm32")]
use axiom_windowing::WindowingApi;
use std::cell::RefCell;
use std::rc::Rc;

/// The presentation-target element id the live backend binds to when a
/// [`Window`] does not name one.
#[cfg(target_arch = "wasm32")]
const DEFAULT_SURFACE_ID: &str = "axiom-surface";

use crate::assets::Assets;
use crate::default_plugins::DefaultPlugins;
use crate::material::Material;
use crate::mesh::Mesh;
use crate::mesh_geometry::{mesh_geometry, MeshGeometry};
use crate::scene_commands::SceneCommands;
use crate::window::Window;

/// The engine's spatial-reasoning queries on [`RunningApp`] (raycast / overlap).
mod queries;

/// Typed component access by `Entity` (`get`/`set`/`query`).
mod components;

/// The dynamic, kind-keyed retained-world surface (`spawn_empty`/`set_dynamic`/
/// `query_dynamic`/`despawn_subtree`/`children_of`) — the app-blind component arm
/// a wasm-boundary game world is built on.
mod dynamic_world;

/// Incremental runtime scene authoring (`add_mesh`/`add_material`/`add_light`/
/// `set_camera`) — growing the live world a piece at a time after the app is
/// running.
mod authoring;
pub use authoring::TextureDataError;

/// The per-frame `tick` family.
mod frame;

/// The running app's per-frame render-look setters (clear colour, hemisphere
/// ambient, depth fog, colour grade) — the "what the frame looks like" knobs,
/// grouped in one small file.
mod render_look;

/// The live-backend resource exports (mesh streams, material albedos).
mod resources;

/// Scene authoring expressed as the engine's own startup preparation task — the
/// structural reason `realize` cannot reach `Running` before the world exists.
mod preparation;
use preparation::{
    AuthorTask, AuthoredCell, PreparedSurfaces, PreparedSurfacesCell, SurfaceTask,
    SURFACE_TASK_NAME,
};

/// The default fixed simulation step: 1 ms, matching the engine's slices.
const DEFAULT_STEP_NANOS: u64 = 1_000_000;

/// The wire schema for a full [`RunningApp::snapshot_session`] buffer (the sim
/// state + RNG aggregate). Independent of the inner sim/world schema, so the
/// embedding contract can version without disturbing the scene format.
const SESSION_SCHEMA: SchemaVersion = SchemaVersion::new(1, 0);

/// A user setup callback: populates the asset collections and authors the scene.
type SetupFn = Box<dyn FnOnce(&mut SceneCommands, &mut Assets<Mesh>, &mut Assets<Material>)>;

/// A user installation callback: finishes authoring against the *realized*
/// world, where the runtime-only registration surface lives. See [`App::install`].
type InstallFn = Box<dyn FnOnce(&mut RunningApp)>;

/// The name the engine's own scene-authoring preparation task is pushed under.
/// It is the first entry in every schedule, so it is also the name a preparation
/// failure would report if authoring itself ever became fallible.
const AUTHOR_TASK_NAME: &str = "axiom/author";

/// The engine entry point. Configure it with `window`, `fixed_timestep_nanos`,
/// `add_plugins`, and `setup`, then `run` it.
pub struct App {
    window: Window,
    step_nanos: u64,
    render: bool,
    setup: Option<SetupFn>,
    // The app's own startup preparation work, in the order `prepare_with` was
    // called. `realize` drains this onto the schedule *after* the engine's own
    // `AuthorTask`, so an app task can never observe an unauthored world.
    preparation: Vec<(&'static str, Box<dyn PreparationTask>)>,
    // The authored appearance surfaces this app's materials name. Compiled by
    // the engine's own barrier during preparation and carried to the presentation
    // driver by `run`; see `App::surfaces`.
    surfaces: Vec<axiom_surface::Surface>,
    // The app's post-realization installation step, run by `build` on the
    // realized world before anything reads it; see `App::install`.
    install: Option<InstallFn>,
}

impl App {
    /// A default app: an 800x600 window, a 1 ms fixed step, rendering disabled
    /// until `add_plugins(DefaultPlugins)`, and no scene.
    pub fn new() -> Self {
        App {
            window: Window::default(),
            step_nanos: DEFAULT_STEP_NANOS,
            render: false,
            setup: None,
            preparation: Vec::new(),
            surfaces: Vec::new(),
            install: None,
        }
    }

    /// **Finish authoring against the realized world**, inside [`Self::build`] —
    /// the hook that lets an app on the ordinary [`Self::run`] loop use the
    /// registration surface that exists only on a [`RunningApp`]:
    /// [`RunningApp::add_mesh_data`] (app-authored geometry),
    /// [`RunningApp::add_texture_data`] (app-authored albedo pixels), and
    /// [`RunningApp::spawn`].
    ///
    /// Without it those three were unreachable from `run`, because `run` calls
    /// `build()` itself and then immediately reads `mesh_set`,
    /// `material_textures` and `renderable_count` to size and fill the live
    /// backend — leaving an app with its own geometry no choice but to abandon
    /// the engine loop and drive `axiom-windowing` by hand. That is why every
    /// author-geometry app in this repository hand-rolls its browser arm. This
    /// closure runs after realization and *before* `build` returns, so the live
    /// backend sees an installed mesh, texture, and node exactly as it sees a
    /// `setup`-authored one.
    ///
    /// `setup` remains the right place for everything it can express; `install`
    /// is for what only the realized world can register. Both run inside
    /// `build`, in that order.
    pub fn install<F>(mut self, install: F) -> Self
    where
        F: FnOnce(&mut RunningApp) + 'static,
    {
        self.install = Some(Box::new(install));
        self
    }

    /// **Declare the authored [`axiom_surface::Surface`]s this app's materials
    /// name**, so the engine compiles them at the preparation barrier and the
    /// presentation loop draws with them.
    ///
    /// A [`Material::from_surface`](crate::prelude::Material::from_surface)
    /// reduces its surface to a content digest, because a `Material` is a `Copy`
    /// per-asset description and a surface owns graphs. That digest is what
    /// travels the whole render chain — but a digest cannot be *compiled*, so the
    /// surfaces themselves have to reach the engine by some route, and this is
    /// it. They are joined back to the materials that name them by the very same
    /// digest, so there is no second list to keep in step by hand: hand over the
    /// surfaces you authored materials from, in any order, and every material
    /// finds its own.
    ///
    /// Without this an app rendering through [`App::run`] draws **every** authored
    /// surface as its constant fallback, whatever it wrote, because the loop has
    /// nothing to compile and nothing to name. An app that authors no surface
    /// calls this never and is entirely unaffected.
    pub fn surfaces(mut self, surfaces: Vec<axiom_surface::Surface>) -> Self {
        self.surfaces = surfaces;
        self
    }

    /// Set the window/viewport configuration.
    pub fn window(mut self, window: Window) -> Self {
        self.window = window;
        self
    }

    /// Set the fixed simulation step in nanoseconds.
    pub fn fixed_timestep_nanos(mut self, nanos: u64) -> Self {
        self.step_nanos = nanos;
        self
    }

    /// Add the standard plugin bundle, enabling the render path.
    pub fn add_plugins(mut self, _: DefaultPlugins) -> Self {
        self.render = true;
        self
    }

    /// Set the scene-authoring setup callback. It receives the scene command
    /// buffer and the mesh/material asset collections to populate.
    pub fn setup<F>(mut self, setup: F) -> Self
    where
        F: FnOnce(&mut SceneCommands, &mut Assets<Mesh>, &mut Assets<Material>) + 'static,
    {
        self.setup = Some(Box::new(setup));
        self
    }

    /// Contribute one startup **preparation task** — expensive, launch-only work
    /// (procedural generation, texture synthesis, mesh construction) that must
    /// run to completion before the simulation is allowed to step.
    ///
    /// Tasks run in the order they are added, and always **after** the engine's
    /// own scene-authoring task: [`RunningApp::realize`] pushes that one first,
    /// so an app task cannot get in front of it. There is deliberately no order
    /// key and no reserved band — a band would rest on every caller honouring a
    /// convention, whereas push order makes precedence structural.
    ///
    /// A task produces data, not entities: `PreparationTask::prepare` takes no
    /// arguments and so can never touch the `RunningApp`. Write the product into
    /// storage the task's own constructor captured (an `Rc<RefCell<Option<_>>>`),
    /// then register it into the scene after [`App::build`] returns.
    ///
    /// If any task returns `Err`, the phase aborts, the runtime becomes
    /// terminally failed, and `build()` panics: a world that was never built is
    /// a composition-time programming error, not a recoverable frame condition.
    pub fn prepare_with(mut self, name: &'static str, task: Box<dyn PreparationTask>) -> Self {
        self.preparation.push((name, task));
        self
    }

    /// Realize the app: run setup, build the scene + resources, and return a
    /// running app ready to drive frames with [`RunningApp::tick`]. This is the
    /// headless core; the terminal `run` (which owns the per-frame loop) is
    /// built on top of it.
    pub fn build(mut self) -> RunningApp {
        let install = self.install.take();
        let mut running = RunningApp::realize(self);
        install
            .into_iter()
            .for_each(|install| install(&mut running));
        running
    }

    /// Run the app on the web: realize the world, configure the surface, and
    /// drive the terminal per-frame loop through `axiom-windowing` — the
    /// `requestAnimationFrame` loop that presents the deterministic cubes through
    /// the live backend. `run` requires a window backend, and today only the web
    /// has one, so it is wasm32-only; native builds drive headlessly via
    /// [`App::build`] + [`RunningApp::tick`]. The umbrella stays platform-free:
    /// it hands windowing a surface-id string and a per-frame closure producing
    /// plain draw data, never a platform type.
    #[cfg(target_arch = "wasm32")]
    pub fn run(self) {
        let cfg = &self.window;
        let surface_id = cfg.surface_id().unwrap_or(DEFAULT_SURFACE_ID).to_string();
        let (width, height) = (cfg.width(), cfg.height());

        let mut windowing = WindowingApi::new();
        windowing
            .configure_surface(width, height)
            .expect("surface dimensions are valid");

        let mut running = self.build();
        let meshes = running.mesh_set();
        let materials = running.material_textures();
        let max_instances = running.renderable_count() as u32;
        // The app-authored render look reaches the live driver, which binds the
        // backend with it. Without this the browser render used the engine default
        // hemisphere and no fog no matter what the app authored, so a night scene
        // came out under a default daylight fill.
        windowing.set_ambient(running.ambient());
        running
            .depth_fog()
            .into_iter()
            .for_each(|fog| windowing.set_depth_fog(fog));
        // **The authored surfaces, carried into the loop.** The barrier above
        // already planned and validated this set inside the preparation phase;
        // the driver compiles it onto the device it binds and resolves every
        // frame's batches through the material table. Before this the loop
        // presented an empty program slice on every frame, so an authored surface
        // could not reach a pixel through `run` at all.
        windowing.set_surfaces(running.surfaces().to_vec());
        windowing.set_material_programs(running.material_surface_programs());
        let _ =
            windowing.run_web_multi(&surface_id, meshes, materials, max_instances, move |tick| {
                let outcome = running.tick(tick);
                let lights = outcome
                    .lights()
                    .iter()
                    .map(|l| (l.kind(), l.vec(), l.color(), l.intensity()))
                    .collect();
                (
                    outcome.clear_color(),
                    lights,
                    outcome.light_view_proj(),
                    outcome.mesh_batches(),
                    // Per-instance caster flags (matching `mesh_batches`' order)
                    // drive the Canvas backend's planar contact shadows.
                    axiom_host::FrameCamera::new(
                        outcome.camera_view(),
                        outcome.camera_projection(),
                        outcome.camera_view_proj(),
                    ),
                    outcome.mesh_batch_casters(),
                    // The frame's SDF raymarch scene, composited over the meshes
                    // by the live backend.
                    outcome.sdf_scene().cloned(),
                )
            });
    }

    /// Run the app as a **backend comparison**: realize the world once and present
    /// every deterministic frame to three surfaces at once, each pinned to a
    /// different backend (WebGPU / WebGL2 / Canvas 2D). This is the no-frame
    /// successor to the old gallery triptych — one instance, one sim, three
    /// renderers — so the panes are always frame-identical. `surface_ids` are the
    /// three presentation element ids (in WebGPU / WebGL2 / Canvas2D order). Like
    /// [`Self::run`] it is wasm32-only (it owns the live present loop) and hands
    /// windowing only plain per-frame draw data, never a platform type.
    #[cfg(target_arch = "wasm32")]
    pub fn run_compare(self, surface_ids: [&str; 3]) {
        let cfg = &self.window;
        let (width, height) = (cfg.width(), cfg.height());

        let mut windowing = WindowingApi::new();
        windowing
            .configure_surface(width, height)
            .expect("surface dimensions are valid");

        let mut running = self.build();
        let meshes = running.mesh_set();
        let materials = running.material_textures();
        let max_instances = running.renderable_count() as u32;
        // The app-authored render look reaches the live driver, which binds the
        // backend with it. Without this the browser render used the engine default
        // hemisphere and no fog no matter what the app authored, so a night scene
        // came out under a default daylight fill.
        windowing.set_ambient(running.ambient());
        running
            .depth_fog()
            .into_iter()
            .for_each(|fog| windowing.set_depth_fog(fog));
        // Every pane compares the same authored materials, not three different
        // constant fallbacks.
        windowing.set_surfaces(running.surfaces().to_vec());
        windowing.set_material_programs(running.material_surface_programs());
        let _ =
            windowing.run_web_compare(surface_ids, meshes, materials, max_instances, move |tick| {
                let outcome = running.tick(tick);
                let lights = outcome
                    .lights()
                    .iter()
                    .map(|l| (l.kind(), l.vec(), l.color(), l.intensity()))
                    .collect();
                (
                    outcome.clear_color(),
                    lights,
                    outcome.light_view_proj(),
                    outcome.mesh_batches(),
                    axiom_host::FrameCamera::new(
                        outcome.camera_view(),
                        outcome.camera_projection(),
                        outcome.camera_view_proj(),
                    ),
                    outcome.mesh_batch_casters(),
                    outcome.sdf_scene().cloned(),
                )
            });
    }
}

impl Default for App {
    fn default() -> Self {
        App::new()
    }
}

impl std::fmt::Debug for App {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("App")
            .field("window", &self.window)
            .field("step_nanos", &self.step_nanos)
            .field("render", &self.render)
            .field("has_setup", &self.setup.is_some())
            .field("has_install", &self.install.is_some())
            // The surfaces themselves are graphs; the count is what a reader
            // checking a builder by eye actually wants.
            .field("surfaces", &self.surfaces.len())
            // `Box<dyn PreparationTask>` is not `Debug`; the names are the only
            // part of a task the builder can honestly show.
            .field(
                "preparation",
                &self
                    .preparation
                    .iter()
                    .map(|(name, _)| *name)
                    .collect::<Vec<_>>(),
            )
            .finish()
    }
}

/// A realized app: the durable world plus the per-frame engine machinery. Drive
/// it with [`Self::tick`]; each call advances exactly one deterministic frame.
#[derive(Debug)]
pub struct RunningApp {
    frame_api: FrameApi,
    pipeline: RenderPipelineApi,
    webgpu: WebGpuApi,
    runtime: Runtime,
    driver: HostStepDriver,
    frame_builder: FrameBuilder,
    viewport: HostViewport,
    scene: SceneApi,
    step_nanos: u64,
    render: bool,
    clear_color: [f32; 4],
    // The frame's hemisphere ambient (sky/ground fill), authored by the app and
    // carried onto every `FrameOutcome`. Defaults to the engine hemisphere so an
    // app that never sets it renders exactly as before.
    ambient: FrameAmbient,
    // The frame's atmospheric depth fog, authored by the app and carried onto every
    // `FrameOutcome`, so the GPU shader's fog term and the Canvas 2D fog post-pass
    // read the same numbers. `None` leaves each backend on its own prior default, so
    // an app that never sets one is unchanged.
    depth_fog: Option<FrameDepthFog>,
    sky: Option<axiom_host::FrameSky>,
    /// The frame's two-band indirect fill, authored by the app.
    indirect: Option<axiom_host::FrameIndirect>,
    bloom: Option<axiom_host::FrameBloom>,
    // The frame's tonemap/colour grade (exposure/white-balance/contrast/
    // saturation), authored by the app and carried onto every `FrameOutcome` so
    // both the offscreen capture and the live present arm grade identically.
    // `None` presents untonemapped, so an app that never sets one is unchanged.
    postprocess: Option<FramePostProcess>,
    light_direction: Vec3,
    // Held in full (not just an id) so base colour, albedo texture, and catalog
    // surface (emissive/roughness/opacity) all reach the render path.
    meshes: Vec<(u64, MeshGeometry)>,
    materials: Vec<(u64, Material)>,
    // App-authored raw-pixel albedo textures `(id, width, height, RGBA8)`,
    // registered at runtime via `add_texture_data` and resolved by
    // `material_textures` when a material references one. The setup closure cannot
    // register these, so this starts empty and grows only at runtime.
    custom_textures: Vec<(u64, u32, u32, Vec<u8>)>,
    // The live backend's per-instance buffer capacity.
    renderables: usize,
    // Per-frame skinned draws the app queued (bake-once meshes deformed by a joint
    // palette). Filled during authoring via `submit_skinned_draw` and drained into
    // the frame outcome each render, so it never accumulates across frames.
    pending_skinned: Vec<PendingSkinned>,
    // What the surface barrier produced during preparation: the authored surface
    // set itself (carried to the presentation driver), the program count, and what
    // this engine could not honour for it. Empty for the overwhelming majority of
    // apps, which author no surface at all.
    surfaces: PreparedSurfaces,
}

/// A skinned draw the app queued this frame: the mesh + material to draw, the tint
/// colour, its world transform (column-major), and the joint-matrix palette
/// (column-major) that deforms it. Drained into the frame outcome each render.
#[derive(Debug)]
pub(crate) struct PendingSkinned {
    pub(crate) mesh_id: u64,
    pub(crate) material_id: u64,
    pub(crate) color: [f32; 4],
    pub(crate) world: [f32; 16],
    pub(crate) palette: Vec<[f32; 16]>,
}

impl RunningApp {
    /// Realize a built [`App`] into a running one: construct the engine
    /// machinery, run the startup **preparation phase**, and only then start the
    /// runtime.
    ///
    /// The lifecycle here is `initialize → prepare → start`, and the ordering is
    /// structural rather than incidental. Scene authoring is not a step that
    /// happens to sit before `start()`; it *is* the first task on the
    /// preparation schedule. Because `Runtime::start` accepts only `Prepared`,
    /// a `realize` that tried to start before authoring could not reach
    /// `Running` at all — it would panic on the `start` expectation. This
    /// replaces an earlier shape in which `start()` ran first and the runtime
    /// reported `Running` for an application whose meshes did not yet exist.
    fn realize(app: App) -> Self {
        let host_api = HostApi::new();
        let frame_api = FrameApi::new();

        let mut runtime =
            Runtime::new(RuntimeConfig::new(app.step_nanos).with_diagnostics_enabled(false))
                .expect("fixed step is valid");
        runtime
            .initialize()
            .expect("runtime initialize cannot fail");

        let boundary_config = host_api
            .boundary_config(app.step_nanos, 1)
            .expect("max-steps-per-frame = 1 is valid");
        let mut driver = host_api.step_driver(boundary_config);
        driver.apply_lifecycle_signal(HostLifecycleSignal::Started);
        let frame_builder = frame_api.frame_builder(app.step_nanos);

        let surface = app.window;
        let viewport = host_api
            .viewport(
                surface.width(),
                surface.height(),
                Ratio::new(1.0).expect("unit scale factor is finite"),
            )
            .expect("surface dimensions are valid");
        let aspect = surface.width() as f32 / surface.height() as f32;

        // The preparation phase. The engine's own authoring goes on first, then
        // the app's contributed tasks in the order `prepare_with` was called —
        // so no app task can observe a world that has not been authored yet.
        let authored_cell: AuthoredCell = Rc::new(RefCell::new(None));
        let prepared_surfaces: PreparedSurfacesCell = Rc::new(RefCell::new(None));
        let mut schedule = PreparationSchedule::new();
        schedule.push(
            AUTHOR_TASK_NAME,
            Box::new(AuthorTask::new(
                app.setup,
                aspect,
                Rc::clone(&authored_cell),
            )),
        );
        // The surface barrier — the one place an engine app compiles a surface
        // program, and it is here, inside the preparation phase, rather than on
        // the frame that first draws with it. An app that authored no surface
        // prepares an empty set and pays nothing.
        schedule.push(
            SURFACE_TASK_NAME,
            Box::new(SurfaceTask::new(
                app.surfaces,
                surface.width(),
                surface.height(),
                Rc::clone(&prepared_surfaces),
            )),
        );
        app.preparation
            .into_iter()
            .for_each(|(name, task)| schedule.push(name, task));

        // A preparation failure is a composition-time programming error — the
        // world was never built — so it is not swallowed and stepped over. The
        // runtime is terminally `Failed` here and `start` could not succeed
        // anyway; panicking names the failing task instead of hiding it.
        runtime.prepare(schedule).expect("app preparation succeeds");
        runtime.start().expect("a prepared runtime starts");

        let authored = authored_cell
            .borrow_mut()
            .take()
            .expect("preparation authored the scene");
        let surfaces = prepared_surfaces
            .borrow_mut()
            .take()
            .expect("preparation compiled the surface set");

        RunningApp {
            frame_api,
            pipeline: RenderPipelineApi::new(),
            webgpu: WebGpuApi::new_recording(),
            runtime,
            driver,
            frame_builder,
            viewport,
            scene: authored.scene,
            step_nanos: app.step_nanos,
            render: app.render,
            clear_color: surface.clear_color().to_array(),
            ambient: FrameAmbient::default_hemisphere(),
            depth_fog: None,
            sky: None,
            indirect: None,
            bloom: None,
            postprocess: None,
            light_direction: authored.light_direction,
            meshes: authored.meshes,
            materials: authored.materials,
            custom_textures: Vec::new(),
            renderables: authored.renderables,
            pending_skinned: Vec::new(),
            surfaces,
        }
    }

    /// Run a setup callback and realize it into the scene + resolved resources.
    /// Shared by [`Self::realize`] (initial build) and [`Self::reauthor`] (live
    /// rebuild): both turn an authoring closure into a fresh scene, the per-frame
    /// light direction, the resolved mesh geometry and material colours, and the
    /// renderable count.
    fn author(setup: Option<SetupFn>, aspect: f32) -> AuthoredScene {
        let math = MathApi::new();
        let mut commands = SceneCommands::new(aspect);
        let mut meshes: Assets<Mesh> = Assets::new();
        let mut materials: Assets<Material> = Assets::new();
        setup
            .into_iter()
            .for_each(|setup| setup(&mut commands, &mut meshes, &mut materials));
        let renderables = commands.renderable_count();

        let mut scene = SceneApi::new();
        let light_direction = commands
            .realize_into(&mut scene, &math)
            .unwrap_or(Vec3::ZERO);
        // Propagate world transforms once at author time so spatial queries
        // answer correctly from the very first frame, before any `tick`.
        scene.update_world_transforms();

        let materials: Vec<(u64, Material)> = materials
            .iter()
            .enumerate()
            .map(|(i, m)| ((i + 1) as u64, *m))
            .collect();
        let meshes: Vec<(u64, MeshGeometry)> = meshes
            .iter()
            .enumerate()
            .map(|(i, mesh)| ((i + 1) as u64, mesh_geometry(mesh)))
            .collect();

        AuthoredScene {
            scene,
            light_direction,
            meshes,
            materials,
            renderables,
        }
    }

    /// Re-author the scene in place while the app keeps running: re-run a setup
    /// closure and replace the scene, light direction, resolved geometry/material
    /// colours, and renderable count, **keeping** the runtime, host driver, frame
    /// builder, and viewport — so the engine frame tick stays monotonic across the
    /// reload (the host driver requires it). This is the write-side dual of
    /// introspection: an external editor hands the engine a new scene description
    /// at a tick boundary and the next frame renders it.
    ///
    /// Mesh *geometry* is not re-uploaded — the live windowing backend's vertex
    /// buffer is fixed at startup. Reauthoring therefore changes instance
    /// transforms, material colours, and the renderable count (bounded by the
    /// instance-buffer capacity the backend was sized with), never the base mesh.
    pub fn reauthor<F>(&mut self, setup: F)
    where
        F: FnOnce(&mut SceneCommands, &mut Assets<Mesh>, &mut Assets<Material>) + 'static,
    {
        let aspect = self.viewport.physical_width() as f32 / self.viewport.physical_height() as f32;
        let authored = Self::author(Some(Box::new(setup)), aspect);
        self.scene = authored.scene;
        self.light_direction = authored.light_direction;
        self.meshes = authored.meshes;
        self.materials = authored.materials;
        self.renderables = authored.renderables;
    }

    /// How many renderables the scene draws each frame — the live backend's
    /// per-instance buffer capacity. Counts what `setup` authored plus every
    /// node [`Self::spawn`] added; a [`Self::despawn`] does not shrink it,
    /// because the GPU instance buffer this sizes is allocated once and never
    /// reallocated mid-run.
    pub fn renderable_count(&self) -> usize {
        self.renderables
    }

    /// The authored surfaces the preparation barrier compiled — what
    /// [`App::surfaces`] declared, ready to hand to a presentation driver.
    pub fn surfaces(&self) -> &[axiom_surface::Surface] {
        &self.surfaces.surfaces
    }

    /// How many **distinct surface programs** the barrier compiled.
    ///
    /// Deduplicated by content digest, so two materials authored from equal
    /// surfaces cost one program and a surface whose every channel is a plain
    /// constant costs none. Assert on it and a variant explosion is a failing
    /// test rather than a slow first frame.
    pub fn surface_program_count(&self) -> u32 {
        self.surfaces.programs
    }

    /// What this engine could not honour for the authored surface set — empty
    /// when everything lowered. Resolved once at the barrier, not per frame, and
    /// reported rather than silently dropped.
    pub fn surface_degradations(&self) -> &[axiom_host::FrameFeature] {
        &self.surfaces.degradations
    }

    /// **The `(material id, surface program)` table a presentation driver
    /// resolves each frame's batches through.**
    ///
    /// A frame reaches the live backend as per-`(mesh, material)` instance
    /// batches, which carry no appearance program of their own — that is exactly
    /// why the engine's own loop used to render every authored surface as its
    /// constant fallback. It does not need one: a material names at most one
    /// surface, so the program is recoverable from the material id the batch
    /// already carries, and this is that recovery table.
    ///
    /// Only materials that name a surface appear; every other material takes the
    /// built-in fixed material path (program `0`), so an app that authored no
    /// surface hands over an empty table and its frames are unchanged.
    pub fn material_surface_programs(&self) -> Vec<(u64, u64)> {
        self.materials
            .iter()
            .map(|(id, material)| (*id, material.surface_program()))
            .filter(|(_, program)| *program != 0)
            .collect()
    }

    /// Serialize the durable simulation state — the scene world (entity identity,
    /// component columns, and the player/controller maps) — to bytes, so a caller
    /// can record it per frame and later fork from a recorded frame. The per-frame
    /// engine machinery (runtime, driver, frame builder) is deliberately excluded:
    /// under continue-forward resume the tick keeps advancing and only the scene
    /// state is restored. Pair with [`Self::restore_sim`].
    pub fn snapshot_sim(&self) -> Vec<u8> {
        self.scene.snapshot_state()
    }

    /// Restore the simulation state from bytes produced by [`Self::snapshot_sim`]
    /// — forking the world to that recorded frame. Live play then resumes from the
    /// restored scene with the tick continuing forward. A truncated or
    /// version-incompatible buffer returns a deterministic error.
    pub fn restore_sim(&mut self, bytes: &[u8]) -> KernelResult<()> {
        self.scene.restore_state(bytes)
    }

    /// Serialize a full **session snapshot** — the durable sim state ([`Self::snapshot_sim`])
    /// *and* the host's deterministic random generator — into one opaque, versioned
    /// buffer. This is the embedding contract an authoritative host stores verbatim
    /// for persistence, room rewind, crash recovery, or an out-of-process worker:
    /// one buffer in, one buffer out. The RNG lives **inside** the blob, so a
    /// restored session continues the identical random sequence (loot, spawns,
    /// crits) rather than diverging. The host owns the generator and hands it in;
    /// [`Self::restore_session`] hands it back. Layout:
    /// `[session schema][length-prefixed sim bytes][rng state]`.
    pub fn snapshot_session(&self, rng: &DeterministicRng) -> Vec<u8> {
        let mut writer = BinaryWriter::new();
        SESSION_SCHEMA.write_to(&mut writer);
        writer.write_byte_slice(&self.snapshot_sim());
        rng.reflect_write(&mut writer);
        writer.into_bytes()
    }

    /// Restore a session from bytes produced by [`Self::snapshot_session`]: the sim
    /// state is forked to the recorded frame and the captured generator is returned
    /// for the host to resume from. A truncated or version-incompatible buffer
    /// returns a deterministic error, never a panic.
    ///
    /// The whole header — schema, the length-prefixed sim slice, *and* the trailing
    /// rng state — is decoded **before** the sim is mutated, so a buffer that is
    /// truncated anywhere fails with the live app left untouched. (The only
    /// mutation, `restore_sim`, is the final step.)
    pub fn restore_session(&mut self, bytes: &[u8]) -> KernelResult<DeterministicRng> {
        let mut reader = BinaryReader::new(bytes);
        SchemaVersion::read_from(&mut reader)
            .and_then(|version| {
                SESSION_SCHEMA
                    .is_compatible_with(version)
                    .then_some(())
                    .ok_or_else(|| {
                        KernelError::new(
                            KernelErrorScope::Binary,
                            KernelErrorCode::SchemaVersionMismatch,
                            "session snapshot schema major version is incompatible",
                        )
                    })
            })
            .and_then(|()| reader.read_byte_slice())
            .and_then(|world_bytes| {
                DeterministicRng::reflect_read(&mut reader).map(|rng| (world_bytes, rng))
            })
            .and_then(|(world_bytes, rng)| self.restore_sim(world_bytes).map(|()| rng))
    }
}

/// The product of running a setup closure: a realized scene plus the resolved
/// resources and counts a [`RunningApp`] holds. Returned by [`RunningApp::author`]
/// and consumed by both the initial build and a live [`RunningApp::reauthor`].
#[derive(Debug)]
struct AuthoredScene {
    scene: SceneApi,
    light_direction: Vec3,
    meshes: Vec<(u64, MeshGeometry)>,
    materials: Vec<(u64, Material)>,
    renderables: usize,
}

#[cfg(test)]
#[path = "app_tests.rs"]
mod tests;
