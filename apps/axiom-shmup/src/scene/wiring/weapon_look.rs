//! **The gun's look, and the hands** — the composition step that turns
//! [`crate::weapons::materials`] (the ported `weapons/materials.js`) and
//! [`crate::weapons::hands`] (the ported `weapons/hands.js` arm rig) into the
//! engine contracts a frame actually draws.
//!
//! Nothing here is a port of a single source file. It is the wiring
//! `weapons/index.js` performs when it hands `WeaponMaterials.get(key)` to a
//! `THREE.Mesh` and parents the two [`Arm`]s under the viewmodel rig — the same
//! shape [`crate::scene::wiring::look`] already has for the street.
//!
//! # 1. The rifle was wearing a debug palette
//!
//! `scene::app::install_rifle` bound `Material::lit(bucket_color(bucket))`.
//! [`crate::viewer::bucket_color`] says what it is in its own doc comment: "the
//! viewer's own reading of them, not the game's material system". It is nine
//! hand-picked greys, written for a turntable that had no way to bind a shader
//! graph. The rifle has been rendering in it ever since, which is why it is
//! untextured — and `weapons/materials.rs`, 1,678 lines carrying the real
//! fifteen-entry table, had exactly one consumer for one constant
//! (`ENV_OCCLUSION`, read by nothing in the scene either).
//!
//! [`WeaponLook`] is the replacement, and it is the level's own path with the
//! weapon table substituted for the palette:
//!
//! ```text
//!   MaterialLook  (street)                WeaponLook  (gun + hands)
//!   Palette::ALL           -> opts        WEAPON_MATERIALS       -> opts
//!   MaterialSystem::get    -> params      MaterialSystem::get    -> params
//!   engine_params          -> Surface     engine_params          -> Surface
//!   bake_albedo_maps       -> texture     TextureSet::bake_at    -> texture
//! ```
//!
//! The middle two rows are literally the same functions
//! ([`crate::materials::system::MaterialSystem`] and
//! [`crate::scene::wiring::look::engine_params`]); only the `opts` differ, which
//! is exactly what `materials.js` is — "a re-parameterisation of the shared
//! procedural PBR library for hand-held scale" (its own module doc).
//!
//! # 2. The hands were built, solved, and never drawn
//!
//! `weapons/hands.rs` is **not** unreferenced: [`crate::weapons::viewmodel`]
//! constructs both arms in `Viewmodel::new` and `Viewmodel::solve_hands` runs the
//! two-bone IK every frame inside `late_update`. What was missing is only the
//! last step — nothing ever turned [`Arm::meshes`] into engine geometry, so the
//! player holds the rifle with two fully-posed invisible arms.
//!
//! [`HandGeometry`] takes that rig apart once at build time and
//! [`drive_hands`] writes one [`Transform`] per part per frame.
//!
//! ## Why retained nodes and not `submit_skinned_draw`
//!
//! An arm is a rigid hierarchy of ~45 meshes hanging off ~18 animated frames, so
//! the obvious economy is one skinned mesh per (arm, surface) with the node
//! matrices as the joint palette — 8 draws instead of ~82. It was rejected, and
//! the reason is a **backend limit, not a preference**:
//! `axiom-gpu-backend`'s skinned pass sets exactly one pipeline
//! (`scene_renderer.rs`'s `skinning.pipeline`) for every skinned draw in the
//! frame, so a skinned draw **never binds a surface program**. Routing the hands
//! through it would trade the whole procedural material — the thing this slice
//! exists to bind — for a draw-call saving. See the notes file; it is written up
//! as an engine finding rather than worked around here.
//!
//! ## The mirror is baked, not transformed
//!
//! `handInner.scale.x` is `-1` on one arm ([`Arm::hand_mirror_x`]) and it is the
//! only non-unit scale in the whole arena. A [`Transform`] can carry a `-1`
//! scale, but the renderer transforms normals by the world matrix rather than by
//! its normal matrix, so a mirrored node would light from the inside. The mirror
//! is a *fixed* ancestor scale — pose changes only ever write rotations — so
//! [`HandGeometry`] applies it to the geometry once, at build, and every
//! per-frame transform stays a pure rigid motion.
//!
//! # Three honest gaps, all engine-side
//!
//! 1. **The vertex masks cannot arrive.** Every weapon material sets
//!    `vertexMasks: true`, and the edge wear it selects is driven by a per-vertex
//!    curvature mask ([`crate::materials::masks`]). `MeshData` has no colour
//!    stream, `interleave_vertices` writes an opaque white constant, and
//!    `material_shader::compose` passes `vec3<f32>(0.0)` for `vColor` with a
//!    comment saying so. The mask therefore reads **zero** — no wear, rather than
//!    wear everywhere — so the flag is inert, not wrong.
//! 2. **A tint above 1 clips.** `axiom_surface::MaterialParams::tint` is an sRGB
//!    `u32`; the table's tint is a linear `THREE.Color` and for the five
//!    `metalness: 1` entries it is an **F0**, not an albedo — `brass` is
//!    `(2.3, 1.58, 0.74)`. [`linear_to_hex`] clamps to 1 and quantises to 8 bits.
//! 1b. **The albedo bake is resolution-starved, and the cure already exists in
//!    this app.** [`weapon_bake_size`] carries the arithmetic: this table's
//!    detail layer packs 9-30 detail cells into one base tile, so the street's
//!    64² bake gives it 2.1-7.1 texels per cell and the finest entries fall at
//!    or under the Nyquist limit — the grain cannot be represented, and the
//!    per-pixel weathering layers that excuse a coarse bake on the street are
//!    switched off here by `BASE.weather`. This slice raises the cap to 128²
//!    above `Quality::Low`, which is the largest step a *CPU* bake can afford
//!    at boot. The table actually wants 256². **Hand-off:**
//!    [`crate::materials::gpu_bake`] already exists, already emits the WGSL for
//!    `owSurface`, and already has a CPU/GPU parity test; routing
//!    [`WeaponLook::new`]'s bake through it removes the quadratic boot cost and
//!    with it this ceiling. That file is owned elsewhere, so the change is
//!    named here rather than made.
//! 3. **`MeshBasicMaterial` and additive blending have no counterpart.** The five
//!    custom keys the source owns outright (`cavity`, `optic_tube`, `glass`,
//!    `lens_ring`, `lens_vig`) include two unlit additive overlays. An unlit
//!    material is approximated as a **black albedo carrying the colour as
//!    emissive**, which is the closest the fixed material path gets; the additive
//!    blend mode is dropped and the material blends normally on its opacity.

