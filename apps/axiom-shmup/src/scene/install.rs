//! **Installing the built game into the engine's scene.**
//!
//! The app tier's translation between module contracts, which is where
//! `CLAUDE.md` puts it: `axiom-scene` produces a level, `axiom-resources`
//! produces materials, `axiom` consumes meshes and nodes, and no module may name
//! another's types. Something in `apps/` has to carry a batch across, and this
//! is that something.
//!
//! Split out of `scene::app`, which had grown into a composition root, a scene
//! installer, a frame loop and a browser entry point at once — and therefore had
//! to be edited five times for every capability added to the game. Uploading
//! geometry has nothing to do with driving a frame; they shared a file for no
//! reason but history.
use std::collections::BTreeMap;

use axiom::prelude::*;

use crate::config::Quality;
use crate::materials::upload::{bake_library, BakeCaps, Rgba8Map};
use crate::scene::level::{key_albedo, LevelBatch};
use crate::viewer::to_mesh_data;
use crate::world::palette::Palette;
use crate::world::system::WorldLight;



/// The three per-material maps one palette key uploads: albedo, tangent-space
/// normal, and the `(occlusion, roughness, metalness, height)` pack.
#[derive(Clone, Copy)]
struct KeyMaps {
    albedo: u64,
    normal: u64,
    orm: u64,
    /// Binding 5 and binding 6. Both are **library-wide**, not per-surface:
    /// `buildDetail`/`buildMacro` are built once by `MaterialSystem` and every
    /// material in the source samples the same two (`index.js:207-210`), so the
    /// same pair of ids is copied into every key's entry rather than uploaded
    /// forty-six times.
    detail: u64,
    macro_: u64,
}

/// Register one baked map's pixels and hand back its id.
///
/// A free function rather than a closure over `running`, because the two shared
/// maps are uploaded ONCE above the per-surface loop and a closure holding the
/// mutable borrow across both would not compile.
fn upload_map(running: &mut RunningApp, m: &Rgba8Map) -> u64 {
    running
        .add_texture_data(m.width, m.height, m.pixels.clone())
        .expect("a baked map is width * height * 4 bytes")
        .id()
}

