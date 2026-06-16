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
use axiom_host::{HostApi, HostFrameInput, HostLifecycleSignal, HostStepDriver, HostViewport};
use axiom_kernel::{Radians, Ratio};
use axiom_math::{MathApi, Vec2, Vec3};
use axiom_render_pipeline::RenderPipelineApi;
use axiom_resources::ResourcesApi;
use axiom_runtime::{Runtime, RuntimeConfig};
use axiom_scene::SceneApi;
use axiom_webgpu::WebGpuApi;
// Windowing is the platform presentation backend, used only by the wasm `run`
// terminal entry; native builds drive headlessly via `build` + `tick`.
#[cfg(target_arch = "wasm32")]
use axiom_windowing::WindowingApi;

/// The presentation-target element id the live backend binds to when a
/// [`Window`] does not name one.
#[cfg(target_arch = "wasm32")]
const DEFAULT_SURFACE_ID: &str = "axiom-surface";

use crate::assets::Assets;
use crate::controller::FirstPersonInput;
use crate::default_plugins::DefaultPlugins;
use crate::frame_outcome::{DrawData, FrameOutcome};
use crate::material::Material;
use crate::mesh::Mesh;
use crate::player::PlayerInput;
use crate::scene_commands::SceneCommands;
use crate::window::Window;

/// The default fixed simulation step: 1 ms, matching the engine's slices.
const DEFAULT_STEP_NANOS: u64 = 1_000_000;

/// A user setup callback: populates the asset collections and authors the scene.
type SetupFn = Box<dyn FnOnce(&mut SceneCommands, &mut Assets<Mesh>, &mut Assets<Material>)>;