use std::collections::BTreeMap;

use axiom::prelude::*;
use axiom_math::Quat;

use crate::config::Quality;
use crate::materials::bake::Texture as BakedTexture;
use crate::materials::system::{MaterialOpts, MaterialSystem, OptValue, RendererCaps};
use crate::materials::upload::{quantize, Rgba8Map, RUNTIME_BAKE_SIZE};
use crate::scene::wiring::look::engine_params;
use crate::viewer::to_mesh_data;
use crate::weapons::geometry::{merge_all, Geo};
use crate::weapons::hands::{geo_apply, m4_scale, Arm, HandSurface};
use crate::weapons::materials::{
    fallback, hex_to_linear, material_keys, material_request, CustomMaterial, FallbackMaterial,
    MaterialKind, MaterialRequest, WeaponMaterial,
};
use crate::weapons::rig_math::{M4, Q, V3};

/* ==================================================================== */
/* the material table                                                    */
/* ==================================================================== */

/// The five keys `WeaponMaterials.get` answers **before** it consults
/// `WEAPON_MATERIALS` (`materials.js:883-910`). They are not in
/// [`material_keys`], so they have to be named here or the optic's glass, its
/// tube, its lens ring, its vignette and every bore cavity fall through to
/// `_fallback` and render as flat grey.
const CUSTOM_KEYS: [&str; 5] = ["cavity", "optic_tube", "glass", "lens_ring", "lens_vig"];

/// One weapon material key resolved into the engine's contracts, before any of
/// it is registered on a [`RunningApp`].
///
/// The split between this and [`WeaponMaterials`] is the one the engine forces:
/// resolving the table and running the CPU bake need no app, and registering a
/// texture or a material needs a realized one. So [`WeaponLook::new`] does the
/// first half at `Game` construction (where the quality knob lives) and
/// [`WeaponLook::install`] does the second inside `App::install`.
#[derive(Debug, Clone)]
pub struct WeaponKeyLook {
    /// The `WEAPON_MATERIALS` key, or one of [`CUSTOM_KEYS`].
    pub key: &'static str,
    /// The runtime-material surface carrying this key's own resolved
    /// `axiom_surface::MaterialParams`, or `None` for a custom key (which the
    /// source builds as a plain THREE material, with no shader graph at all).
    pub surface: Option<Surface>,
    /// What the surface modulates. `WHITE` for a library key — the tint lives in
    /// the surface parameters, exactly where `materials.js` puts it — and the
    /// authored colour for a custom key.
    pub base_color: Color,
    /// Set only for an unlit (`MeshBasicMaterial`) custom key. See the module
    /// doc's gap 3.
    pub emissive: Option<Color>,
    pub opacity: Ratio,
    /// Which entry of [`WeaponLook::bakes`] this key samples. Two keys that
    /// resolve to the same bake key share one texture — the collapse `bakeKey`
    /// exists for.
    pub bake_key: Option<String>,
}

/// Owns the ported [`MaterialSystem`] long enough to resolve every weapon
/// material, and holds the CPU bake its surfaces sample.
///
/// This is the weapons' [`crate::scene::wiring::look::MaterialLook`]. It is a
/// **second** `MaterialSystem` rather than a reuse of the level's, and that is
/// deliberate: `MaterialSystem::get` caches on `(library key, stableKey(opts))`,
/// so sharing one would be correct — but `MaterialLook` is owned by `Game` and
/// this is constructed inside `App::install`'s closure, which cannot borrow it.
/// The duplication costs one facade and the fifteen weapon bakes; it buys the
/// two tables independent lifetimes. If the two are ever merged, merge them at
/// `Game` and hand this a `&mut MaterialSystem`.
pub struct WeaponLook {
    keys: Vec<WeaponKeyLook>,
    /// `(bake key, albedo map)`, deduplicated — the same shape
    /// [`crate::materials::upload::bake_albedo_maps`] returns for the street.
    bakes: Vec<(String, Rgba8Map)>,
}

impl WeaponLook {
    /// Resolve every weapon material and bake its albedo.
    ///
    /// The bake is albedo-only — a CPU bake at the library's authored 1024² is
    /// minutes of work and the fix is the source's own GPU bake. The weapon
    /// table names three libraries (`rubber`, `metal_brushed`, `fabric`) but its
    /// entries override `bake.seed` and `bake.relief` per key, so it costs up to
    /// fifteen bakes rather than three — the same per-surface cost the street's
    /// nineteen already pay.
    ///
    /// Its *resolution*, though, is [`weapon_bake_size`] and deliberately not
    /// the street's [`RUNTIME_BAKE_SIZE`]; see that function for why 64² cannot
    /// represent this table's detail layer at all.
    ///
    /// `ground_y` is not a parameter: every weapon material zeroes the three
    /// world-space weathering terms (`BASE.weather = [0, 0, 0, 0.62]`) precisely
    /// because they are meaningless for something parented to the camera, and the
    /// ground-splash term is the only consumer of the ground height.
    pub fn new(quality: Quality) -> Self {
        let mut system = MaterialSystem::new(Some(RendererCaps {
            max_anisotropy: Some(8.0),
        }));
        // `configure` reports whether the quality moved; at construction it
        // always has, and nothing here acts on the answer.
        let _ = system.configure(quality, 8);
        system.set_ground_level(0.0);

        let size = weapon_bake_size(quality);
        let mut bakes: Vec<(String, Rgba8Map)> = Vec::new();
        let keys = CUSTOM_KEYS
            .into_iter()
            .chain(material_keys())
            .map(|key| resolve_key(&mut system, key, size, &mut bakes))
            .collect();
        WeaponLook { keys, bakes }
    }

    /// Every resolved key, in `WeaponMaterials.get`'s own check order (the five
    /// custom keys first, then `WEAPON_MATERIALS` in declaration order).
    pub fn keys(&self) -> &[WeaponKeyLook] {
        &self.keys
    }

    /// The distinct surfaces to declare at authoring time, deduplicated by
    /// content digest, so the preparation barrier compiles every program the
    /// frame will name **before** the first frame. A program the barrier never
    /// saw renders its constant fallback and reports the miss.
    ///
    /// With `detile` forced off by
    /// [`crate::scene::wiring::look::engine_params`] this is one surface, and it
    /// is very likely the *same* one `MaterialLook::surfaces` already returns (a
    /// runtime material's parameters are excluded from its digest). Concatenating
    /// the two lists is still the right call at the authoring site: a duplicate
    /// digest costs nothing, and a missing one costs the whole gun.
    pub fn surfaces(&self) -> Vec<Surface> {
        let mut seen: Vec<u64> = Vec::new();
        self.keys
            .iter()
            .filter_map(|look| look.surface.as_ref())
            .filter_map(|surface| {
                // By PARAMETER REGION, not digest: every runtime material
                // shares one digest, so deduplicating on it prepared a single
                // region for the whole weapon table and the rifle's fifteen
                // materials all shaded as whichever one survived.
                let key = surface.param_key().raw();
                let fresh = !seen.contains(&key);
                fresh.then(|| {
                    seen.push(key);
                    surface.clone()
                })
            })
            .collect()
    }

