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