/// The engine entry point. Configure it with `window`, `fixed_timestep_nanos`,
/// `add_plugins`, and `setup`, then `run` it.
pub struct App {
    window: Window,
    step_nanos: u64,
    render: bool,
    setup: Option<SetupFn>,
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
        }
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

    /// Realize the app: run setup, build the scene + resources, and return a
    /// running app ready to drive frames with [`RunningApp::tick`]. This is the
    /// headless core; the terminal `run` (which owns the per-frame loop) is
    /// built on top of it.
    pub fn build(self) -> RunningApp {
        RunningApp::realize(self)
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
        // Read the surface config through a non-`window`-named binding so this
        // platform-free module never spells the literal platform needles.
        let cfg = &self.window;
        let surface_id = cfg.surface_id().unwrap_or(DEFAULT_SURFACE_ID).to_string();
        let (width, height) = (cfg.width(), cfg.height());

        let mut windowing = WindowingApi::new();
        windowing
            .configure_surface(width, height)
            .expect("surface dimensions are valid");

        let mut running = self.build();
        let (vertices, indices) = running.mesh_vertex_stream();
        let max_instances = running.renderable_count() as u32;
        let _ = windowing.run_web(&surface_id, vertices, indices, max_instances, move |tick| {
            let outcome = running.tick(tick);
            let count = outcome.draws().len() as u32;
            (outcome.clear_color(), outcome.instance_floats(), count)
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
    light_direction: Vec3,
    // Each registered mesh's own resolved geometry (keyed by handle id) and each
    // material's colour. The scene's renderables reference these ids.
    meshes: Vec<(u64, MeshGeometry)>,
    materials: Vec<(u64, [f32; 4])>,
    // How many renderables the scene draws each frame (the live backend's
    // per-instance buffer capacity).
    renderables: usize,
}

impl RunningApp {
    fn realize(app: App) -> Self {
        let math = MathApi::new();
        let host_api = HostApi::new();
        let frame_api = FrameApi::new();

        let mut runtime =
            Runtime::new(RuntimeConfig::new(app.step_nanos).with_diagnostics_enabled(false))
                .expect("fixed step is valid");
        runtime
            .initialize()
            .expect("runtime initialize cannot fail");
        runtime.start().expect("runtime start cannot fail");

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

        // Run user setup: populate assets + author the scene as commands.
        let mut commands = SceneCommands::new(aspect);
        let mut meshes: Assets<Mesh> = Assets::new();
        let mut materials: Assets<Material> = Assets::new();
        if let Some(setup) = app.setup {
            setup(&mut commands, &mut meshes, &mut materials);
        }
        let renderables = commands.renderable_count();

        // Realize the scene; capture the per-frame light direction.
        let mut scene = SceneApi::new();
        let light_direction = commands
            .realize_into(&mut scene, &math)
            .unwrap_or(Vec3::ZERO);

        // Each material asset -> (handle id, colour); each mesh asset -> its own
        // resolved geometry keyed by handle id. The engine resolves a mesh by its
        // kind, so distinct meshes get distinct geometry (today the only built-in
        // kind is the cube).
        let materials: Vec<(u64, [f32; 4])> = materials
            .iter()
            .enumerate()
            .map(|(i, m)| ((i + 1) as u64, m.base_color().to_array()))
            .collect();
        let meshes: Vec<(u64, MeshGeometry)> = meshes
            .iter()
            .enumerate()
            .map(|(i, mesh)| ((i + 1) as u64, mesh_geometry(mesh)))
            .collect();

        RunningApp {
            frame_api,
            pipeline: RenderPipelineApi::new(),
            webgpu: WebGpuApi::new_recording(),
            runtime,
            driver,
            frame_builder,
            viewport,
            scene,
            step_nanos: app.step_nanos,
            render: app.render,
            clear_color: surface.clear_color().to_array(),
            light_direction,
            meshes,
            materials,
            renderables,
        }
    }

    /// How many renderables the scene draws each frame — the live backend's
    /// per-instance buffer capacity.
    pub fn renderable_count(&self) -> usize {
        self.renderables
    }

    /// The first mesh's geometry as the live backend's vertex stream
    /// (interleaved position+normal, 6 floats per vertex) plus its triangle-list
    /// indices. Empty when the app registered no mesh. Plain data the windowing
    /// backend uploads.
    pub fn mesh_vertex_stream(&self) -> (Vec<f32>, Vec<u32>) {
        match self.meshes.first() {
            Some((_, geom)) => {
                let mut vertices = Vec::with_capacity(geom.positions.len() * 6);
                for (p, n) in geom.positions.iter().zip(geom.normals.iter()) {
                    vertices.extend_from_slice(&[p.x, p.y, p.z, n.x, n.y, n.z]);
                }
                (vertices, geom.indices.clone())
            }
            None => (Vec::new(), Vec::new()),
        }
    }

    /// Drive one deterministic frame at `tick`: step the runtime, advance the
    /// scene at the tick, and (when rendering is enabled) submit the frame and
    /// summarise the per-object draws. Browser-free and fully replayable — the
    /// outcome is a pure function of `tick`. The caller (the run loop) owns the
    /// monotonic tick and must pass `0, 1, 2, …` in order.
    pub fn tick(&mut self, tick: u64) -> FrameOutcome {
        self.tick_with_controls(tick, &[], &[])
    }

    /// Drive one deterministic frame at `tick`, applying `inputs` (per-player
    /// move deltas) to the simulation before stepping. The input-free
    /// [`Self::tick`] is `tick_with(tick, &[])`. Like `tick`, the outcome is a
    /// pure function of `tick` and `inputs`, so two peers given the same
    /// confirmed inputs produce byte-identical frames.
    pub fn tick_with(&mut self, tick: u64, inputs: &[PlayerInput]) -> FrameOutcome {
        self.tick_with_controls(tick, inputs, &[])
    }

    /// Drive one deterministic frame at `tick`, applying both per-player move
    /// `inputs` and first-person `controls` to the simulation before stepping.
    /// [`Self::tick`] and [`Self::tick_with`] are the empty-`controls` cases. A
    /// `control` yaws and moves its addressed [`crate::prelude::Controller`] node
    /// along its own facing — the first-person camera path — while `inputs`
    /// translate [`crate::prelude::Player`] nodes in world space. The outcome
    /// stays a pure function of `tick`, `inputs`, and `controls`.
    pub fn tick_with_controls(
        &mut self,
        tick: u64,
        inputs: &[PlayerInput],
        controls: &[FirstPersonInput],
    ) -> FrameOutcome {
        let width = self.viewport.physical_width();
        let height = self.viewport.physical_height();

        let host_input = HostFrameInput::new(tick + 1, self.step_nanos, self.viewport);
        let host_report = self
            .driver
            .drive(&mut self.runtime, host_input)
            .expect("driver inputs are deterministic and valid");
        let mut commands: Vec<_> = inputs
            .iter()
            .enumerate()
            .map(|(i, input)| self.scene.move_command(i as u64, input.player, input.delta))
            .collect();
        for (j, control) in controls.iter().enumerate() {
            let turn = Radians::new(control.turn.as_radians()).expect("authored turn is finite");
            commands.push(self.scene.controller_command(
                (inputs.len() + j) as u64,
                control.index,
                control.move_local,
                turn,
            ));
        }
        let engine_frame = self
            .frame_builder
            .build(&host_report, commands)
            .expect("host report sequence is monotone");
        let frame_ctx = self.frame_api.frame_context(&engine_frame);
        self.scene.advance(tick, &frame_ctx);

        if !self.render {
            return FrameOutcome::simulation_only(tick, self.clear_color);
        }

        let mut frame =
            self.pipeline
                .new_frame(width, height, self.clear_color, self.light_direction);
        for (id, geometry) in &self.meshes {
            self.pipeline.frame_add_mesh(
                &mut frame,
                *id,
                geometry.positions.clone(),
                geometry.normals.clone(),
                geometry.uvs.clone(),
                geometry.indices.clone(),
            );
        }
        for (id, color) in &self.materials {
            self.pipeline.frame_add_material(&mut frame, *id, *color);
        }
        let report = self.pipeline.submit(&frame, &self.scene, &self.webgpu);

        let view_projection = self.pipeline.report_view_projection(&report);
        let draw_count = self.pipeline.report_draw_count(&report);
        let mut draws = Vec::with_capacity(draw_count);
        for i in 0..draw_count {
            let world = self
                .pipeline
                .report_draw_world(&report, i)
                .expect("draw index in range");
            let color = self
                .pipeline
                .report_draw_color(&report, i)
                .expect("draw index in range");
            draws.push(DrawData::new(
                view_projection.multiply(world).as_cols_array(),
                color,
            ));
        }

        FrameOutcome::new(
            tick,
            self.pipeline.report_command_count(&report),
            self.pipeline.report_clear_color(&report),
            draws,
            self.pipeline.report_presented(&report),
            self.pipeline.report_recorded(&report),
        )
    }
}

/// One mesh's resolved geometry: the vertex streams the render pipeline uploads.
#[derive(Debug)]
struct MeshGeometry {
    positions: Vec<Vec3>,
    normals: Vec<Vec3>,
    uvs: Vec<Vec2>,
    indices: Vec<u32>,
}

/// Resolve a mesh description into renderable geometry by its kind. Each kind
/// maps to an engine primitive; the built-in cube is the one kind today, and
/// further primitives are added here. This mapping lives in the umbrella because
/// it bridges the umbrella's `Mesh` enum to an `axiom-resources` primitive —
/// neither module can name the other's types, so the composition is the feature
/// module's job.
fn mesh_geometry(mesh: &Mesh) -> MeshGeometry {
    match mesh {
        Mesh::Cube => cube_geometry(),
    }
}

/// The engine's built-in cube primitive. `axiom-resources` owns the cube mesh
/// data; this only threads it into plain vertex streams the renderer uploads
/// (the resources table is a local, so its un-nameable type never escapes here).
fn cube_geometry() -> MeshGeometry {
    let resources = ResourcesApi::new();
    let mut table = resources.empty_table();
    let id = resources.register_cube_mesh(&mut table).raw();
    let resolved = resources.resolve(&table);
    let vertex_count = resources
        .resolved_mesh_vertex_count(&resolved, id)
        .expect("cube mesh present");
    let mut positions = Vec::with_capacity(vertex_count);
    let mut normals = Vec::with_capacity(vertex_count);
    let mut uvs = Vec::with_capacity(vertex_count);
    for v in 0..vertex_count {
        let p = resources
            .resolved_mesh_position_at(&resolved, id, v)
            .expect("vertex in range");
        let n = resources
            .resolved_mesh_normal_at(&resolved, id, v)
            .expect("vertex in range");
        let u = resources
            .resolved_mesh_uv_at(&resolved, id, v)
            .expect("vertex in range");
        positions.push(Vec3::new(p[0], p[1], p[2]));
        normals.push(Vec3::new(n[0], n[1], n[2]));
        uvs.push(Vec2::new(u[0], u[1]));
    }
    let indices = resources
        .resolved_mesh_indices(&resolved, id)
        .expect("cube mesh present")
        .to_vec();
    MeshGeometry {
        positions,
        normals,
        uvs,
        indices,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::angle::Angle;
    use crate::camera::{Camera, PerspectiveProjection};
    use crate::color::Color;
    use crate::directional_light::DirectionalLight;
    use crate::renderable::Renderable;
    use crate::spin::Spin;
    use axiom_kernel::Meters;
    use axiom_math::Transform;

    /// A linear colour channel from a known-finite authored literal.
    fn ch(value: f32) -> Ratio {
        Ratio::new(value).expect("authored colour channel is finite")
    }

    /// The three-cube demo scene authored against the public App surface.
    fn three_cube_app() -> App {
        App::new()
            .window(Window::new(800, 600).with_clear_color(Color::linear_rgb(
                ch(0.05),
                ch(0.06),
                ch(0.08),
            )))
            .add_plugins(DefaultPlugins)
            .setup(|world, meshes, materials| {
                let cube = meshes.add(Mesh::cube());
                let cubes = [
                    (
                        -2.6,
                        Vec3::UNIT_Y,
                        Color::linear_rgb(ch(0.85), ch(0.25), ch(0.25)),
                    ),
                    (
                        0.0,
                        Vec3::UNIT_X,
                        Color::linear_rgb(ch(0.30), ch(0.80), ch(0.35)),
                    ),
                    (
                        2.6,
                        Vec3::new(1.0, 1.0, 0.0),
                        Color::linear_rgb(ch(0.30), ch(0.50), ch(0.95)),
                    ),
                ];
                for (offset_x, axis, color) in cubes {
                    let material = materials.add(Material::lit(color));
                    world
                        .spawn(Transform::from_translation(Vec3::new(offset_x, 0.0, 0.0)))
                        .with_child((
                            Renderable {
                                mesh: cube,
                                material,
                            },
                            Spin::around(axis).period(360),
                        ));
                }
                world.spawn((
                    Transform::from_translation(Vec3::new(0.0, 0.0, 8.0)),
                    Camera::perspective(PerspectiveProjection {
                        fov_y: Angle::degrees(60.0),
                        near: Meters::new(0.1).expect("authored near plane is finite"),
                        far: Meters::new(100.0).expect("authored far plane is finite"),
                    }),
                ));
                world.spawn((
                    Transform::IDENTITY,
                    DirectionalLight {
                        direction: Vec3::new(0.3, -1.0, 0.4),
                        color: Color::WHITE,
                        intensity: Ratio::new(1.0).expect("authored intensity is finite"),
                    },
                ));
            })
    }

    /// An app with one renderable player cube (player 0) plus a camera, so a
    /// move shows up in the frame's draws.
    fn player_app() -> App {
        use crate::player::Player;
        App::new()
            .window(Window::new(800, 600))
            .add_plugins(DefaultPlugins)
            .setup(|world, meshes, materials| {
                let cube = meshes.add(Mesh::cube());
                let material = materials.add(Material::lit(Color::WHITE));
                world.spawn((
                    Transform::IDENTITY,
                    Renderable {
                        mesh: cube,
                        material,
                    },
                    Player::new(0),
                ));
                world.spawn((
                    Transform::from_translation(Vec3::new(0.0, 0.0, 8.0)),
                    Camera::perspective(PerspectiveProjection {
                        fov_y: Angle::degrees(60.0),
                        near: Meters::new(0.1).expect("near plane is finite"),
                        far: Meters::new(100.0).expect("far plane is finite"),
                    }),
                ));
            })
    }

    /// An app with one renderable cube in front of a first-person camera marked
    /// as controller 0, so turning/moving the camera changes the frame.
    fn controller_app() -> App {
        use crate::controller::Controller;
        App::new()
            .window(Window::new(800, 600))
            .add_plugins(DefaultPlugins)
            .setup(|world, meshes, materials| {
                let cube = meshes.add(Mesh::cube());
                let material = materials.add(Material::lit(Color::WHITE));
                world.spawn((
                    Transform::from_translation(Vec3::new(0.0, 0.0, -5.0)),
                    Renderable {
                        mesh: cube,
                        material,
                    },
                ));
                world.spawn((
                    Transform::IDENTITY,
                    Camera::perspective(PerspectiveProjection {
                        fov_y: Angle::degrees(60.0),
                        near: Meters::new(0.1).expect("near plane is finite"),
                        far: Meters::new(100.0).expect("far plane is finite"),
                    }),
                    Controller::new(0),
                ));
            })
    }

    #[test]
    fn tick_with_controls_moves_the_camera() {
        // Moving the controller camera forward changes the cube's on-screen draw;
        // an input-free tick does not.
        let moved = controller_app().build().tick_with_controls(
            0,
            &[],
            &[FirstPersonInput::new(
                0,
                Vec3::new(0.0, 0.0, -1.0),
                Angle::radians(0.0),
            )],
        );
        let still = controller_app().build().tick_with_controls(0, &[], &[]);
        assert_ne!(
            moved.draws(),
            still.draws(),
            "a camera move must change the rendered frame"
        );
    }

    #[test]
    fn tick_with_controls_turn_changes_the_frame_and_is_deterministic() {
        let drive = || {
            let mut app = controller_app().build();
            let mut last = app.tick(0);
            for i in 0..3 {
                last = app.tick_with_controls(
                    i + 1,
                    &[],
                    &[FirstPersonInput::new(
                        0,
                        Vec3::new(0.0, 0.0, -0.2),
                        Angle::radians(0.15),
                    )],
                );
            }
            last
        };
        // Same controls ⇒ byte-identical frames; turning changes the view from a
        // bare tick.
        assert_eq!(drive(), drive());
        assert_ne!(drive().draws(), controller_app().build().tick(0).draws());
    }

    #[test]
    fn tick_with_moves_a_player_cube() {
        let moved = player_app()
            .build()
            .tick_with(0, &[PlayerInput::new(0, Vec3::new(1.0, 0.0, 0.0))]);
        let still = player_app().build().tick_with(0, &[]);
        // Moving player 0 changes its on-screen draw; an input-free tick does not.
        assert_ne!(
            moved.draws(),
            still.draws(),
            "a player move must change the rendered frame"
        );
    }

    #[test]
    fn tick_with_is_deterministic_and_accumulates() {
        // Same inputs ⇒ byte-identical frames; the move accumulates across ticks.
        let drive = |deltas: &[f32]| {
            let mut app = player_app().build();
            let mut last = app.tick_with(0, &[]);
            for (i, &dx) in deltas.iter().enumerate() {
                last = app.tick_with(
                    i as u64 + 1,
                    &[PlayerInput::new(0, Vec3::new(dx, 0.0, 0.0))],
                );
            }
            last
        };
        assert_eq!(drive(&[0.5, 0.5]), drive(&[0.5, 0.5]));
        // Two +0.5 steps land somewhere a single +0.5 step does not.
        assert_ne!(drive(&[0.5, 0.5]).draws(), drive(&[0.5]).draws());
    }

    #[test]
    fn app_builder_is_debug_and_default() {
        let app = App::default().fixed_timestep_nanos(2_000_000);
        assert!(format!("{app:?}").contains("App"));
    }

    #[test]
    fn an_app_with_no_setup_runs_an_empty_simulation() {
        // Exercises the no-setup path (the `None` arm of the setup callback).
        let mut app = App::new().build();
        let outcome = app.tick(0);
        assert_eq!(outcome.command_count(), 0);
        assert!(outcome.draws().is_empty());
    }

    #[test]
    fn three_cubes_produce_the_deterministic_submission() {
        let mut app = three_cube_app().build();
        assert!(format!("{app:?}").starts_with("RunningApp"));
        let outcome = app.tick(0);
        // Clear + SetCamera + SetPipeline + 3 x (SetMesh + SetMaterial +
        // DrawIndexed) + Present = 13.
        assert_eq!(outcome.command_count(), 13);
        assert_eq!(outcome.draws().len(), 3);
        assert_eq!(outcome.clear_color(), [0.05, 0.06, 0.08, 1.0]);
        assert!(outcome.recorded());
        assert!(!outcome.presented());
        assert_eq!(outcome.tick(), 0);
    }

    #[test]
    fn the_three_cubes_have_distinct_colours() {
        let mut app = three_cube_app().build();
        let draws = app.tick(0);
        let c: Vec<[f32; 4]> = draws.draws().iter().map(|d| d.color()).collect();
        assert_ne!(c[0], c[1]);
        assert_ne!(c[1], c[2]);
        assert_ne!(c[0], c[2]);
    }

    #[test]
    fn a_held_world_evolves_and_replays_deterministically() {
        // Tick 0 differs from a later tick (the cubes spun)...
        let mut a = three_cube_app().build();
        let early = a.tick(0);
        let mut later_outcome = early.clone();
        for t in 1..=60 {
            later_outcome = a.tick(t);
        }
        assert_eq!(later_outcome.tick(), 60);
        assert_ne!(early.draws()[0].mvp(), later_outcome.draws()[0].mvp());

        // ...and a fresh app replays tick 0 byte-equal.
        let mut b = three_cube_app().build();
        assert_eq!(b.tick(0), early);
    }

    #[test]
    fn without_default_plugins_the_app_only_simulates() {
        let mut app = App::new()
            .window(Window::new(100, 100))
            .setup(|world, _meshes, _materials| {
                world.spawn(Transform::IDENTITY);
            })
            .build();
        let outcome = app.tick(0);
        assert_eq!(outcome.command_count(), 0);
        assert!(outcome.draws().is_empty());
        assert!(!outcome.recorded());
    }

    #[test]
    fn a_render_app_with_no_meshes_still_clears_and_presents() {
        // Exercises the empty-geometry branch: render enabled, no renderables.
        let mut app = App::new()
            .window(Window::new(64, 64))
            .add_plugins(DefaultPlugins)
            .setup(|world, _meshes, _materials| {
                world.spawn((
                    Transform::IDENTITY,
                    DirectionalLight {
                        direction: Vec3::new(0.0, -1.0, 0.0),
                        color: Color::WHITE,
                        intensity: Ratio::new(1.0).expect("authored intensity is finite"),
                    },
                ));
            })
            .build();
        let outcome = app.tick(0);
        // Clear + Present, no draws.
        assert_eq!(outcome.draws().len(), 0);
        assert!(outcome.recorded());
    }

    #[test]
    fn realized_app_exposes_geometry_and_renderable_count() {
        let app = three_cube_app().build();
        assert_eq!(app.renderable_count(), 3);
        let (vertices, indices) = app.mesh_vertex_stream();
        assert!(!vertices.is_empty());
        assert_eq!(vertices.len() % 6, 0); // position(3) + normal(3) per vertex
        assert!(!indices.is_empty());
    }

    #[test]
    fn an_app_with_no_mesh_has_empty_geometry() {
        // Exercises the no-mesh arm of mesh_vertex_stream + a zero count.
        let app = App::new().build();
        assert_eq!(app.renderable_count(), 0);
        let (vertices, indices) = app.mesh_vertex_stream();
        assert!(vertices.is_empty());
        assert!(indices.is_empty());
    }
}
