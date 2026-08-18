//! **The browser bootstrap** — `src/main.js`, on Axiom's engine path.
//!
//! [`build`] realizes the game and the engine world together: [`Game::new`]
//! builds the level, the BVH, the character and the camera rig; the [`App`] is
//! authored with the sky's key light and hemisphere ambient, and `install`
//! uploads the level's geometry and the rifle. [`frame`] then advances one
//! rendered frame — game first, camera second, engine tick third.
//!
//! ## Why this drives `axiom-windowing` rather than calling `App::run`
//!
//! `App::run` is the normal engine path and this app is authored entirely for
//! it: `App::new().window().add_plugins().setup().install()`, then `build()`
//! into a [`RunningApp`]. What it cannot do is drive an *input-driven* game,
//! because `run`'s per-frame closure is `|tick| running.tick(tick)` and takes no
//! app callback — there is no seam at which a game reads this frame's input,
//! steps its simulation, and writes this frame's camera before the engine
//! renders. Every input-driven Rust app in this repository hits the same wall
//! and drives `axiom-windowing` itself (`apps/burnt-rubber/src/web.rs`,
//! `apps/dog/src/live.rs`).
//!
//! So [`start`] replicates `App::run`'s body exactly — same surface
//! configuration, same ambient/fog/surface/material-program carry-over, same
//! seven-tuple frame closure — with three lines added ahead of `tick`:
//! [`Game::frame`], the camera write, and the HUD pull. Nothing about the
//! authoring path changes; only the loop's owner does.
//!
//! **The right fix is an engine one**: a per-frame app hook on `App`
//! (`App::each_frame(FnMut(&mut RunningApp, u64))`, called by `run` and by
//! `RunningApp::tick`, so it is natively testable and covered). That belongs in
//! `modules/axiom`, which this port is not permitted to touch — see the notes
//! file, where it is written up as the one engine capability this scene needs
//! and does not have.

use std::collections::BTreeMap;

use axiom::prelude::*;
use axiom_math::Quat;

use crate::scene::game::{CameraPose, Game};
use crate::scene::level::LevelBatch;
use crate::viewer::{bucket_color, ch, to_mesh_data};
use crate::weapons::geometry::Geo;
use crate::weapons::models::rifle::build_rifle;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

/// The canvas the page hands the engine.
pub const SURFACE_ID: &str = "claude-of-duty";

/// Metres in front of the spawn the rifle is laid down. Far enough to sit inside
/// the 80-degree frame from a 1.66 m eye (atan(1.5 / 3.0) = 27 degrees below the
/// horizon, against a 40-degree half-FOV) rather than under the bottom edge.
const RIFLE_STANDOFF: f64 = 3.0;

/// Metres the rifle's bore axis rides above the ground when it is laid on its
/// magazine. The magazine hangs ~0.15 m below the bore (`rifle.js`'s `magSeat`),
/// so this seats it without sinking it.
const RIFLE_BORE_HEIGHT: f64 = 0.16;

/// Near/far planes. Near is centimetre-scale because the rifle on the ground
/// gets within arm's reach; far covers the 168 m terrain plate.
const NEAR: f64 = 0.05;
const FAR: f64 = 400.0;

/// A built scene: the game state and the realized engine world it renders
/// through.
pub struct Scene {
    pub game: Game,
    pub app: RunningApp,
}

/// The rifle's merged-per-material buckets — `buildRifle().body.build()`.
fn rifle_buckets() -> BTreeMap<String, Geo> {
    build_rifle().body.build()
}

