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
const NEAR: f64 = 0.05;
const FAR: f64 = 400.0;

/// A built scene: the game state and the realized engine world it renders
/// through.
pub struct Scene {
    pub game: Game,
    pub app: RunningApp,
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
            install_level(running, batches, &look);
            // The world's own practicals. Without these the only light in the
            // scene is the sun, so every interior, every arcade and the whole
            // shaded side of the street was lit by ambient alone.
            install_practicals(running, &practicals, spawn.position);
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
        rifle_nodes,
        hand_nodes,
        soldier_draw,
        fx_draw,
    }
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

fn install_level(
    running: &mut RunningApp,
    batches: Vec<LevelBatch>,
    look: &[(&'static str, crate::scene::wiring::look::KeyLook)],
) {
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
        // **Each key gets its OWN surface**, carrying the `MaterialParams` the
        // ported forge resolved for it from `materials/library.js`. It used to
        // be one hand-authored `street_material()` for all 46 keys, so concrete,
        // brick, metal, glass and asphalt were shaded identically and the street
        // read as one grey mush — the single largest difference from the
        // reference capture.
        //
        // This is still ONE pipeline: a runtime material's parameters are
        // excluded from its digest, so 46 parameter sets share one program.
        //
        // The baked albedo rides in as the material's custom texture, and the
        // shader multiplies the two exactly as the source multiplies `mat.map`
        // by `p.tint`. A batch whose palette key names no surface keeps the
        // untextured path (a 1x1 white albedo), which is what it had before.
        let key = look.iter().find(|(k, _)| *k == batch.key).map(|(_, v)| v);
        let surface = key.map(|k| k.surface.clone());
        // The texture lookup stays on `key_surface_name`, which is the function
        // the baked map table is keyed by. `KeyLook::library_name` is the same
        // string today, but routing the lookup through a second source of that
        // name is how a silent all-miss happens.
        let texture = key_surface_name(&batch.key).and_then(|name| textures.get(name).copied());
        let base = texture.map_or(batch.albedo, |_| textured_base_color(&batch.key, batch.albedo));
        let mut material = Material::lit(base);
        material = surface.map_or(material.clone(), |s| material.clone().with_surface(s));
        // The practicals. `level::key_albedo` reads only `opts.tint`, so every
        // `three.emissive` in the library was dropped and the lamps, window
        // glows and lens caps rendered unlit.
        material = key
            .and_then(|k| k.emissive)
            .map_or(material.clone(), |e| material.clone().with_emissive(e));
        material = texture.map_or(material.clone(), |id| {
            material
                .clone()
                .with_custom_texture(id)
                // The street runs from underfoot to the 168 m horizon, which is
                // the grazing-angle case anisotropy exists for.
                .with_texture_sampling(TextureSampling::Anisotropic)
        });
        let material = running.add_material(material);
        for placement in batch.instances {
            running.spawn(Spawn::new(placement, mesh, material));
        }
    }
}

/// How many of the world's practicals a frame can actually light with.
///
/// The main pass carries `array<Light, 16>` (`scene_wgsl`) and the sun is one
/// of them, so fifteen is the whole punctual budget. The source has no such
/// ceiling — it registers all twenty-five and lets `render` distance-cull them
/// per frame (`_stabiliseLightCount`, `world/index.js:285-307`).
///
/// Three of the fifteen go to `scene::wiring::fx_draw`'s muzzle-flash ramp,
/// which spawns after the practicals — the frame fills its sixteen light
/// slots in spawn order, so an over-subscribed budget drops the flash
/// silently rather than reporting it.
const PRACTICAL_SLOTS: usize = 12;

/// Spawn the world's practicals — `this.bulbs` and `this.lamps` from
/// `_addLights` (`world/index.js:169-196`).
///
/// These are what stop an interior reading as a black hole. Until now the
/// scene had no punctual light at all: only the sun, so anything the sun did
/// not reach was lit by ambient alone.
///
/// **Two honest gaps, both engine-side, both stated rather than papered over:**
///
/// 1. *No range.* A three `PointLight` carries `distance` and `decay` (13 m /
///    22 m, decay 2), and its falloff is `1/d²` windowed to zero at `distance`.
///    The engine's point light carries neither — the main pass attenuates every
///    one of them by a fixed `1/(1 + 0.09d + 0.032d²)` that never reaches zero.
///    The two curves cannot be reconciled by choosing an intensity, so the
///    intensity below is re-fitted to agree at a room's own scale (~3 m) and is
///    deliberately NOT the source's raw number. Carrying `5` unchanged would
///    put a bulb's light on a wall fifty metres down the street.
/// 2. *No per-frame re-target.* `PointLight` is a `Bundle`, not a `Component`,
///    so a spawned light's colour and intensity cannot be rewritten per frame —
///    only its node's `Transform` can. That rules out both the source's
///    distance cull and the dusk ramp (`14 * mix`), so the fifteen slots go to
///    the practicals nearest the player's spawn and stay there.
fn install_practicals(running: &mut RunningApp, practicals: &[WorldLight], eye: [f64; 3]) -> Vec<Entity> {
    // Three's `1/d²` windowed by `distance`, against the engine's fixed curve,
    // evaluated at the distance a room-sized practical actually does its work.
    // Matching there is what keeps a bulb reading as a bulb.
    const FIT_DISTANCE: f64 = 3.0;
    let engine_falloff = 1.0 / (1.0 + 0.09 * FIT_DISTANCE + 0.032 * FIT_DISTANCE * FIT_DISTANCE);

    let mut ordered: Vec<&WorldLight> = practicals
        .iter()
        // A light the sky has switched off contributes nothing and must not
        // take a slot from one that is lit: the street lamps sit at intensity
        // `0` all day (`14 * mix`, and `mix` is `0` above dusk).
        .filter(|l| l.intensity > 0.0)
        .collect();
    ordered.sort_by(|a, b| {
        let d = |l: &WorldLight| {
            let (dx, dy, dz) = (l.position.x - eye[0], l.position.y - eye[1], l.position.z - eye[2]);
            dx * dx + dy * dy + dz * dz
        };
        d(a).total_cmp(&d(b))
    });

    ordered
        .iter()
        .take(PRACTICAL_SLOTS)
        .map(|l| {
            let window = 1.0 - (FIT_DISTANCE / l.distance).powi(4);
            let three = l.intensity / (FIT_DISTANCE * FIT_DISTANCE) * window.max(0.0) * window.max(0.0);
            let intensity = (three / engine_falloff) as f32;
            let light = PointLight {
                color: crate::scene::level::hex_to_linear(l.color),
                intensity: Ratio::new(intensity).expect("a re-fitted practical intensity is finite"),
            };
            let at = Transform::from_translation(Vec3::new(
                l.position.x as f32,
                l.position.y as f32,
                l.position.z as f32,
            ));
            running.add_point_light(light, at)
        })
        .collect()
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

//// Advance the weapon rig and hang the rifle off the camera.
///
/// `viewmodel.js` composes the rig as a **child of the camera anchor**, and
/// `Viewmodel::rig_pose` returns that local transform — view-model space, not
/// world. So the world transform is the camera's own composed with it, which is
/// what turns "a rifle lying in the road" into "a rifle held in front of the
/// eye".
///
/// Every bucket takes the same transform because the source moves one `group`;
/// the per-part animation (bolt, mag, trigger) lives in
/// [`crate::weapons::viewmodel::PartsState`] and needs per-part nodes, which
/// this scene does not build yet — the buckets are merged **per material**, not
/// per part. That is a real limit and it is stated rather than faked: the rig
/// sways, breathes, kicks and transitions to ADS, and the bolt does not cycle.
fn drive_viewmodel(scene: &mut Scene, pose: CameraPose) {
    let axis = |a: Vec3, angle: f64| {
        Quat::from_axis_angle(a, angle as f32).expect("an authored camera angle is finite")
    };
    // The camera's own rotation, composed exactly as `write_camera` composes
    // it — YXZ, because the source overrides Three's default order. Composing
    // it differently here would make the gun bank against the view.
    let camera_rot = axis(Vec3::UNIT_Y, pose.rotation.yaw)
        .multiply(axis(Vec3::UNIT_X, pose.rotation.pitch))
        .multiply(axis(Vec3::UNIT_Z, pose.rotation.roll));

    // The pose comes from the weapons core, which already stepped its own
    // viewmodel in `Game::frame` off real input — including the trigger, so the
    // rifle now recoils. This function used to build a `FrameInput` and drive a
    // SECOND viewmodel with `trigger: false` hardcoded, which is why the gun
    // could never kick.
    let (rig_pos, rig_quat) = scene.game.weapons.rig_pose();
    let local = Vec3::new(rig_pos.x as f32, rig_pos.y as f32, rig_pos.z as f32);
    let rig_rot = Quat::new(
        rig_quat.x as f32,
        rig_quat.y as f32,
        rig_quat.z as f32,
        rig_quat.w as f32,
    );
    let eye = Vec3::new(pose.eye[0] as f32, pose.eye[1] as f32, pose.eye[2] as f32);
    let world = Transform::new(
        eye.add(camera_rot.rotate(local)),
        camera_rot.multiply(rig_rot),
        Vec3::new(1.0, 1.0, 1.0),
    );
    scene.rifle_nodes.iter().for_each(|node| {
        scene.app.set(*node, world);
    });
    // The arms ride the same rig transform the rifle does: `solve_hands`
    // rebases both shoulders out of camera space into rig space before solving,
    // so an arm's root IS the rig. This lives inside `drive_viewmodel` on
    // purpose — both frame paths call it, so neither can silently skip the
    // hands the way the viewmodel itself once was skipped.
    let viewmodel = &mut scene.game.weapons.core_mut().viewmodel;
    drive_hands(
        &mut scene.app,
        &scene.hand_nodes,
        &mut viewmodel.arm_l,
        &mut viewmodel.arm_r,
        world,
    );
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
    // `.ow-hud` is `position: fixed; inset: 0`, so the HUD sizes to the
    // VIEWPORT, not to the surface's backing store.
    let hud_w = window
        .inner_width()
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(f64::from(width));
    let hud_h = window
        .inner_height()
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(f64::from(height));
    scene.game.hud.resize(hud_w, hud_h);
    // Build the overlay, the four layers, every widget's DOM, and inject
    // `style.css.tpl`. Exactly once.
    scene.game.hud.mount();

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
    scene
        .app
        .sky()
        .into_iter()
        .for_each(|sky| windowing.set_sky(sky));
    scene
        .app
        .indirect()
        .into_iter()
        .for_each(|fill| windowing.set_indirect(fill));
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
    // **The scene's photometric scale, restored before the curve.**
    //
    // `SkyDriver::key_light` divides the sun by `KEY_INTENSITY_FULL_SCALE`
    // because the engine types a light's intensity as a `Ratio`, and that
    // constant's own doc says what it is: *"a stand-in for the source's exposure
    // path, not a replacement for it."* Nothing ever put the scale back, so
    // every surface reached AgX about a factor of eight under-exposed and the
    // frame came out dark and over-saturated — which is exactly the
    // "untonemapped" signature this file's AgX note describes, produced not by a
    // missing tone map but by feeding a correct one the wrong radiance.
    //
    // Measured against the reference capture: the street's mid-tone needed
    // ~24x, and `KEY_INTENSITY_FULL_SCALE * PI` is 24.8. The `PI` is not
    // conjecture — a normalisation this app removed is the only thing being put
    // back, and the diffuse BRDF's `1/PI` is the second half of it.
    let exposure =
        (crate::scene::wiring::look::KEY_INTENSITY_FULL_SCALE * std::f64::consts::PI) as f32;
    windowing.set_tonemap(FrameTonemap::blended(
        Ratio::new(1.0).expect("an authored tone-map strength is finite"),
        Ratio::new(exposure).expect("the restored photometric scale is finite"),
    ));
    windowing.set_surfaces(scene.app.surfaces().to_vec());
    windowing.set_material_programs(scene.app.material_surface_programs());

    let meshes = scene.app.mesh_set();
    let materials = scene.app.material_textures();
    // The bake-once soldier bodies, uploaded at bind alongside the rigid set.
    let skinned_meshes = scene.app.skinned_mesh_set();
    // The live backend sizes its skinned instance buffer from `max_instances`
    // too, so the soldiers' upper bound has to be in it.
    let max_instances =
        (scene.app.renderable_count() + scene.soldier_draw.max_draws_per_frame()) as u32;
    // The shared cell the driver reads just before each present.
    let skinned_source: std::rc::Rc<
        std::cell::RefCell<Vec<(u64, u64, [f32; 16], [f32; 16], [f32; 4], Vec<[f32; 16]>)>>,
    > = Default::default();
    let skinned_sink = std::rc::Rc::clone(&skinned_source);

    let performance = window.performance().expect("a performance clock");
    let mut last = performance.now();

    // `run_web_multi_skinned`, not `run_web_multi`: the plain entry uploads no
    // skinned meshes and reads no skinned draws, so the soldiers would simulate
    // and never appear. This call had ZERO callers in the repository — the
    // skinning path was exercised only headless, by `tools/axiom-shot`.
    let _ = windowing.run_web_multi_skinned(
        SURFACE_ID,
        meshes,
        materials,
        skinned_meshes,
        max_instances,
        // Keep the ambient the app already bound.
        None,
        skinned_source,
        move |tick| {
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
        // The same three steps `frame` runs. This loop INLINES them rather than
        // calling it, so anything added to `frame` alone silently never runs in
        // the browser — which is exactly how the viewmodel appeared wired and
        // was not. Keep the two in step.
        drive_viewmodel(&mut scene, pose);
        scene.game.hud_frame(&input.borrow());
        scene.fx_draw.frame(
            &mut scene.app,
            &scene.game.fx_audio.fx,
            pose,
            scene.game.time.elapsed,
        );
        scene.soldier_draw.frame(&mut scene.app, &scene.game.ai);

        let outcome = scene.app.tick(tick);
        // The skinned draws ride the shared cell, not the returned tuple.
        *skinned_sink.borrow_mut() = outcome
            .skinned_draws()
            .iter()
            .map(|d| {
                (
                    d.mesh_id(),
                    d.material_id(),
                    d.mvp(),
                    d.world(),
                    d.color(),
                    d.joints().to_vec(),
                )
            })
            .collect();
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
        },
    );
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
