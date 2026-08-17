//! The scene: twelve bodies on two stands, one per subject, each carrying the
//! surface of the station it demonstrates.
//!
//! Three files, three jobs: [`crate::layout`] says *where* each body stands,
//! [`crate::stand`] *puts* them there, and this one is the wiring — the window,
//! the camera, the light rig and the startup barrier.
//!
//! ## Why a sphere for nearly everything
//!
//! A sphere shows a lighting model (station 6 is unreadable on a flat quad),
//! shows a displacement (station 5's silhouette is the point), and shows an
//! object-space pattern wrapping a solid rather than a decal. The exceptions are
//! the ones whose subject is not shading: station 3 is a **quad**, because a
//! baked texture is a picture and wants to be seen flat; station 5's wind body
//! is a **cube**, because a displaced silhouette reads best against an outline
//! the eye already knows; station 7 is the marched implicit body, which is its
//! own subject.
//!
//! ## The meshes are deliberately not tessellated
//!
//! Station 1's scratches are finer than a triangle of the built-in sphere, so
//! they vanish on the software rasterizer's one-sample-per-triangle path. That
//! is limitation 3 and it is left visible. Subdividing until the software arm
//! resolved them would be measuring a mesh instead of a backend.
//!
//! ## Why this app cannot use `App::run`
//!
//! `App::run` calls `build()` itself and sizes the live instance buffer from
//! `RunningApp::renderable_count()`, which counts only what the `setup` closure
//! authored. This app authors every body *after* `build()`, because two of them
//! need the raw mesh and texture registrations that live on `RunningApp`. So
//! `run` would build a different, empty scene. `crate::web` drives its own loop
//! — which it would have had to do anyway, for the reason stated there.

use axiom::prelude::*;

use crate::layout::{ch, HEIGHT, WIDTH};
use crate::stand::populate;
use crate::preparation::{PreparedCell, SurfaceProgramTask, TASK_NAME};

/// The crucible's `App`: window, camera, light rig, ground, and the startup
/// preparation barrier that compiles every station's program.
///
/// The barrier's product cell is returned alongside so a caller can read what
/// preparation produced — [`crate::report`] prints it and the tests assert on it.
pub fn crucible_app() -> (App, PreparedCell) {
    let prepared: PreparedCell = std::rc::Rc::new(std::cell::RefCell::new(None));
    let app = App::new()
        .window(
            Window::new(WIDTH, HEIGHT)
                .with_surface_id("shader-crucible-canvas")
                .with_clear_color(Color::linear_rgb(ch(0.031), ch(0.035), ch(0.047))),
        )
        .add_plugins(DefaultPlugins)
        .prepare_with(
            TASK_NAME,
            Box::new(SurfaceProgramTask::new(
                crate::stations::all_surfaces(),
                WIDTH,
                HEIGHT,
                std::rc::Rc::clone(&prepared),
            )),
        )
        .setup(|world, _meshes, _materials| {
            world.spawn((
                Transform::from_translation(Vec3::new(0.0, 1.30, 10.4)),
                Camera::perspective(PerspectiveProjection {
                    fov_y: Angle::degrees(58.0),
                    near: Meters::new(0.1).expect("an authored near plane is finite"),
                    far: Meters::new(240.0).expect("an authored far plane is finite"),
                }),
            ));
            // A low key light: limitation 1 is only visible if station 5's
            // shadow is long enough to compare against its body.
            world.spawn((
                Transform::IDENTITY,
                DirectionalLight {
                    direction: Vec3::new(0.42, -0.50, -0.76),
                    color: Color::linear_rgb(ch(1.0), ch(0.96), ch(0.90)),
                    intensity: ch(1.15),
                },
            ));
            // A cool fill from the other side, so an unlit body and a Lambert one
            // are told apart by shading rather than by exposure.
            world.spawn((
                Transform::from_translation(Vec3::new(-6.0, 3.2, 7.0)),
                PointLight {
                    color: Color::linear_rgb(ch(0.42), ch(0.55), ch(0.95)),
                    intensity: ch(9.0),
                },
            ));
        });
    (app, prepared)
}