    /// Register the baked textures and the materials on a realized app, and hand
    /// back the key-to-handle table the two installers index.
    pub fn install(&self, running: &mut RunningApp) -> WeaponMaterials {
        let textures: Vec<(String, u64)> = self
            .bakes
            .iter()
            .map(|(bake_key, map)| {
                let handle = running
                    .add_texture_data(map.width, map.height, map.pixels.clone())
                    .expect("a baked weapon map is width * height * 4 bytes");
                (bake_key.clone(), handle.id())
            })
            .collect();

        let entries = self
            .keys
            .iter()
            .map(|look| {
                let texture = look
                    .bake_key
                    .as_ref()
                    .and_then(|want| textures.iter().find(|(have, _)| have == want))
                    .map(|(_, id)| *id);
                let mut material = Material::lit(look.base_color).with_opacity(look.opacity);
                if let Some(surface) = look.surface.clone() {
                    material = material.with_surface(surface);
                }
                if let Some(emissive) = look.emissive {
                    material = material.with_emissive(emissive);
                }
                if let Some(id) = texture {
                    material = material
                        .with_custom_texture(id)
                        // A 64-texel tile over a 95 mm weapon tile, seen from
                        // 0.4 m, is MAGNIFIED — `Crisp` point-samples a
                        // magnified texture by design, which on a gun this close
                        // is visible blocking. `Anisotropic` is the only variant
                        // that makes magnification linear.
                        .with_texture_sampling(TextureSampling::Anisotropic);
                }
                (look.key, running.add_material(material))
            })
            .collect();
        WeaponMaterials { entries }
    }
}

