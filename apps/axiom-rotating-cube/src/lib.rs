//! # Axiom — Browser/WASM Rotating-Cube App
//!
//! The browser-visible rotating-cube slice, now **pure scene description** on the
//! engine's high-level `App` API. The engine (`axiom`) owns the window/canvas
//! binding, the GPU backend, the render pipeline, the fixed-tick simulation, and
//! the `Spin` component; this app just authors three spinning cubes, a camera,
//! and a light, then `run`s.
//!
//! The browser entry is a single `#[wasm_bindgen]` `rotating_cube_start` the page
//! calls after confirming WebGPU is available; on the web `App::run` drives the
//! requestAnimationFrame loop and presents real pixels. The scene-authoring is
//! browser-free and unit-tested on native through `App::build` + `tick`.
//!
//! Extracted from the merged `apps/axiom-gallery` crate back into its own
//! composition-leaf app (the gallery de-merge): the wasm entry keeps its
//! namespaced `rotating_cube_start` name, [`rotating_cube_core`] remains the
//! native capture-harness core (axiom-shot / `slice.toml`), and
//! `rotating_cube_compare_start` is the crate-local port of the gallery's
//! backend-comparison entry for this demo.

use axiom::prelude::*;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
pub mod overlay;

/// A linear colour channel / intensity from a known-finite authored literal.
fn ch(value: f32) -> Ratio {
    Ratio::new(value).expect("authored colour channel is finite")
}

/// The deterministic three-cube scene, authored against the engine's `App` API:
/// a parent at an x-offset with a spinning child cube renderable, times three
/// (red on Y, green on X, blue on a diagonal), plus a pulled-back camera and a
/// single directional light.
pub fn rotating_cubes_app() -> App {
    App::new()
        .window(
            Window::new(800, 600)
                .with_surface_id("axiom-cube-canvas")
                .with_clear_color(Color::linear_rgb(ch(0.05), ch(0.06), ch(0.08))),
        )
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
            cubes.into_iter().for_each(|(offset_x, axis, color)| {
                let material = materials.add(Material::lit(color).with_texture(Texture::Checker));
                world
                    .spawn(Transform::from_translation(Vec3::new(offset_x, 0.0, 0.0)))
                    .with_child((
                        Renderable {
                            mesh: cube,
                            material,
                        },
                        Spin::around(axis).period(360),
                    ));
            });
            // A distinct mesh (scaled quad) so it batches separately from the cubes.
            let plane = meshes.add(Mesh::plane());
            let ground = materials.add(
                Material::lit(Color::linear_rgb(ch(0.18), ch(0.20), ch(0.24)))
                    .with_texture(Texture::UvGrid),
            );
            world.spawn((
                Transform::combine(
                    Transform::from_translation(Vec3::new(0.0, -2.0, 0.0)),
                    Transform::from_scale(Vec3::new(30.0, 1.0, 30.0)),
                ),
                Renderable {
                    mesh: plane,
                    material: ground,
                },
            ));
            // A third distinct mesh, so it also batches separately.
            let sphere = meshes.add(Mesh::sphere());
            let sphere_material = materials.add(
                Material::lit(Color::linear_rgb(ch(0.90), ch(0.78), ch(0.30)))
                    .with_texture(Texture::Checker),
            );
            world.spawn((
                Transform::combine(
                    Transform::from_translation(Vec3::new(0.0, 2.6, 0.0)),
                    Transform::from_scale(Vec3::new(1.6, 1.6, 1.6)),
                ),
                Renderable {
                    mesh: sphere,
                    material: sphere_material,
                },
            ));
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
                    intensity: ch(1.0),
                },
            ));
            // Each point light orbits by riding a spinning parent transform.
            let orbit_lights = [
                (
                    Vec3::UNIT_Y,
                    Color::linear_rgb(ch(0.95), ch(0.25), ch(0.25)),
                    200,
                ),
                (
                    Vec3::UNIT_Y,
                    Color::linear_rgb(ch(0.25), ch(0.95), ch(0.35)),
                    320,
                ),
                (
                    Vec3::UNIT_Y,
                    Color::linear_rgb(ch(0.30), ch(0.45), ch(0.98)),
                    260,
                ),
            ];
            orbit_lights.into_iter().for_each(|(axis, color, period)| {
                world
                    .spawn((Transform::IDENTITY, Spin::around(axis).period(period)))
                    .with_child((
                        Transform::from_translation(Vec3::new(4.5, 1.2, 0.0)),
                        PointLight {
                            color,
                            intensity: ch(9.0),
                        },
                    ));
            });
        })
}