/// Bake the palette's surfaces and register **all five** maps per library name.
///
/// This used to bake and upload the albedo alone, behind a comment saying it was
/// "the only map the engine can be handed" because `axiom_host::MaterialTexture`
/// "carries albedo pixels and nothing else". That expired without anyone
/// re-reading it: `MaterialTexture` grew `with_normal`, `with_orm_height`,
/// `with_detail` and `with_macro_field`, and `RunningApp::material_textures`
/// already fills all four from the material's own slots. No engine change was
/// needed — the app was answering a question the engine had stopped asking.
///
/// It buys more than crispness. The ORM pack carries **per-texel roughness and
/// metalness**, which is the difference between glass, brick and steel reading
/// as three materials instead of three tints of one; the normal is what gives a
/// wall relief under a moving sun. Both are layers the source's own palette
/// comment calls out as what "stop a wall reading as one flat colour".
///
/// The same argument, one round later, is why the two **shared** maps go up
/// here too. `bake_library` has always returned `detail` and `macro_field`
/// beside the per-surface maps and nothing read them, so bindings 5 and 6 held
/// `scene_renderer`'s 1x1 neutrals — and neither neutral is that layer's
/// identity:
///
/// * the detail neutral's alpha is `0`, and `axiom_detail_micro` decodes it as
///   `(a - 0.5) * 2 = -1` — a **permanent full trough**. Its justification
///   (`scene_renderer.rs:1215-1218`) is that `owDetailP.z` is 0 for a material
///   with no detail block, which is true of most of the engine and false of
///   every key here: the shmup's palette authors real `detail` blocks, so every
///   surface was being darkened by `0.95 * detail[2]` and having its AO cut by
///   `0.30 * detail[2]` at every near-field pixel.
/// * the macro neutral's alpha is `255`, i.e. `1.0`, against a layer whose
///   midpoint is `0.5`. `masks.rs:168`'s `smoothstep(0.25, 0.85, mac1.b * 0.65 +
///   mac2.a * 0.55)` reads `0.876` — past the upper edge — so `wearN` was pinned
///   at 1.0, and `weathering.rs:229`'s `smoothstep(0.30, 0.66, sN * 0.72 +
///   sFine * 0.38)` reads `0.911` and was pinned the same way. The street was
///   running at maximum wear and maximum rain streak, uniformly, with no spatial
///   structure — which is most of what makes it read as one grey mush.
///
/// So this is not "two more textures for crispness". It is the difference
/// between two layers running on a saturated constant and running on the field
/// the generator actually baked.
fn install_surface_textures(running: &mut RunningApp) -> BTreeMap<&'static str, KeyMaps> {
    let names: Vec<&'static str> = Palette::ALL
        .iter()
        .map(|(_, entry)| entry.name)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    // TWO caps, not one: the nineteen per-surface bakes and the two shared ones
    // are different budgets and were never the same number. See `BakeCaps`.
    let library = bake_library(Quality::Ultra, BakeCaps::RUNTIME, &names);
    // One upload per distinct BAKE KEY, then every name that resolved to it
    // points at the same three handles. Forty-six palette entries collapse onto
    // nineteen bakes, and uploading per name would pay for the duplicates.
    // The two shared maps go up ONCE for the whole library and every key points
    // at the same pair. `bake_library` returns them beside the per-surface maps
    // and nothing used to read them, so bindings 5 and 6 held the backend's 1x1
    // neutrals and the detail and macro layers ran as algebraic identities.
    let detail = upload_map(running, &library.detail);
    let macro_ = upload_map(running, &library.macro_field);
    let mut by_key: BTreeMap<String, KeyMaps> = BTreeMap::new();
    library.surfaces.iter().for_each(|(key, maps)| {
        let ids = KeyMaps {
            albedo: upload_map(running, &maps.albedo),
            normal: upload_map(running, &maps.normal),
            orm: upload_map(running, &maps.orm_height),
            detail,
            macro_,
        };
        by_key.insert(key.clone(), ids);
    });
    names
        .iter()
        .filter_map(|name| {
            library
                .names
                .iter()
                .find(|(n, _)| n == name)
                .and_then(|(_, key)| by_key.get(key))
                .map(|ids| (*name, *ids))
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
fn textured_base_color(_key: &str, _batch_albedo: Color) -> Color {
    // WHITE, unconditionally. This used to return the palette entry's `tint` for
    // any key that had one, and white only for keys that did not -- which
    // applied the tint TWICE.
    //
    // `MaterialParams.tint` already carries it (`wiring::look::engine_params`,
    // `tint: p.tint`) and `material_shader`'s tint/wear layer applies it inside
    // the surface program. Multiplying the base colour by the same tint on the
    // way in squares it: a 0.7 warm tint becomes 0.49, and every hue pulls
    // further from neutral than the palette authored.
    //
    // Measured on the `hero` shot against the original, before this fix: the
    // port ran 1.87x warmer (R/B on the luma-normalised triple) and 2.46x more
    // saturated, with shadow chromaticity inverted -- 1.66 warm where the
    // original is 0.84 cool. Squaring a warm tint does exactly that, because it
    // moves saturation superlinearly while a single application does not.
    //
    // The old doc had the source's own answer quoted in it and drew the wrong
    // conclusion from it: `materials/index.js:199` constructs the material with
    // `color: 0xffffff`. That is white for EVERY key, not just untinted ones --
    // the generator and the shader own the colour, and the material contributes
    // an identity multiply.
    Color::WHITE
}

pub fn install_level(
    running: &mut RunningApp,
    batches: Vec<LevelBatch>,
    look: &[(&'static str, crate::scene::wiring::look::KeyLook)],
    // Every spawned node is tagged with the palette key it is made of, so the
    // dev console can name it on screen. Installing is the only place that
    // knows both the identifier and the world position at once — after this
    // the batch is gone and the node is an opaque `Entity`.
    console: &mut crate::scene::console::DevConsole,
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
        // Every one of these three keys off the PALETTE key, not the merge
        // key. They read `batch.key` until instanced batches were re-keyed to
        // their prototype id, at which point every prop in the level lost its
        // surface program, its emissive and its maps in one go.
        let key = look
            .iter()
            .find(|(k, _)| *k == batch.palette_key)
            .map(|(_, v)| v);
        let surface = key.map(|k| k.surface.clone());
        // The texture lookup stays on `key_surface_name`, which is the function
        // the baked map table is keyed by. `KeyLook::library_name` is the same
        // string today, but routing the lookup through a second source of that
        // name is how a silent all-miss happens.
        let texture =
            key_surface_name(&batch.palette_key).and_then(|name| textures.get(name).copied());
        let base = texture.map_or(batch.albedo, |_| {
            textured_base_color(&batch.palette_key, batch.albedo)
        });
        let mut material = Material::lit(base);
        material = surface.map_or(material.clone(), |s| material.clone().with_surface(s));
        // The practicals. `level::key_albedo` reads only `opts.tint`, so every
        // `three.emissive` in the library was dropped and the lamps, window
        // glows and lens caps rendered unlit.
        material = key
            .and_then(|k| k.emissive)
            .map_or(material.clone(), |e| material.clone().with_emissive(e));
        material = texture.map_or(material.clone(), |ids| {
            material
                .clone()
                .with_custom_texture(ids.albedo)
                // The two maps that were baked and never bound. The normal gives
                // the wall relief under a moving sun; the ORM pack carries
                // per-texel roughness and metalness, which is what separates
                // glass from brick from steel instead of leaving them three
                // tints of one flat response.
                .with_normal_texture(ids.normal)
                .with_orm_texture(ids.orm)
                // The micro-detail tile and the macro variation field: the two
                // layers whose stated job is to stop a wall reading as one flat
                // colour. Bound to the backend's neutrals they multiply by one.
                .with_detail_texture(ids.detail)
                .with_macro_texture(ids.macro_)
                // The street runs from underfoot to the 168 m horizon, which is
                // the grazing-angle case anisotropy exists for.
                .with_texture_sampling(TextureSampling::Anisotropic)
        });
        let material = running.add_material(material);
        // A statics batch is one identity transform covering baked-in world
        // geometry, so its position says nothing; tag it at each instance only
        // for the props, where the transform IS where the thing is.
        let kind = [
            crate::scene::console::KIND_STATIC,
            crate::scene::console::KIND_PROP,
        ][usize::from(batch.instances.len() > 1)];
        for placement in batch.instances {
            let at = placement.translation;
            console.tag(
                &batch.palette_key,
                kind,
                [f64::from(at.x), f64::from(at.y), f64::from(at.z)],
            );
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
pub fn install_practicals(running: &mut RunningApp, practicals: &[WorldLight], eye: [f64; 3]) -> Vec<Entity> {
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

