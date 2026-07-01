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
use axiom_host::{HostApi, HostLifecycleSignal, HostStepDriver, HostViewport};
use axiom_kernel::{
    BinaryReader, BinaryWriter, DeterministicRng, KernelError, KernelErrorCode, KernelErrorScope,
    KernelResult, Ratio, Reflect, SchemaVersion,
};
use axiom_math::{MathApi, Vec3};
use axiom_render_pipeline::RenderPipelineApi;
use axiom_runtime::{Runtime, RuntimeConfig};
use axiom_scene::SceneApi;
use axiom_webgpu::WebGpuApi;
#[cfg(target_arch = "wasm32")]
use axiom_windowing::WindowingApi;

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

/// The per-frame `tick` family.
mod frame;

/// The live-backend resource exports (mesh streams, material albedos).
mod resources;

/// The default fixed simulation step: 1 ms, matching the engine's slices.
const DEFAULT_STEP_NANOS: u64 = 1_000_000;

/// The wire schema for a full [`RunningApp::snapshot_session`] buffer (the sim
/// state + RNG aggregate). Independent of the inner sim/world schema, so the
/// embedding contract can version without disturbing the scene format.
const SESSION_SCHEMA: SchemaVersion = SchemaVersion::new(1, 0);

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
                    outcome.camera_view_proj(),
                    outcome.mesh_batch_casters(),
                    // The frame's SDF raymarch scene, composited over the meshes
                    // by the live backend.
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
    // Held in full (not just an id) so base colour, albedo texture, and catalog
    // surface (emissive/roughness/opacity) all reach the render path.
    meshes: Vec<(u64, MeshGeometry)>,
    materials: Vec<(u64, Material)>,
    // The live backend's per-instance buffer capacity.
    renderables: usize,
}

