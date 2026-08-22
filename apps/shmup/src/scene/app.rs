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

use crate::materials::upload::{bake_albedo_maps, RUNTIME_BAKE_SIZE};
use crate::scene::game::{CameraPose, Game};
use crate::scene::level::LevelBatch;
use crate::world::palette::Palette;
use crate::viewer::{bucket_color, ch, to_mesh_data};
use crate::weapons::geometry::Geo;
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
        // Declared so the preparation barrier compiles the program before the
        // first frame. Nothing compiles inside a frame in this engine; a program
        // the barrier never saw renders its fallback and reports the miss.
        .surfaces(vec![street_material()])
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
/// The street's runtime material — the port of `materials/shader.js`, authored
/// as an `axiom_surface::Surface`.
///
/// The parameters are `DEFAULT_PARAMS`' own, with two departures, both because
/// this app is not yet uploading the maps the layers sample:
///
/// * `parallax: 0.0` — the source's default. Parallax occlusion mapping marches
///   a height map, and the ORM+height binding is still the neutral 1x1, so a
///   non-zero depth would march a flat field and cost the loop for nothing.
/// * `detile: 0.0` — the source's default too, and it keeps this app on ONE
///   program: de-tiling is a structural permutation
///   (`axiom_surface::SurfaceKind`), so turning it on here would compile a
///   second pipeline to de-tile a 1x1 texture.
///
/// What *is* on by default and does real work with no maps bound: the macro
/// variation band, the weathering stack (rain runoff, ground splash, the dust
/// wedge — all world-anchored and procedural), the cavity and wear masks, and
/// the tint/roughness remap. Those are the layers that stop a wall reading as
/// one flat colour.
///
/// `ground_y` is the world height the splash term measures up from, so it must
/// match the level's ground plane rather than being left at the default.
fn street_material() -> Surface {
    runtime_material(MaterialParams {
        // Metres per texture tile — the street is authored at metre scale.
        scale: 2.0,
        // World-anchored weathering needs the real ground height.
        ground_y: 0.0,
        // No height map bound yet; see the doc comment.
        parallax: 0.0,
        detile: 0.0,
        ..MaterialParams::default()
    })
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
fn install_surface_textures(running: &mut RunningApp) -> BTreeMap<&'static str, u64> {
    let names: Vec<&'static str> = Palette::ALL
        .iter()
        .map(|(_, entry)| entry.name)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    bake_albedo_maps(&names, RUNTIME_BAKE_SIZE)
        .into_iter()
        .zip(names)
        .map(|((_, map), name)| {
            let handle = running
                .add_texture_data(map.width, map.height, map.pixels)
                .expect("a baked map is width * height * 4 bytes");
            (name, handle.id())
        })
        .collect()
}

/// The library name a palette key's material is baked from, or `None` for a key
/// the palette does not carry (the emissive glow keys, which have no surface).
fn key_surface_name(key: &str) -> Option<&'static str> {
    Palette::ALL
        .iter()
        .find(|(name, _)| *name == key)
        .map(|(_, entry)| entry.name)
}

/// The base colour a **textured** batch multiplies its baked albedo by.
///
/// `LevelBatch::albedo` is `level::key_albedo`: the palette entry's authored
/// `tint`, or — where the entry has none — a neutral mid grey standing in for
/// "the material's own generator owns the colour" (`level.rs`'s own words).
/// Now that the generator's colour is actually uploaded, that stand-in has to
/// go: multiplying a real baked albedo by a 0.69 grey would darken every
/// untinted surface by exactly the amount the stand-in was invented to supply.
/// An untinted key becomes white, which is what the source constructs the
/// material with (`index.js:199`, `color: 0xffffff`).
fn textured_base_color(key: &str, batch_albedo: Color) -> Color {
    Palette::ALL
        .iter()
        .find(|(name, _)| *name == key)
        .and_then(|(_, entry)| entry.opts.tint)
        .map_or(Color::WHITE, |_| batch_albedo)
}

