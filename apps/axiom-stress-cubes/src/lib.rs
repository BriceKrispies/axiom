//! # Axiom — Browser/WASM Stress-Cubes App
//!
//! A **load/stress visual**: a field of `N` independently-spinning cube
//! renderables, authored as pure scene description on the engine's `App` API and
//! presented through the real wgpu instanced drawer. It is the browser-visible
//! counterpart to `bench/pipeline-scaling`: that harness measures the
//! deterministic CPU pipeline in numbers; this one *shows* the same pipeline
//! painting, so you can watch the frame rate fall as `N` climbs and feel where
//! the cost lives.
//!
//! `N` is chosen by the page (a `?cubes=` query parameter), handed to the
//! `#[wasm_bindgen]` `stress_start(cubes)` entry, which authors an `N`-cube grid
//! and `run`s. The engine owns the canvas/GPU binding, the render pipeline, the
//! fixed-tick simulation, and the `Spin` component; this app only authors the
//! scene. Scene authoring is browser-free and unit-tested on native through
//! `App::build` + `tick`; the page measures FPS independently in JS.
//!
//! Standalone home of the demo formerly merged into `apps/axiom-gallery`
//! (`src/stress_cubes/`), extracted in the gallery de-merge.

use axiom::prelude::*;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
pub mod overlay;

/// Window/surface resolution (the wgpu surface size, independent of CSS size).
const SURFACE_WIDTH: u32 = 1280;
const SURFACE_HEIGHT: u32 = 800;
/// Grid spacing between cube centres, in world units.
const SPACING: f32 = 2.2;
/// Clamp the requested cube count into a sane range (avoids a 0-instance buffer
/// and a pathological allocation from a fat query string).
const MIN_CUBES: u32 = 1;
const MAX_CUBES: u32 = 200_000;

/// A linear colour channel / intensity from a known-finite value.
fn ch(value: f32) -> Ratio {
    Ratio::new(value.clamp(0.0, 1.0)).expect("clamped colour channel is finite")
}

/// A varied per-cube colour: a smooth rainbow sweep over the cube index so the
/// field reads as a dense, multicoloured swarm rather than one flat mass.
fn cube_color(i: u32, total: u32) -> Color {
    let t = i as f32 / total.max(1) as f32;
    let tau = std::f32::consts::TAU;
    Color::linear_rgb(
        ch(0.5 + 0.45 * (tau * t).sin()),
        ch(0.5 + 0.45 * (tau * t + tau / 3.0).sin()),
        ch(0.5 + 0.45 * (tau * t + 2.0 * tau / 3.0).sin()),
    )
}

