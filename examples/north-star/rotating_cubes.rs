//! NORTH-STAR SKETCH — NOT COMPILED, NOT A WORKSPACE MEMBER.
//!
//! This file imagines the rotating-cube demo written against a *finished*,
//! stable Axiom API (the Godot/Unity/Unreal-tier surface we are building
//! toward). None of these symbols exist yet — `axiom::prelude`, `App`,
//! `Spin`, `DefaultPlugins`, etc. are the target ergonomics, not today's API.
//! It lives under `examples/north-star/` precisely so it stays OUT of the
//! Cargo workspace: it is never built, classified by the Module Law, or held
//! to the Coverage Law. It is a design target to measure the real engine
//! against, nothing more.
//!
//! The shipping equivalent today is the ~8-file
//! `apps/axiom-demo-rotating-cube-browser` crate. Every file there is engine
//! surface this one `App` does not yet have to provide. When this file can be
//! deleted because it compiles verbatim, the engine is "done."
//!
//! ---
//!
//! Axiom example: three deterministic spinning cubes in a browser canvas.
//!
//! This is the whole app. The engine owns the window/canvas binding, the GPU
//! backend, the render pipeline, the fixed-tick simulation, and the `Spin`
//! component that animates rotation. We just describe the scene.
//!
//! Build: `axiom build --target web` -> serves a page hosting
//! `<canvas id="axiom-cube-canvas">`.

use axiom::prelude::*;

fn main() {
    App::new()
        // Window/canvas + presentation. On the web target `canvas_id` binds to
        // the element; on native it opens a window of the same size.
        .window(Window {
            title: "Axiom — Rotating Cubes",
            canvas_id: "axiom-cube-canvas",
            size: uvec2(800, 600),
            clear_color: Color::linear_rgb(0.05, 0.06, 0.08),
            present_mode: PresentMode::Fifo,
            ..default()
        })
        // Fixed-step deterministic simulation: 1 ms tick, like the slice.
        .fixed_timestep(Duration::from_millis(1))
        // Standard renderer, input, time, scene-graph, asset systems.
        .add_plugins(DefaultPlugins)
        .add_systems(Startup, setup_scene)
        .run();
}

/// Linear-RGBA colour per cube, matching the original demo.
const RED: Color = Color::linear_rgb(0.85, 0.25, 0.25);
const GREEN: Color = Color::linear_rgb(0.30, 0.80, 0.35);
const BLUE: Color = Color::linear_rgb(0.30, 0.50, 0.95);

/// One full revolution every 360 ticks (the engine's `Spin` integrates this
/// against the fixed timestep, so rotation is deterministic and replayable).
const PERIOD: Ticks = Ticks(360);

fn setup_scene(mut world: SceneCommands, mut meshes: Assets<Mesh>, mut materials: Assets<Material>) {
    // One shared cube mesh; one lit material per cube.
    let cube = meshes.add(Mesh::cube(1.0));

    // (offset on x, spin axis, colour) — three cubes, three axes.
    let cubes = [
        (-2.6, Vec3::Y,                  RED),
        ( 0.0, Vec3::X,                  GREEN),
        ( 2.6, Vec3::new(1.0, 1.0, 0.0), BLUE),
    ];

    for (offset_x, axis, color) in cubes {
        let material = materials.add(Material::lit(color));
        // A parent at the x-offset, and a spinning child renderable — the same
        // translation-parent / spinning-child shape as the original slice.
        world
            .spawn(Transform::from_translation(vec3(offset_x, 0.0, 0.0)))
            .with_child((
                Renderable { mesh: cube.clone(), material },
                Spin::around(axis).period(PERIOD),
            ));
    }

    // Camera pulled back on +z, 60° vertical FOV.
    world.spawn((
        Transform::from_translation(vec3(0.0, 0.0, 8.0)).looking_at(Vec3::ZERO, Vec3::Y),
        Camera::perspective(PerspectiveProjection {
            fov_y: Angle::degrees(60.0),
            near: 0.1,
            far: 100.0,
        }),
    ));

    // Single white directional light.
    world.spawn((
        Transform::IDENTITY,
        DirectionalLight {
            direction: vec3(0.3, -1.0, 0.4),
            color: Color::WHITE,
            intensity: 1.0,
        },
    ));
}