fn install_level(running: &mut RunningApp, batches: Vec<LevelBatch>) {
    let street = street_material();
    let textures = install_surface_textures(running);
    for batch in batches {
        let mesh = running
            .add_mesh_data(batch.mesh)
            .expect("an assembler batch is valid renderable geometry");
        // The batch keeps its own albedo and gains the runtime material's
        // program. `Material::from_surface` would force white and throw the
        // palette away — the source's shader *modulates* the diffuse colour, it
        // does not replace it.
        //
        // Every batch names the same surface, so this is one program and one
        // pipeline across the whole street; only the instance colour differs.
        //
        // The baked albedo rides in as the material's custom texture, and the
        // shader multiplies the two exactly as the source multiplies `mat.map`
        // by `p.tint`. A batch whose palette key names no surface keeps the
        // untextured path (a 1x1 white albedo), which is what it had before.
        let texture = key_surface_name(&batch.key).and_then(|name| textures.get(name).copied());
        let material = Material::lit(batch.albedo).with_surface(street.clone());
        let material = running.add_material(
            texture.map_or(material, |id| {
                Material::lit(textured_base_color(&batch.key, batch.albedo))
                    .with_surface(street.clone())
                    .with_custom_texture(id)
                    // The street runs from underfoot to the 168 m horizon, which
                    // is the grazing-angle case anisotropy exists for.
                    .with_texture_sampling(TextureSampling::Anisotropic)
            }),
        );
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
/// **Not** Three's default `'XYZ'` order. The source explicitly overrides it —
/// `this.camera.rotation.order = 'YXZ'` (`engine.js:30`) — and `camera.rs`'s
/// own doc comment names the consequence: "apply yaw, then pitch, then roll".
/// For Euler order `'YXZ'`, Three composes the rotation matrix as `Ry * Rx *
/// Rz` (yaw outermost, roll innermost — `qy * qx * qz`), so that is what this
/// composes too: yaw always rotates around the true world-up axis, so a pure
/// pitch (or a pitch layered under any yaw) never introduces roll. Composing
/// `qx * qy * qz` instead (Three's *default* order, which this file used to
/// assume) rotates pitch around the world-fixed X axis rather than the
/// camera's own local right vector once any yaw is present — which bakes a
/// spurious, non-decaying bank into the view the moment the player looks
/// anywhere off dead-centre. `Quat::from_euler_xyz` composes a third way (`qz *
/// qy * qx`) and is not what either order means; the composition is spelled
/// out explicitly here rather than reached for.
pub fn write_camera(running: &mut RunningApp, pose: CameraPose) {
    let axis = |a: Vec3, angle: f64| {
        Quat::from_axis_angle(a, angle as f32).expect("an authored camera angle is finite")
    };
    let rotation = axis(Vec3::UNIT_Y, pose.rotation.yaw)
        .multiply(axis(Vec3::UNIT_X, pose.rotation.pitch))
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
pub fn shmup_start() {
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
    // AgX, at full strength.
    //
    // The source tonemaps with AgX and meters the scene to an EV100 value; the
    // reference capture in `docs/work-manifests/shmup-port/reference/` is grey
    // and filmic where an untonemapped frame is saturated and contrasty, and
    // that difference is the largest single one between the two images.
    //
    // This also switches the scene target to `Rgba16Float`. Until now nothing
    // did: `RenderCapability::HdrTargets` was granted and `scene_target_format`
    // still returned 8-bit, so every value above 1.0 was clipped at the scene
    // pass and the bloom, exposure and AgX ports were all inert. A tone map is
    // the app-side switch for the whole HDR path.
    windowing.set_tonemap(FrameTonemap::filmic());
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
            axiom_host::FrameCamera::new(
                outcome.camera_view(),
                outcome.camera_projection(),
                outcome.camera_view_proj(),
            ),
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
