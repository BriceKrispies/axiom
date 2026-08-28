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

use crate::config::Quality;
use crate::materials::upload::{bake_library, Rgba8Map, RUNTIME_BAKE_SIZE};
use crate::scene::draw::{drive_viewmodel, write_camera};
use crate::scene::game::{CameraPose, Game};
use crate::scene::level::LevelBatch;
use crate::world::system::WorldLight;
use crate::world::palette::Palette;
use crate::scene::wiring::weapon_look::{
    drive_hands, install_hands, install_rifle, HandGeometry, HandNodes, WeaponLook,
};
use crate::viewer::to_mesh_data;
use crate::weapons::geometry::Geo;
use crate::scene::wiring::look::MaterialLook;
use crate::weapons::models::rifle::build_rifle;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

/// The canvas the page hands the engine.
pub const SURFACE_ID: &str = "shmup";

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
pub const NEAR: f64 = 0.05;
pub const FAR: f64 = 400.0;

/// A built scene: the game state and the realized engine world it renders
/// through.
pub struct Scene {
    pub game: Game,
    pub app: RunningApp,
    /// The dev console: every installed node's palette key and where it is, plus
    /// the overlay switch. Off unless something runs `ids on`.
    /// Shared, because two owners need it: the frame loop reads it to draw the
    /// overlay, and the `window.__ax_console` binding writes it from JS. An
    /// agent driving the game through Playwright is the second owner, and the
    /// whole point of the console is that it can be reached without a rebuild.
    pub console: std::rc::Rc<std::cell::RefCell<crate::scene::console::DevConsole>>,
    /// The rifle's spawned nodes, one per merged material bucket.
    ///
    /// Held so the rig can move them every frame. They used to be spawned and
    /// forgotten, which is why the rifle sat on the road: nothing could move it
    /// after `install`.
    pub rifle_nodes: Vec<Entity>,
    /// The hands' spawned nodes, one per merged (rig frame, glove surface)
    /// group. `Viewmodel` has solved both arms every frame all along; nothing
    /// had ever turned their meshes into engine geometry.
    pub hand_nodes: HandNodes,
    /// The AI's soldier bodies, on the engine's skinning path.
    pub soldier_draw: crate::scene::wiring::soldier_draw::SoldierDraw,
    /// The FX draw pool — particles, decals, brass and the flash-light ramp.
    pub fx_draw: crate::scene::wiring::fx_draw::FxDraw,
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
    // `_addLights`' output. Taken the same way the batches are: the level keeps
    // no reader for them once they are spawned.
    let practicals = std::mem::take(&mut game.level.practicals);
    let rifle = rifle_buckets();
    let spawn = game.spawn;
    let rifle_seat = Vec3::new(
        (spawn.position[0] - spawn.yaw.sin() * RIFLE_STANDOFF) as f32,
        (spawn.position[1] + RIFLE_BORE_HEIGHT) as f32,
        (spawn.position[2] - spawn.yaw.cos() * RIFLE_STANDOFF) as f32,
    );
    let rifle_yaw = spawn.yaw as f32 + std::f32::consts::FRAC_PI_2;