/// Test-only public accessor: builds the exact `App` the wasm `start` entry
/// runs, so out-of-crate integration tests (notably the data-package equivalence
/// test in `tests/rotating_cube_scene_manifest.rs`) can author the scene on
/// native without the browser. Not referenced at runtime.
#[doc(hidden)]
pub fn rotating_cubes_app_for_test() -> App {
    rotating_cubes_app()
}

/// Build the rotating-cube demo's renderable core as a headless [`RunningApp`],
/// for the native capture harness (`axiom-shot`). The scene author
/// ([`rotating_cubes_app`]) is browser-free; this exposes it as a buildable core
/// so the harness can render a real native frame of it.
pub fn rotating_cube_core() -> RunningApp {
    rotating_cubes_app().build()
}

/// Browser entry: author the scene and drive the terminal web run loop. Called
/// from the page once WebGPU is confirmed available.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn rotating_cube_start() {
    console_error_panic_hook::set_once();
    rotating_cubes_app().run();
}

/// Backend-comparison entry: render this demo three ways at once — WebGPU,
/// WebGL2, and Canvas 2D — into three canvases, from ONE wasm instance and ONE
/// deterministic sim. A host (the workspace dev console) creates three canvases
/// and calls this. Crate-local port of the rotating-cube arm of the gallery's
/// shared `compare_start`.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn rotating_cube_compare_start(canvas_a: &str, canvas_b: &str, canvas_c: &str) {
    console_error_panic_hook::set_once();
    rotating_cubes_app().run_compare([canvas_a, canvas_b, canvas_c]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authors_the_deterministic_multi_mesh_scene() {
        let mut app = rotating_cubes_app().build();
        // 3 spinning cubes + a ground plane + a sphere.
        assert_eq!(app.renderable_count(), 5);
        let outcome = app.tick(0);
        // Clear + SetCamera + SetPipeline + 5 x (SetMesh + SetMaterial +
        // DrawIndexed) + Present = 19.
        assert_eq!(outcome.command_count(), 19);
        assert_eq!(outcome.draws().len(), 5);
        assert_eq!(outcome.clear_color(), [0.05, 0.06, 0.08, 1.0]);
        // Batches key on (mesh, material): the three cubes share the cube mesh but
        // each has its own (distinctly-coloured) material, so they batch
        // separately — 3 cube batches + 1 plane + 1 sphere = 5, one instance each.
        let batches = outcome.mesh_batches();
        assert_eq!(batches.len(), 5);
        assert_eq!(batches.iter().map(|batch| batch.3).sum::<u32>(), 5);
        // Every draw carries a material id so the backend can bind its albedo.
        assert!(outcome.draws().iter().all(|d| d.material_id() != 0));
        // The frame resolves four lights: the directional sun + three orbiting
        // point lights. Exactly one is directional (kind 0), three are point.
        assert_eq!(outcome.lights().len(), 4);
        assert_eq!(outcome.lights().iter().filter(|l| l.kind() == 1).count(), 3);
    }

    #[test]
    fn the_scene_spins_and_replays_deterministically() {
        let mut a = rotating_cubes_app().build();
        let early = a.tick(0);
        let mut later = early.clone();
        for t in 1..=60 {
            later = a.tick(t);
        }
        assert_ne!(early.draws()[0].mvp(), later.draws()[0].mvp());

        let mut b = rotating_cubes_app().build();
        assert_eq!(b.tick(0), early);
    }
}