/// The registered weapon materials, by key.
pub struct WeaponMaterials {
    entries: Vec<(&'static str, Handle<Material>)>,
}

impl WeaponMaterials {
    /// This key's material, or `None` for a key neither `WEAPON_MATERIALS` nor
    /// the five custom keys carry.
    pub fn get(&self, key: &str) -> Option<Handle<Material>> {
        self.entries
            .iter()
            .find(|(name, _)| *name == key)
            .map(|(_, handle)| *handle)
    }
}

/// [`WeaponMaterials::get`], falling back the way the source does.
///
/// `_fallback(key)` (`materials.js:913-926`) is the arm the source takes when the
/// materials subsystem is unavailable; here it is the arm for a **bucket name no
/// table entry covers**, which is the same failure with a different cause and the
/// same right answer. Registering it lazily means an unknown bucket costs one
/// material rather than silently rendering the previous bucket's.
fn material_for(
    running: &mut RunningApp,
    materials: &WeaponMaterials,
    key: &str,
) -> Handle<Material> {
    materials.get(key).unwrap_or_else(|| {
        let FallbackMaterial {
            color, roughness, ..
        } = fallback(key);
        running.add_material(
            Material::lit(linear_color(hex_to_linear(color))).with_roughness(ratio(roughness)),
        )
    })
}

/* ==================================================================== */
/* resolving one key                                                     */
/* ==================================================================== */

/// The albedo bake resolution for a **weapon** surface, in texels per base tile.
///
/// This is deliberately *not* [`RUNTIME_BAKE_SIZE`]. That constant's doc
/// justifies 64² with two arguments, and **neither one survives the move from a
/// wall to a viewmodel**:
///
/// 1. *"64² over a 2 m tile is 3 cm per texel: coarse, but it is the real
///    generator's colour field."* That is an argument about world-space density
///    on something five metres away. The number that decides whether a weapon
///    looks like metal is not metres per texel, it is **texels per detail
///    cell** — and that is where 64 fails outright. `detail[0]` is the count of
///    detail tiles packed inside one base tile, and every entry in
///    `weapons/materials.rs` sets it between 9 and 30
///    (`materials.rs:287` = 22, `:339` = 30, `:574` = 26, `:611`/`:869` = 24).
///    At 64² the finest of those gets `64 / 30 = 2.1` texels per cell — *at*
///    the Nyquist limit, i.e. the detail layer cannot be represented at all and
///    bakes to aliased mush. `alu`'s own comment states exactly what is being
///    thrown away: "with diffuse in charge these are the texture ... 22 tiles
///    over a 95 mm base tile is a 4.3 mm cell".
/// 2. *"the material shader's macro, weathering and cavity layers — which are
///    per-pixel and cost nothing here — carry the high frequencies on top of
///    it."* For a weapon they do not. `BASE.weather` is `[0, 0, 0, 0.62]`:
///    `materials.js` **switches the three world-space weathering terms off**,
///    because they key off world Y and that is meaningless for something
///    parented to the camera. The per-pixel layers the street leans on to
///    excuse a coarse bake are, by design, absent from this exact table.
///
/// So the weapon needs its own number. 128² puts the worst entry at
/// `128 / 30 = 4.3` texels per cell and `alu` at 5.8 — across the Nyquist floor,
/// so the grain is *representable* rather than aliased away.
///
/// **Why not 256², which is what the table actually wants.** Eight texels per
/// cell is where a cell reads as grain rather than as noise, and that asks for
/// `30 * 8 = 240` → 256². The cost is the problem: this is boot-time CPU work,
/// quadratic in this number (see [`RUNTIME_BAKE_SIZE`] — `ow_hash22` is a
/// `f64::sin` per sample on a CPU), and 256² is 16x today's texels across up to
/// fifteen bakes. 128² is 4x, which is the bounded step that buys the
/// representability threshold. The real fix is not a bigger CPU bake at all: it
/// is [`crate::materials::gpu_bake`], which already exists in this app and
/// already has a CPU/GPU parity test — routing the weapon bake through it
/// removes this ceiling entirely. See the hand-off note in the module doc.
///
/// `Low` keeps today's 64² exactly, so a machine that cannot afford this does
/// not pay it. The result is still `min`-ed against the surface's own authored
/// size, so this can never ask for more than the generator actually authored.
fn weapon_bake_size(quality: Quality) -> u32 {
    match quality {
        Quality::Low => RUNTIME_BAKE_SIZE,
        Quality::Medium | Quality::High | Quality::Ultra => 128,
    }
}

/// `WeaponMaterials.get(key)`, translated into the engine's contracts.
///
/// `has_library` is `true` because this function *is* holding the library — the
/// source's `!!this.lib`, which is false only in the standalone harness.
fn resolve_key(
    system: &mut MaterialSystem,
    key: &'static str,
    size: u32,
    bakes: &mut Vec<(String, Rgba8Map)>,
) -> WeaponKeyLook {
    match material_request(key, true) {
        MaterialRequest::Custom(custom) => custom_look(key, &custom),
        MaterialRequest::Library { entry, .. } => library_look(system, entry, size, bakes),
        MaterialRequest::Fallback(f) => fallback_look(key, &f),
    }
}

/// A `WEAPON_MATERIALS` entry: the library surface it re-parameterises, resolved
/// through the real facade and baked.
fn library_look(
    system: &mut MaterialSystem,
    entry: &'static WeaponMaterial,
    bake_size: u32,
    bakes: &mut Vec<(String, Rgba8Map)>,
) -> WeaponKeyLook {
    let opts = weapon_opts(entry);
    let bake_key = system.texture_set_key(entry.library, &opts);

    // Bake on the first key that names this set; a second key naming it reuses
    // the map, exactly as `_sets` does.
    if let Some(want) = bake_key.as_ref() {
        if !bakes.iter().any(|(have, _)| have == want) {
            let map = system.texture_set(want).map(|set| {
                let size = set.size.min(bake_size);
                // Albedo only — see `RUNTIME_BAKE_SIZE`. The ORM and normal
                // passes cost a second and third full surface evaluation per
                // texel and have nowhere to be bound anyway
                // (`axiom_host::MaterialTexture` reaches the live arm with the
                // albedo alone).
                albedo_map(&set.bake_at(size, false, false).albedo)
            });
            if let Some(map) = map {
                bakes.push((want.clone(), map));
            }
        }
    }

    // `{ ...DEFAULT_PARAMS, ...LIBRARY[lib].mat, ...opts }`, performed by the
    // ported facade rather than re-derived here.
    let mut params = {
        let def = system.get(entry.library, &opts);
        engine_params(&def.params)
    };
    // `tint` cannot ride the opts bag: `apply_to_params` reads it through
    // `set_hex`, because every *palette* tint is a hex literal, and a weapon
    // tint is a linear `THREE.Color(r, g, b)` triple. Applying it here keeps the
    // one quantity the table cares most about (three of the fifteen entries carry
    // a measured exposure recalibration in it) instead of dropping it to the
    // library's default. See the module doc's gap 2 for what the encode loses.
    params.tint = linear_to_hex(entry.tint);

    WeaponKeyLook {
        key: entry.key,
        surface: Some(runtime_material(params)),
        // The runtime material MODULATES the albedo it is handed and the tint is
        // already in its parameters, so the instance colour must be neutral —
        // `install_level` reaches the same conclusion for an untinted street key.
        base_color: Color::WHITE,
        emissive: None,
        opacity: ratio(1.0),
        bake_key,
    }
}

/// One of the five materials `WeaponMaterials` constructs itself.
///
/// These carry no shader graph in the source either — they are plain
/// `MeshBasicMaterial` / `MeshPhysicalMaterial` literals — so they get no
/// surface here, only their authored colour, roughness and opacity.
fn custom_look(key: &'static str, m: &CustomMaterial) -> WeaponKeyLook {
    let color = linear_color(m.color);
    // `MeshBasicMaterial` is unlit. The fixed material path has no unlit arm, and
    // the nearest honest one is a BLACK albedo carrying the colour as emissive:
    // a lit white card is what an emissive over a lit albedo renders as.
    let unlit = m.kind == MaterialKind::Basic;
    WeaponKeyLook {
        key,
        surface: None,
        base_color: if unlit { Color::BLACK } else { color },
        emissive: if unlit { Some(color) } else { None },
        opacity: ratio(m.opacity.unwrap_or(1.0)),
        bake_key: None,
    }
}

/// `_fallback(key)` as a resolved look. Unreachable from [`WeaponLook::new`] —
/// every key it asks for is in one of the two tables — and kept because
/// `material_request` can return it and a `match` that pretends otherwise is a
/// lie the compiler cannot check.
fn fallback_look(key: &'static str, f: &FallbackMaterial) -> WeaponKeyLook {
    WeaponKeyLook {
        key,
        surface: None,
        base_color: linear_color(hex_to_linear(f.color)),
        emissive: None,
        opacity: ratio(1.0),
        bake_key: None,
    }
}

/// One `WEAPON_MATERIALS` row as the facade's `opts` bag.
///
/// The key names are the source's camelCase ones, because they are what
/// `apply_to_params` matches on and what `stableKey` hashes — a snake_case key
/// would be silently ignored by the first and would change the cache key in the
/// second. `bake` is nested and insertion-ordered, exactly as the source's object
/// literal is.
///
/// `tint` is **deliberately absent**: see [`library_look`].
fn weapon_opts(m: &WeaponMaterial) -> MaterialOpts {
    let bake = m.bake.map(|b| {
        let entries: Vec<(String, OptValue)> = [
            ("size", b.size.map(f64::from)),
            ("seed", b.seed.map(f64::from)),
            ("relief", b.relief),
            ("tintA", b.tint_a.map(f64::from)),
            ("tintB", b.tint_b.map(f64::from)),
        ]
        .into_iter()
        .filter_map(|(k, v)| v.map(|n| (k.to_string(), OptValue::Num(n))))
        .collect();
        OptValue::Obj(entries)
    });

    let authored: Vec<(&str, Option<OptValue>)> = vec![
        // `...BASE` (`materials.js:28-49`) first, as the spread has it.
        ("uvMode", Some(OptValue::Str(m.base.uv_mode.to_string()))),
        ("localSpace", Some(OptValue::Bool(m.base.local_space))),
        ("vertexMasks", Some(OptValue::Bool(m.base.vertex_masks))),
        ("weather", Some(num_array(&m.base.weather))),
        ("macro", Some(num_array(&m.base.macro_))),
        ("aoStrength", Some(OptValue::Num(m.base.ao_strength))),
        // ...then the entry's own overrides.
        ("bake", bake),
        ("scale", Some(OptValue::Num(m.scale))),
        ("roughness", Some(num_array(&m.roughness))),
        ("normalStrength", Some(OptValue::Num(m.normal_strength))),
        ("detail", Some(num_array(&m.detail))),
        ("wear", Some(num_array(&m.wear))),
        ("wearColor", Some(OptValue::Num(f64::from(m.wear_color)))),
        ("wearMaterial", Some(num_array(&m.wear_material))),
        (
            "grimeColor",
            m.grime_color.map(|c| OptValue::Num(f64::from(c))),
        ),
        ("cloth", m.cloth.map(|c| num_array(&c))),
    ];

    authored
        .into_iter()
        .filter_map(|(key, value)| value.map(|v| (key, v)))
        .fold(MaterialOpts::new(), |opts, (key, value)| {
            opts.with(key, value)
        })
}

/// A fixed-length `f64` array as a JS number array.
fn num_array<const N: usize>(values: &[f64; N]) -> OptValue {
    OptValue::Arr(values.iter().copied().map(OptValue::Num).collect())
}

/// One baked albedo [`BakedTexture`] as the RGBA8 map
/// `RunningApp::add_texture_data` takes.
///
/// The quantisation is [`quantize`], the same round-to-nearest the street's bake
/// goes through, so the two upload paths cannot drift apart in the low bit.
fn albedo_map(texture: &BakedTexture) -> Rgba8Map {
    let size = texture.size;
    let mut pixels = Vec::with_capacity((size as usize) * (size as usize) * 4);
    for y in 0..size {
        for x in 0..size {
            let t = texture.get(x, y);
            pixels.extend_from_slice(&[
                quantize(t[0]),
                quantize(t[1]),
                quantize(t[2]),
                quantize(t[3]),
            ]);
        }
    }
    Rgba8Map {
        width: size,
        height: size,
        pixels,
    }
}

/// A linear RGB triple as a [`Color`], clamped and NaN-safe so a value the table
/// pushes above one (the metal F0s) cannot reach the renderer as an unwrap panic.
fn linear_color(c: [f64; 3]) -> Color {
    Color::linear_rgb(ratio(c[0]), ratio(c[1]), ratio(c[2]))
}

/// A `Ratio` from an `f64`, clamped to `0..1` and NaN-safe.
fn ratio(v: f64) -> Ratio {
    Ratio::new(v.clamp(0.0, 1.0) as f32).unwrap_or(Ratio::finite_or_zero(0.0))
}

/// The inverse of `axiom_surface`'s own `hex_to_linear`, which is what
/// `axiom_surface::MaterialParams::pack` runs on the `tint` slot.
///
/// Round-trips a linear triple through the 8-bit sRGB encoding the parameter
/// block stores. Lossy in two ways, both stated in the module doc's gap 2: a
/// triple above 1 cannot keep its magnitude, and every channel quantises to a
/// byte.
///
/// # Why the over-one case normalises instead of clamping per channel
///
/// Two entries in the table are metal **F0**s rather than albedos, and both
/// exceed one: `brass` is `(2.3, 1.58, 0.74)` (`materials.rs:684`) and `copper`
/// is `(2.25, 1.4, 1.09)` (`:706`). Clamping each channel independently is not
/// merely lossy, it is lossy *in the one dimension that carries the material's
/// identity*: brass's ratio is `1 : 0.687 : 0.322`, a warm gold, and channel-wise
/// clamping rewrites it to `1 : 1 : 0.74` — a near-white with the gold gone.
/// Copper's `1 : 0.622 : 0.484` becomes `1 : 1 : 1`, i.e. **exactly neutral**:
/// the encode was deleting the entire hue of the cartridge brass and the bullet
/// jacket, the two warmest things on the weapon.
///
/// Scaling the triple by its largest channel instead keeps the chromaticity
/// exact and spends the whole loss on magnitude — which is the right trade here,
/// because this value is multiplied into a baked albedo by a renderer that has
/// no headroom above one anyway, so the magnitude was never going to survive.
/// The brightest channel still encodes to `0xff`, so nothing that was clipping
/// before moves.
///
/// A triple already at or below one is untouched: the thirteen dielectric
/// entries encode exactly as they did.
fn linear_to_hex(c: [f64; 3]) -> u32 {
    // `f64::max` returns the non-NaN operand, so a NaN channel cannot make the
    // divisor NaN and drag the other two channels down with it.
    let peak = c.iter().fold(0.0_f64, |a, &b| a.max(b));
    let scale = if peak > 1.0 { 1.0 / peak } else { 1.0 };
    let channel = |v: f64| {
        let srgb = linear_to_srgb(v * scale);
        ((srgb * 255.0).round().clamp(0.0, 255.0)) as u32
    };
    (channel(c[0]) << 16) | (channel(c[1]) << 8) | channel(c[2])
}

/// The sRGB opto-electronic transfer function — the exact inverse of the
/// electro-optical one `crate::scene::wiring::look` and
/// `axiom_surface::MaterialParams` both decode with.
fn linear_to_srgb(c: f64) -> f64 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/* ==================================================================== */
/* the rifle                                                             */
/* ==================================================================== */

/// Spawn the rifle's merged buckets and hand back their nodes — the replacement
/// for `scene::app::install_rifle`, with the debug palette gone.
///
/// The transform here is only the **first frame's**; `scene::app::drive_viewmodel`
/// moves these nodes every frame after. Every bucket takes the same transform
/// because the source moves one `group`.
///
/// A bucket name IS a `WeaponMaterials.get` key: `Assembly::add`'s bucket strings
/// are the material keys the builders pass (`"alu"`, `"steel"`, `"glass"`,
/// `"cavity"`, ...), which is what makes this a lookup rather than a translation
/// table.
pub fn install_rifle(
    running: &mut RunningApp,
    buckets: &BTreeMap<String, Geo>,
    seat: Vec3,
    yaw: f32,
    materials: &WeaponMaterials,
) -> Vec<Entity> {
    let rotation = Quat::from_axis_angle(Vec3::UNIT_Y, yaw).expect("authored yaw is finite");
    let transform = Transform::new(seat, rotation, Vec3::new(1.0, 1.0, 1.0));
    buckets
        .iter()
        .map(|(bucket, geo)| {
            let mesh = running
                .add_mesh_data(to_mesh_data(geo))
                .expect("a golden-pinned rifle bucket is valid renderable geometry");
            let material = material_for(running, materials, bucket);
            running.spawn(Spawn::new(transform, mesh, material))
        })
        .collect()
}

/* ==================================================================== */
/* the hands                                                             */
/* ==================================================================== */

/// Which arm a part belongs to. `Arm::side` is a `f64` `-1`/`+1` (the source's
/// own spelling); this is that, as something a lookup can switch on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandSide {
    /// `side = -1` — `Viewmodel::arm_l`, the support hand.
    Left,
    /// `side = +1` — `Viewmodel::arm_r`, the shooting hand.
    Right,
}