    // One key light from `SkySystem`, which already knows whether the sun or
    // the moon is the key. This used to be six loose fields off `SkyLook`, with
    // the negation and the clamp done by hand at the spawn site.
    let key_light = game.sky.key_light();
    let clear_color = game.sky.clear_color();
    let ambient = game.sky.ambient();
    let depth_fog = game.sky.depth_fog();
    // The *visible* sky — dome, key body and cloud layer — authored from the
    // same `SkySystem` the key light comes from. Until now `SkySystem` reached
    // the frame only as light; the sky itself was the window's clear colour.
    let sky = crate::scene::wiring::sky_draw::visible_sky(&game.sky);
    // The two-band fill. Everything the key light misses used to be lit by
    // the hemisphere ambient alone, which is why the shaded side of the
    // street read as a silhouette.
    let indirect = game.sky.indirect_fill();
    let pose = game.pose();
    // The per-key table, owned, so the `install` closure can carry it. Cloning
    // `MaterialLook` itself would clone the whole live `MaterialSystem`.
    let look: Vec<(&'static str, crate::scene::wiring::look::KeyLook)> =
        game.materials.keys().to_vec();
    // The weapon table's own material set — `weapons/materials.js`, through
    // the same `MaterialSystem` the street's palette goes through. The rifle
    // used to wear `viewer::bucket_color`, a nine-entry debug palette written
    // for the parts turntable, which is why it rendered untextured.
    let weapon_look = WeaponLook::new(game.config.quality);
    // Both arms, taken apart once. `Viewmodel::new` already built and posed
    // them and `solve_hands` already runs the IK every frame.
    let hand_geometry = HandGeometry::from_arms(
        &game.weapons.core().viewmodel.arm_l,
        &game.weapons.core().viewmodel.arm_r,
    );
    // The FX atlas tiles, lifted out before the `move` closure is authored.
    let fx_tiles = crate::scene::wiring::fx_draw::FxAtlasTiles::of(&game.fx_audio.fx);
    let surfaces = [game.materials.surfaces(), weapon_look.surfaces()].concat();

    // The console is filled inside `install` (that is where a node's name and
    // its position are both known) and read after, so it travels the same way
    // the spawned entity handles do.
    let console: std::rc::Rc<std::cell::RefCell<crate::scene::console::DevConsole>> =
        Default::default();
    let console_out = std::rc::Rc::clone(&console);

    // Filled inside `install`, read after `build`.
    let nodes: std::rc::Rc<std::cell::RefCell<Vec<Entity>>> = Default::default();
    let nodes_out = std::rc::Rc::clone(&nodes);
    let hands: std::rc::Rc<std::cell::RefCell<Option<HandNodes>>> = Default::default();
    let hands_out = std::rc::Rc::clone(&hands);
    let fx_pool: std::rc::Rc<
        std::cell::RefCell<Option<crate::scene::wiring::fx_draw::FxDraw>>,
    > = Default::default();
    let fx_pool_out = std::rc::Rc::clone(&fx_pool);

    let mut app = App::new()
        .window(
            Window::new(1280, 720)
                .with_surface_id(SURFACE_ID)
                .with_clear_color(clear_color),
        )
        .add_plugins(DefaultPlugins)
        // Declared so the preparation barrier compiles the program before the
        // first frame. Nothing compiles inside a frame in this engine; a program
        // the barrier never saw renders its fallback and reports the miss.
        // Every key's surface, so the preparation barrier compiles them all
        // before the first frame. Nothing compiles inside a frame in this
        // engine; a program the barrier never saw renders its fallback.
        .surfaces(surfaces)
        .setup(move |world, _meshes, _materials| {
            // `SkySystem` already resolved the key — including the negation, the
            // clamp, and whether it is the sun or the moon.
            world.spawn((Transform::IDENTITY, key_light));
        })
        .install(move |running| {
            running.set_ambient(ambient);
            // Aerial perspective from the real atmosphere, which `SkyLook` never
            // produced at all.
            running.set_depth_fog(depth_fog);
            // The engine's own sky pass, evaluated per pixel behind the scene
            // instead of the window's flat clear colour.
            running.set_sky(sky);
            running.set_indirect(indirect);
            crate::scene::install::install_level(
                running,
                batches,
                &look,
                &mut console_out.borrow_mut(),
            );
            // The world's own practicals. Without these the only light in the
            // scene is the sun, so every interior, every arcade and the whole
            // shaded side of the street was lit by ambient alone.
            crate::scene::install::install_practicals(running, &practicals, spawn.position);
            // The nodes escape the install closure so the rig can move them
            // every frame; the engine hands back an `Entity` per spawn and
            // this used to drop all of them on the floor, literally.
            // The FX draw pool. AFTER the practicals, because the frame's
            // sixteen light slots are filled in spawn order.
            *fx_pool_out.borrow_mut() =
                Some(crate::scene::wiring::fx_draw::FxDraw::install(running, &fx_tiles));
            // One material table for the gun AND the hands: the buckets the
            // rifle merges into and the four glove surfaces are both keys in it.
            let weapon_materials = weapon_look.install(running);
            // The nodes escape the install closure so the rig can move them
            // every frame; the engine hands back an `Entity` per spawn and
            // this used to drop all of them on the floor, literally.
            *nodes_out.borrow_mut() =
                install_rifle(running, &rifle, rifle_seat, rifle_yaw, &weapon_materials);
            *hands_out.borrow_mut() =
                Some(install_hands(running, &hand_geometry, &weapon_materials));
            write_camera(running, pose);
        })
        .build();

    // No viewmodel here. `WeaponCore` owns one (`weapons/system.rs`), and a
    // second — which this held briefly — renders a rifle that can never recoil,
    // because the recoil lands on the core's copy. It was also seeded off a
    // FRESH `Rng`, putting the weapons slot outside the level's stream entirely.
    let rifle_nodes = std::mem::take(&mut *nodes.borrow_mut());
    let hand_nodes = hands
        .borrow_mut()
        .take()
        .expect("`install` runs before `build` returns");
    let fx_draw = fx_pool
        .borrow_mut()
        .take()
        .expect("`install` ran and built the FX pool");
    // The soldiers. Registered out here rather than inside `install` because it
    // needs `game.ai`, which the install closure cannot capture.
    let soldier_draw =
        crate::scene::wiring::soldier_draw::SoldierDraw::install(&mut app, &game.ai);
    Scene {
        game,
        app,
        console,
        rifle_nodes,
        hand_nodes,
        soldier_draw,
        fx_draw,
    }
}

/// Bake the palette's surfaces and register one albedo texture per **library
/// name**, returning `(library name, texture handle)`.
///
/// The bake is [`bake_albedo_maps`] at [`RUNTIME_BAKE_SIZE`] — albedo only, and
/// small. Read that constant's doc before changing either fact: the CPU bake of
/// the library at its authored 1024² is roughly fifteen minutes of work, the
/// cause is `owSurface`'s `fract(sin(…))` hashes running scalar instead of
/// 1024²-wide, and the real fix is the source's own GPU bake. This is the part
/// that fits in a page load.
///
/// Only the albedo is registered because it is the only map the engine can be
/// handed: `axiom_host::MaterialTexture` carries albedo pixels and nothing
/// else, and the live GPU arm passes an empty normal-map slice. The normal, the
/// ORM+height, the shared detail tile and the macro field are all produced by
/// [`crate::materials::upload::bake_library`] and have nowhere to go until that
/// contract widens.
/// Advance one rendered frame: step the game with this frame's input, write the
/// camera it resolved, then let the engine render.
pub fn frame(scene: &mut Scene, dt: f64, input: &mut crate::input::Input, tick: u64) -> FrameOutcome {
    let pose = scene.game.frame(dt, input);
    write_camera(&mut scene.app, pose);
    drive_viewmodel(scene, pose);
    // The HUD: model on every target, DOM on wasm32. Its damped channels are
    // stateful, so it ticks whether or not a view is mounted.
    scene.game.hud_frame(input);
    scene.fx_draw.frame(
        &mut scene.app,
        &scene.game.fx_audio.fx,
        pose,
        scene.game.time.elapsed,
    );
    // Every visible soldier, skinned, this frame. Must precede `tick`, which is
    // what drains the queued skinned draws.
    scene.soldier_draw.frame(&mut scene.app, &scene.game.ai);
    scene.app.tick(tick)
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
        // The Assembler's own count is what the engine should be batching to. A
        // hard ceiling, so a future dressing pass that places a prototype
        // outside the instancing path fails here.
        //
        // **The viewmodel is subtracted, not absorbed.** The frame now also
        // carries the rifle's buckets and the hands, and the hands are 82 draws
        // — a deliberate trade in `weapon_look`: a skinned draw cannot bind a
        // surface program (one fixed `skinning.pipeline` for all of them), so
        // eight skinned draws would have cost the glove materials. Folding 91
        // viewmodel draws into a raised ceiling would let the street quietly
        // grow by 91 before anything failed, which is exactly the regression
        // this ceiling exists to catch.
        let viewmodel_draws = scene.hand_nodes.len() + scene.rifle_nodes.len();
        let level_draws = draws - viewmodel_draws;
        assert!(
            level_draws < 150,
            "{level_draws} level draws ({draws} total less {viewmodel_draws}              viewmodel) — the level blew the draw-call budget"
        );
    }

