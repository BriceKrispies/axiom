//! Tests for `app`, split into a sibling file so `app.rs` stays under the
//! engine 1000-line file budget. Included via `#[path]` as a child module, so
//! `super` still refers to `app`.

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
use axiom_runtime::{RuntimeError, RuntimeErrorCode, RuntimeResult, RuntimeState};

/// A linear colour channel from a known-finite authored literal.
fn ch(value: f32) -> Ratio {
    Ratio::new(value).expect("authored colour channel is finite")
}

/// A shared log of what ran, in the order it ran — the only way to observe the
/// preparation phase from outside, since a task is handed no engine state.
type Trace = Rc<RefCell<Vec<&'static str>>>;

fn trace() -> Trace {
    Rc::new(RefCell::new(Vec::new()))
}

/// A preparation task that appends its name to a shared trace.
struct TraceTask {
    name: &'static str,
    trace: Trace,
}

impl PreparationTask for TraceTask {
    fn prepare(&mut self) -> RuntimeResult<()> {
        self.trace.borrow_mut().push(self.name);
        Ok(())
    }
}

fn trace_task(name: &'static str, trace: &Trace) -> Box<dyn PreparationTask> {
    Box::new(TraceTask {
        name,
        trace: Rc::clone(trace),
    })
}

/// A preparation task that always fails, so the phase aborts before `start()`.
struct FailingTask;

impl PreparationTask for FailingTask {
    fn prepare(&mut self) -> RuntimeResult<()> {
        Err(RuntimeError::new(
            RuntimeErrorCode::PreparationFailed,
            "intentional test failure",
        ))
    }
}

/// An app whose setup records `"author"` into `trace`, so the engine's own
/// authoring shows up in the same log the app's preparation tasks write to.
fn traced_app(trace: &Trace) -> App {
    let recorder = Rc::clone(trace);
    App::new()
        .window(Window::new(800, 600))
        .add_plugins(DefaultPlugins)
        .setup(move |world, meshes, materials| {
            recorder.borrow_mut().push("author");
            let cube = meshes.add(Mesh::cube());
            let material = materials.add(Material::lit(Color::WHITE));
            world.spawn((
                Transform::IDENTITY,
                Renderable {
                    mesh: cube,
                    material,
                },
            ));
        })
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
        app.restore_session(&writer.into_bytes())
            .unwrap_err()
            .code(),
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
    // A boxed task is not `Debug`, so the builder shows the names it was given —
    // which is exactly the schedule a reader wants to check by eye.
    let named = app.prepare_with("demo/generate", trace_task("demo", &trace()));
    assert!(format!("{named:?}").contains("demo/generate"));
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
    assert_eq!((mats[0].width(), mats[0].height()), (1, 1));
    assert_eq!(mats[0].pixels(), &[255, 255, 255, 255]);
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
fn set_ambient_flows_onto_the_frame_outcome() {
    let mut app = three_cube_app().build();
    // A fresh app carries the engine default hemisphere ambient, and it rides
    // onto the rendered frame's outcome.
    assert_eq!(app.ambient(), FrameAmbient::default_hemisphere());
    assert_eq!(app.tick(0).ambient(), FrameAmbient::default_hemisphere());
    // Authoring a daylight ambient is reflected on both the app and the frame.
    let daylight = FrameAmbient::new([0.66, 0.71, 0.80], [0.45, 0.42, 0.37]);
    app.set_ambient(daylight);
    assert_eq!(app.ambient(), daylight);
    assert_eq!(app.tick(1).ambient(), daylight);
}

#[test]
fn set_depth_fog_flows_onto_the_frame_outcome() {
    let mut app = three_cube_app().build();
    // A fresh app authors no atmosphere, so every backend keeps its prior default.
    assert_eq!(app.depth_fog(), None);
    assert_eq!(app.tick(0).depth_fog(), None);
    // Authoring a night atmosphere is reflected on both the app and the rendered
    // frame, so the GPU fog term and the Canvas 2D fog post-pass read the same
    // numbers on the very next frame.
    let night = axiom_host::FrameDepthFog::new(
        axiom_kernel::Ratio::finite_or_zero(0.985),
        axiom_kernel::Ratio::finite_or_zero(1.0),
        axiom_kernel::Ratio::finite_or_zero(0.9),
        [0.02, 0.03, 0.08],
    );
    app.set_depth_fog(night);
    assert_eq!(app.depth_fog(), Some(night));
    assert_eq!(app.tick(1).depth_fog(), Some(night));
}

/// A sky is what gives a frame a light source that is actually *in* it, so it
/// has to reach the backend the same way the ambient and the fog do.
#[test]
fn set_sky_flows_onto_the_frame_outcome() {
    let mut app = three_cube_app().build();
    // A fresh app authors no sky, so every backend keeps its flat clear colour
    // and renders byte-identically to before skies existed.
    assert_eq!(app.sky(), None);
    assert_eq!(app.tick(0).sky(), None);

    let moonlit = axiom_host::FrameSky::gradient([0.02, 0.03, 0.06], [0.06, 0.08, 0.13])
        .with_body(
        [0.0, 0.18, 1.0],
        axiom_kernel::Radians::finite_or_zero(0.055),
        [0.85, 0.90, 1.0],
        axiom_kernel::Ratio::finite_or_zero(220.0),
        axiom_kernel::Ratio::finite_or_zero(0.45),
    );
    app.set_sky(moonlit);
    assert_eq!(app.sky(), Some(moonlit));
    assert_eq!(app.tick(1).sky(), Some(moonlit), "and it is on the very next frame");
}

/// Bloom is what makes an emissive above `1.0` mean anything, so it travels the
/// same road.
#[test]
fn set_bloom_flows_onto_the_frame_outcome() {
    let mut app = three_cube_app().build();
    assert_eq!(app.bloom(), None);
    assert_eq!(app.tick(0).bloom(), None, "highlights clip until asked otherwise");

    let glow = axiom_host::FrameBloom::moonlit();
    app.set_bloom(glow);
    assert_eq!(app.bloom(), Some(glow));
    assert_eq!(app.tick(1).bloom(), Some(glow));
    // The two render-look knobs are independent: authoring bloom does not
    // conjure a sky, and neither disturbs the ambient.
    assert_eq!(app.sky(), None);
    assert_eq!(app.ambient(), axiom_host::FrameAmbient::default_hemisphere());
}

#[test]
fn set_postprocess_flows_onto_the_frame_outcome() {
    let mut app = three_cube_app().build();
    // A fresh app authors no grade, so the rendered frame presents untonemapped.
    assert_eq!(app.postprocess(), None);
    assert_eq!(app.tick(0).postprocess(), None);
    // Authoring a grade is reflected on both the app and the rendered frame, so
    // the offscreen capture and the live present arm grade identically.
    let grade = FramePostProcess::cinematic();
    app.set_postprocess(grade);
    assert_eq!(app.postprocess(), Some(grade));
    assert_eq!(app.tick(1).postprocess(), Some(grade));
}

#[test]
fn an_app_with_no_mesh_has_empty_geometry() {
    let app = App::new().build();
    assert_eq!(app.renderable_count(), 0);
    let (vertices, indices) = app.mesh_vertex_stream();
    assert!(vertices.is_empty());
    assert!(indices.is_empty());
}

#[test]
fn realize_leaves_the_runtime_running_with_an_authored_scene() {
    let app = three_cube_app().build();
    assert_eq!(
        app.runtime.state(),
        RuntimeState::Running,
        "a realized app is running"
    );
    // …and it is running over a world that actually exists: the whole point of
    // the reorder is that `Running` and "the meshes exist" became inseparable.
    assert_eq!(app.renderable_count(), 3);
    assert_eq!(app.meshes.len(), 1, "the cube mesh was registered");
    assert_eq!(app.materials.len(), 3, "one material per cube");
}

#[test]
fn the_author_task_runs_before_start() {
    // A failing task aborts the phase, so the runtime never becomes `Prepared`
    // and `start()` is never reached — `realize` panics instead of returning.
    // The trace still shows the engine's own authoring ran, which is precisely
    // the claim: authoring happens strictly before `start()`.
    let log = trace();
    let app = traced_app(&log).prepare_with("test/fails", Box::new(FailingTask));
    let realized = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| app.build()));
    assert!(
        realized.is_err(),
        "a failed preparation must never yield a running app"
    );
    assert_eq!(
        *log.borrow(),
        vec!["author"],
        "authoring had already run when the phase aborted before start()"
    );
}