/// The weapon material key each glove surface wears.
///
/// `viewmodel.js:114-119` binds these four by name, which is why
/// `hands.rs`'s [`HandSurface`] enum has exactly four variants and why
/// `WEAPON_MATERIALS` carries exactly four cloth entries.
fn surface_key(surface: HandSurface) -> &'static str {
    match surface {
        HandSurface::Glove => "glove",
        HandSurface::Pad => "glove_pad",
        HandSurface::Seam => "glove_seam",
        HandSurface::Sleeve => "sleeve",
    }
}

/// One drawable piece of an arm: every mesh that hangs off one rig frame and
/// wears one surface, merged into a single geometry.
struct HandPart {
    side: HandSide,
    /// The rig frame whose world matrix drives this piece. A `THREE.Mesh` node in
    /// this rig is always at identity relative to its parent
    /// (`Arm::add_mesh_node` writes no transform), so the *parent* is the frame
    /// that actually moves, and two meshes sharing a parent and a surface can be
    /// merged into one draw.
    frame: usize,
    key: &'static str,
    geo: Geo,
    /// Whether the mirror was baked into [`Self::geo`] and must therefore be
    /// divided back out of the frame's world matrix. See the module doc.
    mirrored: bool,
}

/// Both arms' geometry, taken apart once so `App::install` can upload it.
///
/// Built **before** `App::build`, from the arms `Viewmodel::new` already
/// constructed, and carried into the install closure — the same shape the level's
/// batches and the rifle's buckets use.
pub struct HandGeometry {
    parts: Vec<HandPart>,
}