/// Build the game and the engine world it draws through.
pub fn build(seed: u32) -> Scene {
    let mut game = Game::new(seed);

    // The level's geometry is consumed by the upload; nothing reads it again.
    let batches = std::mem::take(&mut game.level.batches);
    let rifle = rifle_buckets();
    let spawn = game.spawn;
    let rifle_seat = Vec3::new(
        (spawn.position[0] - spawn.yaw.sin() * RIFLE_STANDOFF) as f32,
        (spawn.position[1] + RIFLE_BORE_HEIGHT) as f32,
        (spawn.position[2] - spawn.yaw.cos() * RIFLE_STANDOFF) as f32,
    );
    let rifle_yaw = spawn.yaw as f32 + std::f32::consts::FRAC_PI_2;

    let sun_direction = game.sky.sun_direction;
    let sun_color = game.sky.sun_color;
    let sun_intensity = game.sky.sun_intensity;
    let clear_color = game.sky.clear_color;
    let ambient_sky = game.sky.ambient_sky;
    let ambient_ground = game.sky.ambient_ground;
    let pose = game.pose();

    let app = App::new()
        .window(
            Window::new(1280, 720)
                .with_surface_id(SURFACE_ID)
                .with_clear_color(clear_color),
        )
        .add_plugins(DefaultPlugins)
        .setup(move |world, _meshes, _materials| {
            // The key light is the sun, aimed the way it travels — the
            // ephemeris direction points *at* the sun, so the light's direction
            // is its negation.
            world.spawn((
                Transform::IDENTITY,
                DirectionalLight {
                    direction: Vec3::new(-sun_direction.x, -sun_direction.y, -sun_direction.z),
                    color: sun_color,
                    intensity: ch(sun_intensity.clamp(0.05, 1.0)),
                },
            ));
        })
        .install(move |running| {
            let sky = ambient_sky.to_array();
            let ground = ambient_ground.to_array();
            running.set_ambient(FrameAmbient::new(
                [sky[0], sky[1], sky[2]],
                [ground[0], ground[1], ground[2]],
            ));
            install_level(running, batches);
            install_rifle(running, &rifle, rifle_seat, rifle_yaw);
            write_camera(running, pose);
        })
        .build();

    Scene { game, app }
}

/// Upload one mesh and one material per level batch, and spawn one node per
/// instance of it.
///
/// **This is the draw-call budget.** A merged static batch has exactly one
/// instance (its vertices are already in world space, so the transform is the
/// identity); a prototype batch has one per placement. Every node in a batch
/// shares the batch's single mesh handle and single material handle, which is
/// what lets the engine collapse the whole batch into one `mesh_batches` entry —
/// one draw. Uploading a mesh per placement instead would turn ~150 props into
/// ~150 draws.
fn install_level(running: &mut RunningApp, batches: Vec<LevelBatch>) {
    for batch in batches {
        let mesh = running
            .add_mesh_data(batch.mesh)
            .expect("an assembler batch is valid renderable geometry");
        let material = running.add_material(Material::lit(batch.albedo));
        for placement in batch.instances {
            running.spawn(Spawn::new(placement, mesh, material));
        }
    }
}

/// Lay the rifle down in the level.
///
/// **Not a viewmodel.** `weapons/rig.js` and `viewmodel.js` are not ported, so
/// there is no hand rig, no ADS pose and no sway — inventing one would be
/// fabricating the very thing the port has not done. The rifle is placed as
/// what it honestly is: an object sitting in the world, at real scale, in front
/// of the player, so the 27 ported parts can be seen assembled.
fn install_rifle(
    running: &mut RunningApp,
    buckets: &BTreeMap<String, Geo>,
    seat: Vec3,
    yaw: f32,
) {
    let rotation = Quat::from_axis_angle(Vec3::UNIT_Y, yaw).expect("authored yaw is finite");
    let transform = Transform::new(seat, rotation, Vec3::new(1.0, 1.0, 1.0));
    for (bucket, geo) in buckets {
        let mesh = running
            .add_mesh_data(to_mesh_data(geo))
            .expect("a golden-pinned rifle bucket is valid renderable geometry");
        let material = running.add_material(Material::lit(bucket_color(bucket)));
        running.spawn(Spawn::new(transform, mesh, material));
    }
}