impl RunningApp {
    fn realize(app: App) -> Self {
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

        let authored = Self::author(app.setup, aspect);

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
            light_direction: authored.light_direction,
            meshes: authored.meshes,
            materials: authored.materials,
            renderables: authored.renderables,
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

    /// Set the per-frame clear (background) colour. Used by a live reload to
    /// update the background without rebuilding the running app.
    pub fn set_clear_color(&mut self, color: [f32; 4]) {
        self.clear_color = color;
    }

    /// How many renderables the scene draws each frame — the live backend's
    /// per-instance buffer capacity.
    pub fn renderable_count(&self) -> usize {
        self.renderables
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
mod tests {
    use super::*;
    use crate::angle::Angle;
    use crate::camera::{Camera, PerspectiveProjection};
    use crate::color::Color;
    use crate::controller::FirstPersonInput;
    use crate::directional_light::DirectionalLight;
    use crate::player::PlayerInput;
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
        let moved = controller_app().build().tick_with_controls(
            0,
            &[],
            &[FirstPersonInput::new(
                0,
                Vec3::new(0.0, 0.0, -1.0),
                Angle::radians(0.0),
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
    fn snapshot_sim_round_trips_through_restore_into_a_fresh_app() {
        let mut app = controller_app().build();
        (0..3).for_each(|i| {
            app.tick_with_controls(
                i,
                &[],
                &[FirstPersonInput::new(
                    0,
                    Vec3::new(0.0, 0.0, -0.3),
                    Angle::radians(0.2),
                    Angle::radians(0.1),
                )],
            );
        });
        let bytes = app.snapshot_sim();

        let mut forked = controller_app().build();
        forked.restore_sim(&bytes).unwrap();
        assert_eq!(forked.snapshot_sim(), bytes);
        assert!(forked.restore_sim(&[7, 7, 7]).is_err());
    }

    #[test]
    fn snapshot_session_round_trips_the_sim_and_continues_the_rng() {
        let mut app = controller_app().build();
        (0..3).for_each(|i| {
            app.tick_with_controls(
                i,
                &[],
                &[FirstPersonInput::new(
                    0,
                    Vec3::new(0.0, 0.0, -0.3),
                    Angle::radians(0.2),
                    Angle::radians(0.1),
                )],
            );
        });
        let mut rng = DeterministicRng::seeded(0xC0FFEE);
        (0..5).for_each(|_| {
            rng.next_u64();
        });
        let blob = app.snapshot_session(&rng);

        let mut forked = controller_app().build();
        let mut restored_rng = forked.restore_session(&blob).unwrap();
        assert_eq!(forked.snapshot_session(&restored_rng), blob);
        let original: Vec<u64> = (0..8).map(|_| rng.next_u64()).collect();
        let replayed: Vec<u64> = (0..8).map(|_| restored_rng.next_u64()).collect();
        assert_eq!(original, replayed);
    }

    #[test]
    fn restore_session_rejects_an_incompatible_schema() {
        let mut writer = BinaryWriter::new();
        SchemaVersion::new(SESSION_SCHEMA.major() + 1, 0).write_to(&mut writer);
        let mut app = controller_app().build();
        assert_eq!(
            app.restore_session(&writer.into_bytes()).unwrap_err().code(),
            KernelErrorCode::SchemaVersionMismatch
        );
    }

    #[test]
    fn restore_session_rejects_truncation_at_every_prefix() {
        let mut app = controller_app().build();
        (0..3).for_each(|i| {
            app.tick_with_controls(
                i,
                &[],
                &[FirstPersonInput::new(
                    0,
                    Vec3::new(0.0, 0.0, -0.4),
                    Angle::radians(0.3),
                    Angle::radians(0.0),
                )],
            );
        });
        let blob = app.snapshot_session(&DeterministicRng::seeded(7));

        let mut forked = controller_app().build();
        let baseline = forked.snapshot_sim();
        // The only mutation is the final `restore_sim`, so a failed (truncated)
        // restore must leave the target's sim byte-for-byte untouched.
        (0..blob.len()).for_each(|len| {
            assert!(forked.restore_session(&blob[..len]).is_err());
            assert_eq!(
                forked.snapshot_sim(),
                baseline,
                "a failed restore must not mutate the live sim (prefix len {len})"
            );
        });
        // The full buffer restores cleanly and forks the source's sim.
        assert!(forked.restore_session(&blob).is_ok());
        assert_eq!(forked.snapshot_sim(), app.snapshot_sim());
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
                        Angle::radians(0.05),
                    )],
                );
            }
            last
        };
        assert_eq!(drive(), drive());
        assert_ne!(drive().draws(), controller_app().build().tick(0).draws());
    }

    #[test]
    fn tick_with_moves_a_player_cube() {
        let moved = player_app()
            .build()
            .tick_with(0, &[PlayerInput::new(0, Vec3::new(1.0, 0.0, 0.0))]);
        let still = player_app().build().tick_with(0, &[]);
        assert_ne!(
            moved.draws(),
            still.draws(),
            "a player move must change the rendered frame"
        );
    }

    #[test]
    fn tick_with_is_deterministic_and_accumulates() {
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
        assert_ne!(drive(&[0.5, 0.5]).draws(), drive(&[0.5]).draws());
    }

    #[test]
    fn app_builder_is_debug_and_default() {
        let app = App::default().fixed_timestep_nanos(2_000_000);
        assert!(format!("{app:?}").contains("App"));
    }

    #[test]
    fn an_app_with_no_setup_runs_an_empty_simulation() {
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
        // DrawIndexed) + Present.
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
        let mut a = three_cube_app().build();
        let early = a.tick(0);
        let mut later_outcome = early.clone();
        for t in 1..=60 {
            later_outcome = a.tick(t);
        }
        assert_eq!(later_outcome.tick(), 60);
        assert_ne!(early.draws()[0].mvp(), later_outcome.draws()[0].mvp());

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
        assert_eq!(outcome.draws().len(), 0);
        assert!(outcome.recorded());
    }

    #[test]
    fn realized_app_exposes_geometry_and_renderable_count() {
        let app = three_cube_app().build();
        assert_eq!(app.renderable_count(), 3);
        let (vertices, indices) = app.mesh_vertex_stream();
        assert!(!vertices.is_empty());
        // position(3)+normal(3)+uv(2)+colour(4) per vertex.
        assert_eq!(vertices.len() % 12, 0);
        // Per-vertex colour defaults to opaque white (so the per-instance colour
        // stays authoritative: white * instance == instance); floats [8..12].
        assert_eq!(&vertices[8..12], &[1.0, 1.0, 1.0, 1.0]);
        assert!(!indices.is_empty());

        let set = app.mesh_set();
        assert_eq!(set.len(), 1);
        assert_eq!(set[0].1.len() % 12, 0);
        assert_eq!(set[0].1, vertices);
        assert_eq!(set[0].2, indices);

        let mats = app.material_textures();
        assert_eq!(mats.len(), 3);
        assert_eq!((mats[0].1, mats[0].2), (1, 1));
        assert_eq!(mats[0].3, vec![255, 255, 255, 255]);
    }

    #[test]
    fn reauthor_replaces_the_scene_and_renderable_count_in_place() {
        let mut app = player_app().build();
        assert_eq!(app.renderable_count(), 1);
        let before = app.tick(0);

        app.reauthor(|world, meshes, materials| {
            let cube = meshes.add(Mesh::cube());
            for offset_x in [-2.6_f32, 0.0, 2.6] {
                let material = materials.add(Material::lit(Color::WHITE));
                world.spawn((
                    Transform::from_translation(Vec3::new(offset_x, 0.0, 0.0)),
                    Renderable {
                        mesh: cube,
                        material,
                    },
                ));
            }
            world.spawn((
                Transform::from_translation(Vec3::new(0.0, 0.0, 8.0)),
                Camera::perspective(PerspectiveProjection {
                    fov_y: Angle::degrees(60.0),
                    near: Meters::new(0.1).expect("near plane is finite"),
                    far: Meters::new(100.0).expect("far plane is finite"),
                }),
            ));
        });
        assert_eq!(app.renderable_count(), 3);
        let after = app.tick(1);
        assert_eq!(
            after.tick(),
            1,
            "the frame tick keeps advancing across reload"
        );
        assert_ne!(before.draws().len(), after.draws().len());
    }

    #[test]
    fn set_clear_color_changes_the_rendered_clear() {
        let mut app = three_cube_app().build();
        assert_eq!(app.tick(0).clear_color(), [0.05, 0.06, 0.08, 1.0]);
        app.set_clear_color([0.5, 0.25, 0.125, 1.0]);
        assert_eq!(app.tick(1).clear_color(), [0.5, 0.25, 0.125, 1.0]);
    }

    #[test]
    fn an_app_with_no_mesh_has_empty_geometry() {
        let app = App::new().build();
        assert_eq!(app.renderable_count(), 0);
        let (vertices, indices) = app.mesh_vertex_stream();
        assert!(vertices.is_empty());
        assert!(indices.is_empty());
    }
}