impl HandGeometry {
    /// Take both arms apart. Reads only; the arms stay owned by the viewmodel and
    /// keep animating.
    pub fn from_arms(left: &Arm, right: &Arm) -> HandGeometry {
        let mut parts = Vec::new();
        collect_parts(left, HandSide::Left, &mut parts);
        collect_parts(right, HandSide::Right, &mut parts);
        HandGeometry { parts }
    }

    /// How many draws these hands cost per frame. **This is the budget**, and it
    /// is not small: an arm is ~45 authored meshes on ~18 animated frames, and
    /// merging per (frame, surface) takes both arms to roughly eighty. There is
    /// no silent drop — every part is drawn — so the number is the honest cost of
    /// a rigid per-joint rig on a renderer whose only per-frame transform lane is
    /// one `Transform` per retained node.
    pub fn draw_count(&self) -> usize {
        self.parts.len()
    }
}

/// Group one arm's meshes by (rig frame, surface), merge each group, and bake the
/// chirality mirror into the geometry of any group that carries one.
fn collect_parts(arm: &Arm, side: HandSide, out: &mut Vec<HandPart>) {
    let mut groups: BTreeMap<(usize, HandSurface), Vec<Geo>> = BTreeMap::new();
    for node in &arm.nodes {
        if let Some(mesh_index) = node.mesh {
            let frame = node.parent.unwrap_or(arm.root);
            let mesh = &arm.meshes[mesh_index];
            groups
                .entry((frame, mesh.surface))
                .or_default()
                .push(mesh.geo.clone());
        }
    }

    for ((frame, surface), geos) in groups {
        let mut geo = merge_all(geos).expect("a group holds at least one geometry");
        let mirrored = accumulated_mirror(arm, frame) < 0.0;
        if mirrored {
            mirror_x(&mut geo);
        }
        out.push(HandPart {
            side,
            frame,
            key: surface_key(surface),
            geo,
            mirrored,
        });
    }
}

/// The product of every ancestor's `scale.x`, this frame's own included.
///
/// `handInner.scale.x` is the only non-unit scale the rig ever writes
/// (`hands.js:595`), so this is `-1` exactly on the subtree below the mirrored
/// hand and `+1` everywhere else — and, because a pose only ever writes
/// rotations, it never changes after construction. That invariance is what makes
/// baking the mirror into the geometry legitimate rather than a first-frame
/// approximation.
fn accumulated_mirror(arm: &Arm, frame: usize) -> f64 {
    let mut mirror = 1.0;
    let mut at = Some(frame);
    while let Some(index) = at {
        mirror *= arm.nodes[index].scale.x;
        at = arm.nodes[index].parent;
    }
    mirror
}

/// Reflect a geometry through the YZ plane: positions and normals through
/// `diag(-1, 1, 1)`, then the triangle winding reversed.
///
/// [`geo_apply`] is `hands.rs`'s own `BufferGeometry.applyMatrix4`, which
/// transforms normals by the **normal matrix** — for a reflection that is the
/// reflection itself, so the normals come out x-negated, which is right. What it
/// does not do is reverse the winding, and a negative-determinant transform
/// inverts triangle orientation, so that is done here.
///
/// **Not `Geo::flip_winding`.** That reverses the winding *and negates every
/// normal*, which is the "turn this surface inside out" operation
/// (`geometry.js:82-100`) — applying it here would undo the reflection's own
/// normal transform and light the mirrored hand from inside.
fn mirror_x(geo: &mut Geo) {
    // Normals BEFORE the flip. `merge_all` returns a single-element list
    // untouched, so a group of one can still be missing them — and a geometry
    // whose normals are computed after the winding reversal gets them pointing
    // inward, which is the exact defect this whole function exists to avoid.
    geo.normalize_attributes();
    geo_apply(geo, m4_scale(-1.0, 1.0, 1.0));
    if geo.index.is_empty() {
        // A non-indexed geometry is a triangle soup; the identity index is what
        // `to_mesh_data` would synthesise for it anyway, and having it here lets
        // the swap below be the same three lines for both shapes.
        geo.index = (0..geo.vert_count() as u32).collect();
    }
    for triangle in geo.index.chunks_exact_mut(3) {
        triangle.swap(0, 2);
    }
}

/// The spawned hand nodes, one per [`HandPart`], held so [`drive_hands`] can move
/// them every frame.
pub struct HandNodes {
    parts: Vec<SpawnedHandPart>,
}

struct SpawnedHandPart {
    side: HandSide,
    frame: usize,
    mirrored: bool,
    entity: Entity,
}

impl HandNodes {
    /// How many nodes the hands hold. See [`HandGeometry::draw_count`].
    pub fn len(&self) -> usize {
        self.parts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }
}