/// The frame's camera, written onto the engine's single camera node.
///
/// The rotation is composed as Three's default `'XYZ'` Euler order — `qx * qy *
/// qz`, which is what `camera.rotation.set(pitch, yaw, roll)` means with r180's
/// default order (`camera.js:348`). `Quat::from_euler_xyz` composes the *other*
/// way (`qz * qy * qx`), which is a different rotation; the port recipe names
/// this trap explicitly, so the composition is spelled out here.
pub fn write_camera(running: &mut RunningApp, pose: CameraPose) {
    let axis = |a: Vec3, angle: f64| {
        Quat::from_axis_angle(a, angle as f32).expect("an authored camera angle is finite")
    };
    let rotation = axis(Vec3::UNIT_X, pose.rotation.pitch)
        .multiply(axis(Vec3::UNIT_Y, pose.rotation.yaw))
        .multiply(axis(Vec3::UNIT_Z, pose.rotation.roll));
    let transform = Transform::new(
        Vec3::new(pose.eye[0] as f32, pose.eye[1] as f32, pose.eye[2] as f32),
        rotation,
        Vec3::new(1.0, 1.0, 1.0),
    );
    running.set_camera(
        Camera::perspective(PerspectiveProjection {
            fov_y: Angle::degrees(pose.fov_degrees as f32),
            near: Meters::new(NEAR as f32).expect("authored near plane is finite"),
            far: Meters::new(FAR as f32).expect("authored far plane is finite"),
        }),
        transform,
    );
}

/// Advance one rendered frame: step the game with this frame's input, write the
/// camera it resolved, then let the engine render.
pub fn frame(scene: &mut Scene, dt: f64, input: &mut crate::input::Input, tick: u64) -> FrameOutcome {
    let pose = scene.game.frame(dt, input);
    write_camera(&mut scene.app, pose);
    // The HUD model is advanced every frame whether or not a view is mounted:
    // its damped channels are stateful, so a HUD that only ticks when visible
    // snaps when it appears.
    scene.game.hud_frame();
    scene.app.tick(tick)
}