/// Author the stress scene: a roughly-square grid of `cubes` spinning cube
/// renderables centred at the origin, a directional light, and a camera pulled
/// back far enough to frame the whole grid. Each cube spins on one of three axes
/// with a per-cube period, so no two are in lockstep — more varied transforms,
/// a denser visual, and a heavier-than-trivial per-frame workload.
pub fn stress_cubes_app(cubes: u32) -> App {
    let cubes = cubes.clamp(MIN_CUBES, MAX_CUBES);
    let cols = (cubes as f32).sqrt().ceil() as u32;
    let rows = cubes.div_ceil(cols);

    // Centre the grid on the origin.
    let x0 = -((cols - 1) as f32) * SPACING / 2.0;
    let y0 = -((rows - 1) as f32) * SPACING / 2.0;

    // Pull the camera back to fit the grid in a 60° vertical FoV at the surface
    // aspect ratio, then add margin so edge cubes aren't clipped.
    let aspect = SURFACE_WIDTH as f32 / SURFACE_HEIGHT as f32;
    let tan_half_fov = (std::f32::consts::FRAC_PI_6).tan(); // tan(30°) = half of 60°
    let half_h = rows as f32 * SPACING / 2.0;
    let half_w = cols as f32 * SPACING / 2.0;
    let dist_v = half_h / tan_half_fov;
    let dist_w = half_w / (tan_half_fov * aspect);
    let camera_z = dist_v.max(dist_w) * 1.15 + 4.0;

    App::new()
        .window(
            Window::new(SURFACE_WIDTH, SURFACE_HEIGHT)
                .with_surface_id("axiom-stress-canvas")
                .with_clear_color(Color::linear_rgb(ch(0.03), ch(0.04), ch(0.06))),
        )
        .add_plugins(DefaultPlugins)
        .setup(move |world, meshes, materials| {
            let cube = meshes.add(Mesh::cube());
            (0..cubes).for_each(|i| {
                let col = i % cols;
                let row = i / cols;
                let x = x0 + col as f32 * SPACING;
                let y = y0 + row as f32 * SPACING;
                // `i % 3` is always 0, 1, or 2, so this index is always in bounds.
                let axis = [Vec3::UNIT_Y, Vec3::UNIT_X, Vec3::new(1.0, 1.0, 0.0)][(i % 3) as usize];
                let period = 120 + (i % 240);
                let material = materials.add(Material::lit(cube_color(i, cubes)));
                world
                    .spawn(Transform::from_translation(Vec3::new(x, y, 0.0)))
                    .with_child((
                        Renderable {
                            mesh: cube,
                            material,
                        },
                        Spin::around(axis).period(period),
                    ));
            });
            world.spawn((
                Transform::from_translation(Vec3::new(0.0, 0.0, camera_z)),
                Camera::perspective(PerspectiveProjection {
                    fov_y: Angle::degrees(60.0),
                    near: Meters::new(0.1).expect("authored near plane is finite"),
                    far: Meters::new(camera_z * 4.0 + 100.0).expect("authored far plane is finite"),
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
        })
}

/// Build the stress-cubes demo's renderable core (an `N`-cube field) as a
/// headless [`RunningApp`], for the native capture harness (`axiom-shot`) and
/// the slice goldens.
pub fn stress_cubes_core(count: u32) -> RunningApp {
    stress_cubes_app(count).build()
}

/// Browser entry: author an `N`-cube stress scene and drive the terminal web run
/// loop. The page passes `cubes` (e.g. from a `?cubes=` query parameter) after
/// confirming a render backend is available; a bare-canvas host (the workspace
/// console) boots it argument-free, so `cubes` is optional and defaults to 2000
/// — the same default the page's cube-bar starts on.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn stress_start(cubes: Option<u32>) {
    console_error_panic_hook::set_once();
    stress_cubes_app(cubes.unwrap_or(2000)).run();
}

/// Backend-comparison entry: render the stress field three ways at once —
/// WebGPU, WebGL2, and Canvas 2D — into three canvases, from ONE wasm instance
/// and ONE deterministic sim. A host (the workspace dev console) creates three
/// canvases and calls this with their ids.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn stress_cubes_compare_start(canvas_a: &str, canvas_b: &str, canvas_c: &str) {
    console_error_panic_hook::set_once();
    stress_cubes_app(2000).run_compare([canvas_a, canvas_b, canvas_c]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authors_the_requested_number_of_cubes() {
        let mut app = stress_cubes_app(16).build();
        assert_eq!(app.renderable_count(), 16);
        let outcome = app.tick(0);
        assert_eq!(outcome.draws().len(), 16);
    }

    #[test]
    fn the_field_spins_and_replays_deterministically() {
        let mut a = stress_cubes_app(9).build();
        let early = a.tick(0);
        let mut later = early.clone();
        for t in 1..=60 {
            later = a.tick(t);
        }
        assert_ne!(early.draws()[0].mvp(), later.draws()[0].mvp());
        let mut b = stress_cubes_app(9).build();
        assert_eq!(b.tick(0), early);
    }

    #[test]
    fn cube_count_is_clamped_to_a_sane_range() {
        // Zero clamps up to MIN_CUBES, so the instance buffer is never empty.
        assert_eq!(stress_cubes_app(0).build().renderable_count(), 1);
    }
}