/// Upload both arms' geometry and spawn one node per part.
///
/// The nodes are spawned at the identity and are moved by the first
/// [`drive_hands`] of the first frame — the same posture `install_rifle` takes
/// with its seat transform.
pub fn install_hands(
    running: &mut RunningApp,
    geometry: &HandGeometry,
    materials: &WeaponMaterials,
) -> HandNodes {
    let parts = geometry
        .parts
        .iter()
        .map(|part| {
            let mesh = running
                .add_mesh_data(to_mesh_data(&part.geo))
                .expect("an arm-rig group is valid renderable geometry");
            let material = material_for(running, materials, part.key);
            let entity = running.spawn(Spawn::new(Transform::IDENTITY, mesh, material));
            SpawnedHandPart {
                side: part.side,
                frame: part.frame,
                mirrored: part.mirrored,
                entity,
            }
        })
        .collect();
    HandNodes { parts }
}

/// Move every hand node onto this frame's solved pose.
///
/// `rig` is the **world** transform of the viewmodel rig — the value
/// `scene::app::drive_viewmodel` already composes for the rifle (the camera's own
/// transform composed with `Viewmodel::rig_pose`). The arms' node arena is
/// expressed in exactly that space: `Viewmodel::solve_hands` rebases each
/// shoulder out of camera space into rig space before solving, so an arm's root
/// IS the rig.
///
/// The arms are taken `&mut` because refreshing the world matrices is
/// [`Arm::update_world_matrix`]'s job and it writes them into the arena. That is
/// the ported walk (`Object3D.updateMatrixWorld(true)`), not a second one.
pub fn drive_hands(
    app: &mut RunningApp,
    nodes: &HandNodes,
    left: &mut Arm,
    right: &mut Arm,
    rig: Transform,
) {
    let (left_root, right_root) = (left.root, right.root);
    left.update_world_matrix(left_root, false, true);
    right.update_world_matrix(right_root, false, true);
    for part in &nodes.parts {
        let arm: &Arm = match part.side {
            HandSide::Left => left,
            HandSide::Right => right,
        };
        let world = arm.nodes[part.frame].matrix_world;
        app.set(part.entity, rig_to_world(world, part.mirrored, rig));
    }
}