/// Browser entry: build the scene, attach the input listeners, and drive the
/// presentation loop. See the module doc comment for why this is
/// `axiom-windowing` rather than `App::run`.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn claude_of_duty_start() {
    console_error_panic_hook::set_once();

    let mut scene = build(crate::engine::CAPTURE_SEED);
    let input = std::rc::Rc::new(std::cell::RefCell::new(crate::input::Input::new()));

    let window = web_sys::window().expect("a browser window");
    let document = window.document().expect("a document");
    let canvas: web_sys::HtmlElement = document
        .get_element_by_id(SURFACE_ID)
        .expect("the page hosts the presentation element")
        .unchecked_into();
    crate::input::dom::attach(&input, &canvas);

    let (width, height) = (1280u32, 720u32);
    scene
        .game
        .hud
        .resize(f64::from(width), f64::from(height));

    let mut windowing = axiom_windowing::WindowingApi::new();
    windowing
        .configure_surface(width, height)
        .expect("surface dimensions are valid");
    windowing.set_ambient(scene.app.ambient());
    scene
        .app
        .depth_fog()
        .into_iter()
        .for_each(|fog| windowing.set_depth_fog(fog));
    windowing.set_surfaces(scene.app.surfaces().to_vec());
    windowing.set_material_programs(scene.app.material_surface_programs());

    let meshes = scene.app.mesh_set();
    let materials = scene.app.material_textures();
    let max_instances = scene.app.renderable_count() as u32;

    let performance = window.performance().expect("a performance clock");
    let mut last = performance.now();

    let _ = windowing.run_web_multi(SURFACE_ID, meshes, materials, max_instances, move |tick| {
        let now = performance.now();
        let dt = (now - last) / 1000.0;
        last = now;

        let pad = crate::input::dom::poll_pad();
        let pose = {
            let mut input = input.borrow_mut();
            input.poll_gamepad(pad);
            scene.game.frame(dt, &mut input)
        };
        write_camera(&mut scene.app, pose);
        scene.game.hud_frame();

        let outcome = scene.app.tick(tick);
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
            outcome.camera_view_proj(),
            outcome.mesh_batch_casters(),
            outcome.sdf_scene().cloned(),
        )
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::CAPTURE_SEED;
    use crate::input::Input;

    #[test]
    fn the_scene_realizes_with_the_level_and_the_rifle_on_screen() {
        let scene = build(CAPTURE_SEED);
        let rifle_buckets = rifle_buckets().len();
        assert!(rifle_buckets >= 4, "the rifle merged into {rifle_buckets} buckets");
        // Every level instance and every rifle bucket is a renderable node.
        assert!(
            scene.app.renderable_count() > rifle_buckets,
            "only {} renderables — the level did not install",
            scene.app.renderable_count()
        );
    }

    #[test]
    fn the_street_costs_about_a_hundred_draws_not_a_node_per_prop() {
        let mut scene = build(CAPTURE_SEED);
        let mut input = Input::new();
        let outcome = frame(&mut scene, 1.0 / 60.0, &mut input, 0);
        let draws = outcome.mesh_batches().len();
        let nodes = scene.app.renderable_count();
        // Instancing is the whole point: many more nodes than draws.
        assert!(
            nodes > draws * 2,
            "{nodes} nodes over {draws} draws — the props are not instancing"
        );
        // The Assembler's own count plus the rifle's buckets is what the engine
        // should be batching to. A hard ceiling, so a future dressing pass that
        // places a prototype outside the instancing path fails here.
        assert!(
            draws < 150,
            "{draws} draws — the level blew the draw-call budget"
        );
    }

    #[test]
    fn the_first_frame_draws_the_whole_scene_under_one_sun() {
        let mut scene = build(CAPTURE_SEED);
        let mut input = Input::new();
        let outcome = frame(&mut scene, 1.0 / 60.0, &mut input, 0);
        assert_eq!(
            outcome.draws().len(),
            scene.app.renderable_count(),
            "every installed node drew"
        );
        assert_eq!(outcome.lights().len(), 1, "the sun is the only light");
        // The clear colour is the resolved sky, not black.
        let clear = outcome.clear_color();
        assert!(
            clear[2] > 0.1,
            "the sky cleared to {clear:?}, which is not daylight"
        );
    }

    #[test]
    fn walking_forward_moves_every_draw_relative_to_the_camera() {
        let mut scene = build(CAPTURE_SEED);
        let mut input = Input::new();
        let before = frame(&mut scene, 1.0 / 60.0, &mut input, 0);
        let first = before.draws()[0].mvp();
        input.key_down("KeyW");
        let mut after = before.clone();
        for tick in 1..=120 {
            after = frame(&mut scene, 1.0 / 60.0, &mut input, tick);
        }
        assert_ne!(first, after.draws()[0].mvp(), "the camera never moved");
    }

    #[test]
    fn the_camera_is_above_the_ground_and_inside_the_far_plane() {
        let mut scene = build(CAPTURE_SEED);
        let mut input = Input::new();
        for tick in 0..30 {
            frame(&mut scene, 1.0 / 60.0, &mut input, tick);
        }
        let pose = scene.game.pose();
        // The spawn sits on a street a metre or so off datum; the eye must be
        // clearly above the feet and nowhere near the far plane.
        assert!(pose.eye[1] > scene.game.movement.position[1] + 1.0);
        assert!(pose.eye[0].abs() < FAR && pose.eye[2].abs() < FAR);
    }

    #[test]
    fn the_camera_rotation_is_composed_in_threes_xyz_order() {
        // A pure yaw must send local -Z to the yaw's forward, with no roll
        // introduced — the composition order is what this checks.
        let mut scene = build(CAPTURE_SEED);
        let yaw = 0.7_f64;
        write_camera(
            &mut scene.app,
            CameraPose {
                eye: [0.0, 2.0, 0.0],
                rotation: crate::player::camera::Euler {
                    pitch: 0.0,
                    yaw,
                    roll: 0.0,
                },
                fov_degrees: 80.0,
            },
        );
        let axis = |a: Vec3, angle: f64| Quat::from_axis_angle(a, angle as f32).unwrap();
        let forward = axis(Vec3::UNIT_X, 0.0)
            .multiply(axis(Vec3::UNIT_Y, yaw))
            .multiply(axis(Vec3::UNIT_Z, 0.0))
            .rotate(Vec3::new(0.0, 0.0, -1.0));
        assert!((forward.x - (-(yaw as f32).sin())).abs() < 1e-5);
        assert!((forward.z - (-(yaw as f32).cos())).abs() < 1e-5);
        assert!(forward.y.abs() < 1e-6, "a pure yaw introduced pitch");
    }
}