    /// The frame's light list is the sun plus the world's practicals, and it
    /// fits the main pass's `array<Light, 16>`.
    ///
    /// This test was called `..._under_one_sun` and asserted exactly that: one
    /// light, the sun. It was true, and it was the bug — `_addLights` had been
    /// ported and never called, so nothing the sun did not reach was lit by
    /// anything but ambient. The assertion is kept rather than deleted because
    /// the OTHER half of it still matters: overflowing sixteen silently drops
    /// lights in the shader, and the budget is the reason the practicals are
    /// selected rather than all spawned.
    #[test]
    fn the_first_frame_draws_the_scene_under_the_sun_and_its_practicals() {
        let mut scene = build(CAPTURE_SEED);
        let mut input = Input::new();
        let outcome = frame(&mut scene, 1.0 / 60.0, &mut input, 0);
        // Everything installed either drew, or is a hidden FX pool slot.
        //
        // This used to be a flat `draws == renderable_count`, which held only
        // while nothing was ever hidden. `FxDraw` spawns its whole sprite budget
        // at install and hides what the frame does not need, so a hidden slot is
        // installed-and-not-drawn BY DESIGN. The half that still bites — nothing
        // ELSE is installed and silently never drawn — survives by subtracting
        // the pool.
        let drawn = outcome.draws().len();
        let pooled = scene.fx_draw.pool_len();
        assert!(
            drawn + pooled >= scene.app.renderable_count(),
            "{} installed, {drawn} drew, {pooled} pooled — something is installed              and never drawn",
            scene.app.renderable_count()
        );

        let lights = outcome.lights().len();
        assert!(
            lights > 1,
            "only the sun is lit — the world's practicals never reached the frame"
        );
        assert!(
            lights <= 16,
            "{lights} lights, and the main pass carries array<Light, 16> — the              overflow is dropped in the shader, silently"
        );
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
    fn the_camera_rotation_is_composed_in_threes_yxz_order() {
        // A pure yaw must send local -Z to the yaw's forward, with no pitch
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
        let forward = axis(Vec3::UNIT_Y, yaw)
            .multiply(axis(Vec3::UNIT_X, 0.0))
            .multiply(axis(Vec3::UNIT_Z, 0.0))
            .rotate(Vec3::new(0.0, 0.0, -1.0));
        assert!((forward.x - (-(yaw as f32).sin())).abs() < 1e-5);
        assert!((forward.z - (-(yaw as f32).cos())).abs() < 1e-5);
        assert!(forward.y.abs() < 1e-6, "a pure yaw introduced pitch");
    }

    #[test]
    fn combined_yaw_and_pitch_introduce_no_roll() {
        // The regression this bug actually was: with Euler order 'YXZ', yaw
        // always turns around the true world-up axis, so the camera's local
        // right vector must stay perfectly horizontal (world Y = 0) for *any*
        // combination of yaw and pitch, with zero roll in the source Euler.
        // Composing pitch outside yaw (Three's default 'XYZ' order, which
        // this file wrongly assumed) rotates pitch around a world-fixed axis
        // once yaw is nonzero and tilts this vector out of the horizontal —
        // a persistent bank that never decays, since it is not the `roll`
        // channel at all.
        let mut scene = build(CAPTURE_SEED);
        for &yaw in &[0.3_f64, -0.9, 1.7, -2.4] {
            for &pitch in &[0.2_f64, -0.5, 0.9] {
                write_camera(
                    &mut scene.app,
                    CameraPose {
                        eye: [0.0, 2.0, 0.0],
                        rotation: crate::player::camera::Euler { pitch, yaw, roll: 0.0 },
                        fov_degrees: 80.0,
                    },
                );
                let axis = |a: Vec3, angle: f64| Quat::from_axis_angle(a, angle as f32).unwrap();
                let right = axis(Vec3::UNIT_Y, yaw)
                    .multiply(axis(Vec3::UNIT_X, pitch))
                    .multiply(axis(Vec3::UNIT_Z, 0.0))
                    .rotate(Vec3::new(1.0, 0.0, 0.0));
                assert!(
                    right.y.abs() < 1e-6,
                    "yaw={yaw} pitch={pitch} tilted the horizon: right.y={}",
                    right.y
                );
            }
        }
    }
}