/// One rig-space world matrix, composed under the rig's own world transform and
/// reduced to the `(translation, rotation, scale)` triple a [`Transform`] holds.
///
/// The reduction is exact rather than a general decomposition, and it is exact
/// for a specific reason: every linear part in this arena is a rotation, possibly
/// times the one fixed reflection. Dividing that reflection back out (the first
/// column negated, since the geometry already carries it) leaves an orthonormal
/// basis, and [`Q::from_basis`] is `Quaternion.setFromRotationMatrix` — the
/// source's own conversion, not a re-derivation.
fn rig_to_world(world: M4, mirrored: bool, rig: Transform) -> Transform {
    let e = world.e;
    let column_x = V3::new(e[0], e[1], e[2]);
    let basis_x = if mirrored {
        column_x.mul_scalar(-1.0)
    } else {
        column_x
    };
    let rotation = Q::from_basis(
        basis_x.normalize_or_zero(),
        V3::new(e[4], e[5], e[6]).normalize_or_zero(),
        V3::new(e[8], e[9], e[10]).normalize_or_zero(),
    );
    let local = Vec3::new(e[12] as f32, e[13] as f32, e[14] as f32);
    let local_rotation = Quat::new(
        rotation.x as f32,
        rotation.y as f32,
        rotation.z as f32,
        rotation.w as f32,
    );
    Transform::new(
        rig.translation.add(rig.rotation.rotate(local)),
        rig.rotation.multiply(local_rotation),
        Vec3::new(1.0, 1.0, 1.0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::weapons::hands::ArmOpts;

    fn look() -> WeaponLook {
        // `Quality::Low` scales every bake size down through `size_of`, and the
        // cap does the rest: this resolves the whole table and bakes it in about
        // the time one 64-texel street surface takes.
        WeaponLook::new(Quality::Low)
    }

    #[test]
    fn every_weapon_material_key_resolves_and_the_custom_five_come_first() {
        let look = look();
        let keys: Vec<&str> = look.keys().iter().map(|k| k.key).collect();
        assert_eq!(&keys[..5], &CUSTOM_KEYS[..]);
        assert_eq!(keys.len(), CUSTOM_KEYS.len() + material_keys().len());
        // Every `WEAPON_MATERIALS` entry got a runtime-material surface; every
        // custom key deliberately did not.
        let (surfaced, plain): (Vec<_>, Vec<_>) =
            look.keys().iter().partition(|k| k.surface.is_some());
        assert_eq!(surfaced.len(), material_keys().len());
        assert_eq!(plain.len(), CUSTOM_KEYS.len());
    }

    #[test]
    fn a_library_key_carries_its_own_baked_albedo_and_the_tables_collapse() {
        let look = look();
        let alu = look
            .keys()
            .iter()
            .find(|k| k.key == "alu")
            .expect("the table's first entry");
        assert!(alu.bake_key.is_some(), "alu names a texture set");
        assert_eq!(alu.base_color, Color::WHITE, "the tint is in the surface");
        // Fifteen entries over three libraries, with per-entry bake overrides:
        // more than three bakes and no more than one per entry.
        assert!(look.bakes.len() > 3);
        assert!(look.bakes.len() <= material_keys().len());
    }

    /// **One pipeline, one parameter region per material.**
    ///
    /// This asserted `surfaces().len() == 1` on the reasoning that a runtime
    /// material's parameters are excluded from its digest, so all fifteen share
    /// one program. The premise holds; using the digest as the REGION key too
    /// did not, and its own closing line named the symptom it would then miss —
    /// "a fallback-shaded gun". Fifteen weapon materials really were shading as
    /// one.
    #[test]
    fn the_surfaces_share_one_program_and_keep_a_region_each() {
        let look = look();
        let surfaces = look.surfaces();
        assert!(!surfaces.is_empty());
        let digests: std::collections::BTreeSet<u64> =
            surfaces.iter().map(|s| s.digest().raw()).collect();
        assert_eq!(digests.len(), 1, "every runtime material is ONE program");
        let regions: std::collections::BTreeSet<u64> =
            surfaces.iter().map(|s| s.param_key().raw()).collect();
        assert_eq!(
            regions.len(),
            surfaces.len(),
            "two weapon materials share a parameter region — one of them is \
             shading as the other"
        );
        assert!(regions.len() > 3, "the table resolved to {} regions", regions.len());
    }

    #[test]
    fn an_unlit_custom_key_becomes_a_black_albedo_carrying_an_emissive() {
        let look = look();
        // `lens_ring` is a `MeshBasicMaterial` — unlit and additive.
        let ring = look
            .keys()
            .iter()
            .find(|k| k.key == "lens_ring")
            .expect("a custom key");
        assert_eq!(ring.base_color, Color::BLACK);
        assert!(ring.emissive.is_some());
        // `optic_tube` is physical, so it stays a lit albedo.
        let tube = look
            .keys()
            .iter()
            .find(|k| k.key == "optic_tube")
            .expect("a custom key");
        assert!(tube.emissive.is_none());
        assert_ne!(tube.base_color, Color::BLACK);
    }

    #[test]
    fn the_tint_round_trips_through_the_hex_the_parameter_block_stores() {
        // Mid grey survives the encode to within a byte.
        let hex = linear_to_hex([0.2158605, 0.2158605, 0.2158605]);
        let back = hex_to_linear(hex);
        assert!((back[0] - 0.2158605).abs() < 0.004, "got {}", back[0]);
        // Black and white are exact.
        assert_eq!(linear_to_hex([0.0, 0.0, 0.0]), 0x00_0000);
        assert_eq!(linear_to_hex([1.0, 1.0, 1.0]), 0xff_ffff);
        // And a triple above one keeps its HUE, spending the loss on magnitude.
        // `brass` is (2.3, 1.58, 0.74) — an F0 rather than an albedo — and its
        // ratio 1 : 0.687 : 0.322 is the whole of what makes it read as gold.
        // The brightest channel still saturates, so nothing that clipped moves.
        let brass = hex_to_linear(linear_to_hex([2.3, 1.58, 0.74]));
        assert_eq!(linear_to_hex([2.3, 1.58, 0.74]) >> 16, 0xff);
        assert!(
            (brass[1] / brass[0] - 1.58 / 2.3).abs() < 0.005,
            "brass lost its green ratio: {brass:?}"
        );
        assert!(
            (brass[2] / brass[0] - 0.74 / 2.3).abs() < 0.005,
            "brass lost its blue ratio: {brass:?}"
        );
        // `copper` (2.25, 1.4, 1.09) is the case channel-wise clamping destroyed
        // outright: all three channels were above one, so it encoded to pure
        // white and the bullet jacket lost its colour entirely.
        let copper = hex_to_linear(linear_to_hex([2.25, 1.4, 1.09]));
        assert!(
            copper[2] < copper[1] && copper[1] < copper[0],
            "copper is not warm: {copper:?}"
        );
        assert!(
            (copper[1] / copper[0] - 1.4 / 2.25).abs() < 0.005,
            "copper lost its green ratio: {copper:?}"
        );
    }

    #[test]
    fn the_weapon_opts_carry_the_bake_override_and_never_the_tint() {
        let alu = crate::weapons::materials::weapon_material("alu").expect("the first entry");
        let opts = weapon_opts(alu);
        assert!(opts.get("tint").is_none(), "the tint rides the params");
        let bake = opts.get("bake").expect("alu overrides its bake");
        let json = bake.to_json().expect("an object stringifies");
        assert!(json.contains("\"seed\":601"), "got {json}");
        assert!(json.contains("\"relief\":0.005"), "got {json}");
        // `OptValue::as_str` is private to the facade, so the assertion reads
        // the JSON `stableKey` would hash — which is the observable value.
        assert_eq!(
            opts.get("uvMode").and_then(OptValue::to_json),
            Some("\"triplanar\"".to_string())
        );
    }

    /// Both arms come apart into fewer parts than they have meshes (the
    /// per-frame merge did something), every part names a real material key, and
    /// exactly one arm is mirrored.
    #[test]
    fn both_arms_come_apart_into_drawable_parts() {
        let left = Arm::new(-1.0, ArmOpts::default());
        let right = Arm::new(1.0, ArmOpts::default());
        let meshes = left.meshes.len() + right.meshes.len();
        let geometry = HandGeometry::from_arms(&left, &right);

        assert!(geometry.draw_count() > 0);
        assert!(
            geometry.draw_count() < meshes,
            "{} parts from {meshes} meshes — the merge did nothing",
            geometry.draw_count()
        );
        let keys: Vec<&str> = CUSTOM_KEYS.into_iter().chain(material_keys()).collect();
        assert!(geometry.parts.iter().all(|p| keys.contains(&p.key)));

        // `handInner.scale.x` is -1 on the RIGHT arm (`side >= 0`), so the
        // mirrored parts are all on that side and there is at least one.
        let mirrored: Vec<HandSide> = geometry
            .parts
            .iter()
            .filter(|p| p.mirrored)
            .map(|p| p.side)
            .collect();
        assert!(!mirrored.is_empty());
        assert!(mirrored.iter().all(|s| *s == HandSide::Right));
    }

    /// The mirror bake leaves a right-handed geometry: reflecting positions and
    /// reversing the winding keeps every triangle's normal pointing the same way
    /// relative to its face.
    #[test]
    fn mirroring_reflects_the_geometry_and_reverses_its_winding() {
        let mut geo = Geo {
            pos: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            normal: vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0],
            uv: vec![0.0; 6],
            index: vec![0, 1, 2],
        };
        mirror_x(&mut geo);
        assert_eq!(geo.index, vec![2, 1, 0]);
        assert_eq!(geo.pos[3], -1.0, "x is reflected");
        assert_eq!(geo.normal[2], 1.0, "the +z normal is untouched by a YZ flip");
    }

    /// A rig-space frame composes under the rig transform, and a mirrored frame
    /// resolves to a pure rotation rather than a reflection.
    #[test]
    fn a_frame_composes_under_the_rig_and_a_mirror_divides_back_out() {
        let rig = Transform::new(
            Vec3::new(1.0, 2.0, 3.0),
            Quat::IDENTITY,
            Vec3::new(1.0, 1.0, 1.0),
        );
        let plain = rig_to_world(
            M4::compose(V3::new(0.1, 0.2, 0.3), Q::IDENTITY, V3::new(1.0, 1.0, 1.0)),
            false,
            rig,
        );
        assert!((plain.translation.x - 1.1).abs() < 1e-6);
        assert!((plain.translation.z - 3.3).abs() < 1e-6);
        assert_eq!(plain.scale, Vec3::new(1.0, 1.0, 1.0));

        // The reflection is divided out, so the reported rotation is the identity
        // and NOT a negative-determinant basis the renderer would light inside
        // out.
        let mirrored = rig_to_world(
            M4::compose(V3::ZERO, Q::IDENTITY, V3::new(-1.0, 1.0, 1.0)),
            true,
            Transform::IDENTITY,
        );
        assert!((mirrored.rotation.w.abs() - 1.0).abs() < 1e-6);
    }
}