#[test]
fn an_app_preparation_task_runs_after_authoring() {
    let log = trace();
    let app = traced_app(&log)
        .prepare_with("test/after", trace_task("app", &log))
        .build();
    assert_eq!(*log.borrow(), vec!["author", "app"]);
    assert_eq!(app.runtime.state(), RuntimeState::Running);
}

#[test]
fn app_preparation_tasks_run_in_the_order_they_were_added() {
    let log = trace();
    let app = traced_app(&log)
        .prepare_with("a", trace_task("a", &log))
        .prepare_with("b", trace_task("b", &log))
        .prepare_with("c", trace_task("c", &log))
        .build();
    assert_eq!(*log.borrow(), vec!["author", "a", "b", "c"]);
    assert_eq!(app.renderable_count(), 1);
}

#[test]
fn an_app_task_cannot_run_before_the_author_task() {
    // `prepare_with` is called *before* `setup` here. Push order on the builder
    // is irrelevant: `realize` pushes the author task first, so there is no
    // call sequence that puts an app task in front of the engine's own work.
    let log = trace();
    let recorder = Rc::clone(&log);
    let _running = App::new()
        .window(Window::new(800, 600))
        .prepare_with("early", trace_task("early", &log))
        .setup(move |_world, _meshes, _materials| {
            recorder.borrow_mut().push("author");
        })
        .build();
    assert_eq!(*log.borrow(), vec!["author", "early"]);
}

#[test]
fn reauthor_still_works_after_running() {
    // Preparation is a launch-time phase, not a rebuild mechanism: a live
    // re-author replaces the world in place and the runtime stays `Running`
    // without a second preparation phase (which the runtime would reject).
    let mut app = player_app().build();
    assert_eq!(app.runtime.state(), RuntimeState::Running);
    app.reauthor(|world, meshes, materials| {
        let cube = meshes.add(Mesh::cube());
        for offset_x in [-2.0_f32, 2.0] {
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
    assert_eq!(app.renderable_count(), 2);
    assert_eq!(
        app.runtime.state(),
        RuntimeState::Running,
        "reauthoring does not disturb the lifecycle"
    );
    assert_eq!(app.tick(1).draws().len(), 2);
}

#[test]
fn an_app_with_no_preparation_tasks_still_realizes() {
    // The schedule holds only the engine's own author task — a bare `App` never
    // touches `prepare_with` and still reaches `Running`.
    let mut app = App::new().build();
    assert_eq!(app.runtime.state(), RuntimeState::Running);
    assert_eq!(app.renderable_count(), 0);
    assert!(app.tick(0).draws().is_empty());
}