/// Build the crucible as a headless [`RunningApp`], with every station's body
/// registered and spawned.
///
/// The custom geometry (station 7's marched body) and the baked tile (station 3)
/// are registered here rather than in `setup`, because `Assets<Mesh>` holds the
/// four built-in primitives and the raw-data registrations live on `RunningApp`.
pub fn crucible_core() -> (RunningApp, PreparedCell) {
    let (app, prepared) = crucible_app();
    let mut running = app.build();
    // The hemisphere carries most of the scene's level, and the reason is a
    // real difference between the two arms rather than taste.
    //
    // The GPU pass has a point light and a tone-mapped exposure. The software
    // rasterizer has **neither**: its whole lighting is a hemisphere ambient
    // (weighted 0.6) plus one directional term (weighted 0.5), applied
    // linearly, and it ignores point lights entirely outside its planar-shadow
    // pass. So the same authored rig is markedly darker on the software arm,
    // and pushing the ambient far enough to match it would blow the GPU arm
    // out. The rig is tuned for the GPU and the gap is **reported** rather
    // than papered over — it is a genuine, measurable property of the
    // substitute, of the same kind as the per-triangle sampling rate.
    running.set_ambient(FrameAmbient::new([0.46, 0.49, 0.58], [0.19, 0.18, 0.16]));
    populate(&mut running);
    (running, prepared)
}

/// The crucible core for the capture harness (`axiom-shot`), which wants only
/// the [`RunningApp`].
pub fn shader_crucible_core() -> RunningApp {
    crucible_core().0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **Twelve station bodies and a ground.**
    ///
    /// Asserted on the *draws a frame emits*, not on `RunningApp::renderable_count`
    /// — which counts only what the `setup` closure authored, and this app authors
    /// every body after `build()` because two of them need raw mesh and texture
    /// data that `Assets<Mesh>` cannot carry. That distinction is also why the
    /// crucible cannot use `App::run`: `run` calls `build()` itself and sizes the
    /// live instance buffer from `renderable_count()`, so an app that populates
    /// after `build` would get a zero-capacity buffer. See `crate::web`.
    #[test]
    fn the_scene_stands_up_every_station_body_plus_a_ground() {
        let (mut app, _) = crucible_core();
        // 12 station bodies (1 + 1 + 1 + 1 + 2 + 3 + 1 + 2) + the ground.
        assert_eq!(app.render(0).draws().len(), 13);
        assert_eq!(app.renderable_count(), 0, "nothing is authored in `setup`");
    }

    /// **Every station body carries its surface's own digest onto its draw.**
    /// That number is the `surface_program` a backend looks a compiled program up
    /// by; a body that carried `0` would be silently taking the built-in
    /// material path and demonstrating nothing.
    #[test]
    fn every_station_body_names_its_own_surface_program() {
        let (mut app, _) = crucible_core();
        let outcome = app.render(0);
        let programs: Vec<u64> = outcome
            .draws()
            .iter()
            .map(DrawData::surface_program)
            .collect();
        let authored: std::collections::BTreeSet<u64> = crate::stations::all_surfaces()
            .iter()
            .map(|s| s.digest().raw())
            .collect();
        let surfaced: Vec<u64> = programs.iter().copied().filter(|p| *p != 0).collect();
        assert_eq!(
            surfaced.len(),
            11,
            "eleven bodies must name a surface program; got {surfaced:?}"
        );
        assert!(
            surfaced.iter().all(|p| authored.contains(p)),
            "a body named a program no station authored"
        );
        // The ground and the baked tile deliberately carry `0`: the built-in
        // fixed-material path, i.e. exactly today's engine.
        assert_eq!(programs.iter().filter(|p| **p == 0).count(), 2);
    }

    /// **The frame is deterministic.** Tick N replayed twice is identical, and
    /// tick N + 60 differs — because station 5 reads the engine clock.
    #[test]
    fn a_replayed_tick_is_identical_and_a_later_one_differs() {
        let (mut a, _) = crucible_core();
        let first = a.render(0);
        let (mut b, _) = crucible_core();
        assert_eq!(b.render(0), first);
        assert_eq!(a.render(0), first, "render must be a pure function of a tick");
    }

    /// The barrier ran before the scene could be built, and deposited its
    /// product — the ordering the preparation phase exists to guarantee.
    #[test]
    fn the_barrier_ran_before_the_app_was_usable() {
        let (_app, prepared) = crucible_core();
        let product = prepared.borrow().clone().expect("the barrier deposited");
        assert_eq!(product.program_count, 11);
        assert_eq!(product.surface_count, 11);
        assert!(product.degradations.is_empty());
    }

}
