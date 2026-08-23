//! The world subsystem facade — level orchestration.
//!
//! Ported from Claude-of-Duty `src/world/index.js:1-446` — the whole file
//! except the light ballast (see "Deliberately deleted" below).
//!
//! **WORLD** — level geometry, the modular building kit, props, set dressing
//! and static collision. A ~120 x 120 m Middle-Eastern market street: one main
//! street with a plaza, flanking alleys, twenty buildings (three of them
//! enterable and furnished across multiple floors), an arched gate closing the
//! vista, and several thousand props. Nothing is loaded from disk — every
//! vertex is generated here.
//!
//! ```text
//! PUBLIC API   const world = ctx.get('world')
//! BUILD      batches (statics + instanced) collision lights stats buildings
//! QUERY      bounds spawn_points spawn(i) ground_height(x,z) is_open(x,z)
//!            level_to_world world_to_level
//! RUNTIME    update(dt, sun_altitude) — the street lamps and the interior
//!            bulbs, driven off the sky's real solar altitude
//! ```
//!
//! This file is *only* orchestration: it owns the pass order, the level→world
//! transform, the spawn table, the punctual lights and the dusk curve. Every
//! vertex it is responsible for is produced by a module that already has its
//! own golden ([`crate::world::ground`], [`crate::world::buildings`],
//! [`crate::world::props`], [`crate::world::dressing`], …).
//!
//! ## Draw order is the level's identity
//!
//! Every pass draws from **one** shared stream, in a fixed order:
//!
//! ```text
//! register_props -> register_dressing_props -> build_ground
//!   -> build_building x20 (+ collapse_roof, 2 draws, where flagged)
//!   -> build_gate -> build_perimeter -> dress_street -> dress_buildings
//!   -> scatter_debris
//! ```
//!
//! Prototypes come first because the level references them by id as it builds;
//! everything after that is a draw against the same `rng`, so a single added,
//! missing or reordered draw anywhere re-rolls the whole street.
//! `tests/world_system_port.rs` pins the stream's four state words and its
//! total draw count after `init`, which is the sharpest available proof that
//! this order is the source's order.
//!
//! ## One fork, not two
//!
//! The source takes exactly one fork: `this.rng = ctx.rng.fork()`
//! (`index.js:91`). `new Assembler({ materials, rng, render })` then stores
//! that same object as `this.rng` (`builder.js:44`) — and **never reads it**.
//! Rust will not let a content pass borrow `&mut Rng` while it also holds the
//! `&mut Assembler` that owns it, so [`Assembler::new`] takes an `Rng` of its
//! own; this facade hands it a fresh `Rng::new(0)` rather than a second fork,
//! because a second `fork()` would draw a `u32` from the root stream that the
//! source never draws and shift every subsystem initialised after `world`.
//! The Assembler's placement jitter does not come from that field either — it
//! comes from `jitter_rig()`'s own fixed-seed stream (`0x9e3779b1`,
//! `dressing.js:325-327`), for exactly the reason the source's comment there
//! gives.
//!
//! (`crate::scene::level::build_level` — an earlier, partial transcription of
//! this same `init` written before `dressing.js` was ported — takes two forks
//! and says so. This facade is the complete one.)
//!
//! ## Deliberately deleted: the light ballast and the pre-warm
//!
//! `_addBallast` / `_stabiliseLightCount` (`index.js:229-308`) park
//! `LIGHT_SLOTS + 4` zero-intensity **black** point lights under the map and
//! top the visible count up every `lateUpdate`, so `numPointLights` — a Three
//! *shader-permutation cache key* — never changes and the renderer never
//! recompiles every lit material mid-walk. `prewarmMaterials` / `_compile`
//! (`index.js:356-406`) is the same problem from the other end: a boot-time
//! `renderer.compileAsync` sweep over the forward pass plus the CSM and
//! gbuffer override materials.
//!
//! Both are pure Three workarounds with **no analogue here** —
//! `docs/work-manifests/shmup-port/05-port-status.md` lists them under "Not
//! being ported, deliberately", and Axiom solves the second one structurally
//! (surface programs compile at a preparation barrier). The functions dropped,
//! in full, are: `_addBallast`, `_stabiliseLightCount`, `prewarmMaterials`,
//! `_compile`, and the `LIGHT_SLOTS` constant they share. `lateUpdate`
//! (`index.js:333-335`) called nothing else, so it is gone too, and with it
//! the `_ballast` / `_pointLights` / `_pointLightsFrame` / `_lightTarget` /
//! `_lightRanges` / `_camPos` / `_collectPointLight` / `_render` state. Nothing
//! else in this file reads any of them.
//!
//! ## Other things this port does not carry, and why
//!
//! * **`A.updateLod(ctx.camera)`** (`index.js:313`). Distance LOD needs a live
//!   camera and a per-frame bounding-sphere test; `assembler.rs`'s module doc
//!   already records that it carries each prototype's `max_dist` as data and
//!   stops there. [`InstancedBatch::max_dist`] is that data.
//! * **`A.mat('lamp_lens')`** (`index.js:192`) and the
//!   `lampLens.emissiveIntensity` write (`index.js:323`). `assembler.rs`
//!   deliberately does not port material resolution — nothing on the Rust side
//!   owns a live material yet. The value the source would write is computed
//!   and exposed as [`WorldSystem::lamp_lens_emissive`], so a renderer can
//!   apply it the moment there is a material to apply it to.
//! * **`materials.setGroundLevel(0)`** (`index.js:103`). Same reason: the
//!   weathering shader that reads it has no runtime here. The constant is
//!   [`GROUND_LEVEL`].
//! * **`this.root` / `ctx.scene.add`** — a `THREE.Group`. [`WorldSystem`]
//!   returns the batches as data; parenting them is a renderer's job.
//! * **`physics.addStatic` / `rebuildStatic`** — `src/physics/index.js` is a
//!   different slice, and `assembler.rs` records the same gap. The collision
//!   meshes come out of [`Assembler::finalize`] as data; feeding them to a
//!   [`StaticWorld`][crate::physics::bvh::StaticWorld] is the caller's step
//!   (`crate::scene::level` already does exactly that).
//! * **`console.info`** (`index.js:156-160`) — a build banner whose facts are
//!   [`WorldSystem::stats`].

use crate::rng::Rng;
use crate::weapons::rig_math::{M4, Q, V3};
use crate::world::assembler::{Assembler, CollisionMesh, InstancedDraw, Stats, StaticMesh};
use crate::world::buildings::{build_building, collapse_roof, BuildingInfo, CollapseHole};
use crate::world::dressing::{
    build_gate, build_perimeter, dress_buildings, dress_street, ground_y, is_open,
    register_dressing_props, scatter_debris,
};
use crate::world::ground::build_ground;
use crate::world::layout::BUILDINGS;
use crate::world::props::register_props;
use crate::engine::Ctx;
use crate::registry::{Phase, Subsystem};
use crate::world::props::RegisteredProto;

/// LEVEL -> WORLD (`index.js:60-62`). The street is authored down -Z; this yaw
/// puts it on the axis the canonical hero/sunset cameras look along, with the
/// market in the near third of the frame and the gate closing the far end.
///
/// **`f64`, as the source authors them.** `Assembler::set_transform` takes
/// `f32` (the whole geometry pass computes in `f32`), so the narrowing happens
/// at that one call site and is visible there. Keeping the constants
/// themselves at `f32` would push the narrowing into `levelToWorld` /
/// `worldToLevel` / `bounds`, which the source evaluates at full double
/// precision — `0.9` narrowed to `f32` and widened back is
/// `0.899999976158142`, a 2.4e-8 error on every query.
pub const LEVEL_YAW: f64 = 0.5877;
pub const LEVEL_TX: f64 = 0.9;
pub const LEVEL_TZ: f64 = 1.34;

/// `materials.setGroundLevel(0)` (`index.js:103`) — weathering in the shared
/// materials keys off the ground plane. See the module doc.
pub const GROUND_LEVEL: f64 = 0.0;

/// Spawn points in LEVEL space: `[x, z, yaw, tag]` (`index.js:74-83`).
pub const SPAWNS: [(f64, f64, f64, &str); 8] = [
    (0.4, 22.5, std::f64::consts::PI, "north street"),
    (-2.4, 30.0, std::f64::consts::PI, "north plaza"),
    (3.6, 5.0, std::f64::consts::PI, "market"),
    (-3.4, -12.0, 0.0, "mid street"),
    (2.6, -32.0, 0.0, "south street"),
    (-1.0, -39.0, 0.0, "gate"),
    (10.5, 4.6, -std::f64::consts::FRAC_PI_2, "east alley"),
    (-9.0, -10.2, std::f64::consts::FRAC_PI_2, "west alley"),
];

/// The playable bounds, in LEVEL space, before `applyMatrix4(A.xform)`
/// (`index.js:149-152`).
pub const BOUNDS_MIN: [f64; 3] = [-62.0, -2.0, -62.0];
pub const BOUNDS_MAX: [f64; 3] = [62.0, 26.0, 62.0];

/// `A.interiorLights.slice(0, 20)` (`index.js:173`) — the cap on how many bare
/// bulbs the world lights.
pub const MAX_INTERIOR_BULBS: usize = 20;

/// One spawn point, already in WORLD space (`index.js:144-148`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpawnPoint {
    pub position: V3,
    pub yaw: f64,
    pub tag: &'static str,
}

/// An axis-aligned box in WORLD space — `THREE.Box3`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    pub min: V3,
    pub max: V3,
}

/// A punctual light the world owns: a bare interior bulb or a street lamp
/// (`index.js:178-190`). `THREE.PointLight(color, intensity, distance, decay)`
/// as plain values, plus the `{ range, priority }` the source hands
/// `A.light`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorldLight {
    /// Packed `0xRRGGBB`, as the source authors it.
    pub color: u32,
    pub intensity: f64,
    pub distance: f64,
    pub decay: f64,
    pub position: V3,
    /// `l.castShadow = false` for every one of them.
    pub cast_shadow: bool,
    /// `A.light(l, { range, priority })`.
    pub range: f64,
    pub priority: u32,
}

/// One merged static batch out of [`Assembler::finalize`].
pub struct StaticBatch {
    pub key: String,
    pub surface: crate::world::palette::Surface,
    pub geo: crate::world::geo::WorldGeo,
}

/// One instanced-prototype batch out of [`Assembler::finalize`], bucketed by
/// 64 m chunk. The matrices, in order, **are** the placement list.
pub struct InstancedBatch {
    pub proto_id: String,
    pub key: String,
    pub surface: crate::world::palette::Surface,
    pub cast_shadow: bool,
    pub receive_shadow: bool,
    pub no_prepass: bool,
    /// `im.userData.owLodDist` — see the module doc on `updateLod`.
    pub max_dist: f32,
    pub matrices: Vec<axiom_math::Mat4>,
    pub masks: Vec<[f32; 3]>,
}

/// `class WorldSystem` (`index.js:85-443`).
///
/// `static id = 'world'`, `static deps = ['materials', 'physics']`. It is not
/// wired as a [`Subsystem`][crate::registry::Subsystem]: `init` needs no
/// `ctx`, `update` needs the sky's solar altitude (a value, not a registry
/// lookup), and `lateUpdate` is gone with the ballast — so the whole facade is
/// a plain value the composition root builds and steps, which is also what
/// makes it testable without a registry.
pub struct WorldSystem {
    /// `this.buildings` (`index.js:128`).
    pub buildings: Vec<BuildingInfo>,
    pub statics: Vec<StaticBatch>,
    pub instanced: Vec<InstancedBatch>,
    pub collision: Vec<CollisionMesh>,
    /// `this.bulbs` (`index.js:170`) — bare bulbs inside the enterable
    /// buildings, in `A.interiorLights` order, capped at
    /// [`MAX_INTERIOR_BULBS`].
    pub bulbs: Vec<WorldLight>,
    /// `this.lamps` (`index.js:171`) — street lamps, in `A.lampAnchors` order.
    pub lamps: Vec<WorldLight>,
    /// `A.lights` — every light registered with the Assembler, in
    /// registration order (bulbs then lamps).
    pub registered_lights: Vec<V3>,
    /// `this.spawnPoints` (`index.js:144-148`).
    pub spawn_points: Vec<SpawnPoint>,
    /// `this.bounds` (`index.js:149-152`).
    pub bounds: Bounds,
    /// `this.stats` (`index.js:153`).
    pub stats: Stats,
    /// The world's own stream, after every pass has drawn from it. The source
    /// keeps `this.rng` alive for the same reason (nothing reads it after
    /// `init`, but it is the level's identity).
    pub rng: Rng,

    /// `this.A.xform`, in `f64`. See [`WorldSystem::world_to_level`].
    xform: M4,
    /// `this._inv` (`index.js:143`).
    inv: M4,
    /// `this._lampMix` (`index.js:193`), initialised to `-1` so the first
    /// `update` always writes.
    lamp_mix: f64,
    /// The value `lampLens.emissiveIntensity` would be given
    /// (`index.js:323`). See the module doc.
    lamp_lens_emissive: f64,

    /// `A.interiorLights` — where the interiors pass asked for a bare bulb,
    /// in **LEVEL** space and uncapped. [`WorldSystem::bulbs`] is the first
    /// [`MAX_INTERIOR_BULBS`] of these turned into lights, in **WORLD** space
    /// (see [`add_lights`]); both are carried because they are two different
    /// facts and only one of them is what a renderer wants.
    pub interior_anchors: Vec<V3>,
    /// `A.lampAnchors`, in LEVEL space.
    pub lamp_anchors: Vec<V3>,

    /// Both prototype tables, in registration order: `register_props` then
    /// `register_dressing_props`.
    ///
    /// Kept because [`InstancedBatch`] names its prototype by id and does NOT
    /// carry the geometry — `finalize` drains the Assembler's table into the
    /// batches, so these summaries are the only place a placed prototype's mesh
    /// is still reachable. A caller turning batches into renderable meshes
    /// cannot do it without them.
    pub prototypes: Vec<RegisteredProto>,
}

impl WorldSystem {
    /// `init(ctx)` (`index.js:89-161`), minus the scene graph, the material
    /// resolution, the physics bridge and the banner. `root` is the engine's
    /// root stream; this forks its own from it exactly once.
    pub fn init(root: &mut Rng) -> WorldSystem {
        WorldSystem::init_observed(root, &mut |_, _| {})
    }

    /// [`WorldSystem::init`] with a per-pass observer: `checkpoint(name,
    /// state)` is called with the shared stream's four state words after each
    /// pass, in pass order.
    ///
    /// Not in the source — but the pass order *is* the level's identity (see
    /// the module doc), and the only way to prove it against the original is
    /// to compare the stream at each boundary. Putting the hook on the real
    /// `init` rather than re-listing the passes in a test is deliberate: a
    /// test that spelled the order out itself could not catch the order being
    /// wrong here.
    pub fn init_observed(
        root: &mut Rng,
        checkpoint: &mut dyn FnMut(&str, [u32; 4]),
    ) -> WorldSystem {
        let mut rng = root.fork();

        // `new Assembler({ materials, rng, render })`. The `rng` argument is
        // stored and never read — see the module doc's "One fork, not two".
        let mut a = Assembler::new(Rng::new(0));
        // The one narrowing: the Assembler bakes the transform into `f32`
        // vertices (see the constants' doc).
        a.set_transform(LEVEL_YAW as f32, LEVEL_TX as f32, LEVEL_TZ as f32);
        checkpoint("start", rng.state());

        // 1. prototypes first: the level references them by id while it builds
        let mut prototypes = register_props(&mut a, &mut rng);
        checkpoint("registerProps", rng.state());
        prototypes.extend(register_dressing_props(&mut a, &mut rng));
        checkpoint("registerDressingProps", rng.state());

        // 2. ground, then the shells, then what people put in and on them
        build_ground(&mut a, &mut rng);
        checkpoint("buildGround", rng.state());

        let mut infos: Vec<BuildingInfo> = Vec::new();
        for spec in BUILDINGS {
            let info = build_building(&mut a, &mut rng, spec);
            if spec.collapse {
                // Two draws, in this order, and only for a flagged spec: the
                // hole position is chosen by the CALLER, not by `buildings.js`
                // (`index.js:121-126`).
                let hx = spec.x as f32 + rng.range(-2.0, 2.0) as f32;
                let hz = spec.z as f32 + rng.range(-2.0, 2.0) as f32;
                collapse_roof(&mut a, &mut rng, spec, &info, CollapseHole { x: hx, z: hz });
            }
            infos.push(info);
            checkpoint(&format!("building:{}", spec.id), rng.state());
        }

        build_gate(&mut a, &mut rng);
        checkpoint("buildGate", rng.state());
        build_perimeter(&mut a, &mut rng);
        checkpoint("buildPerimeter", rng.state());
        dress_street(&mut a, &mut rng);
        checkpoint("dressStreet", rng.state());
        dress_buildings(&mut a, &mut rng, &infos);
        checkpoint("dressBuildings", rng.state());
        scatter_debris(&mut a, &mut rng);
        checkpoint("scatterDebris", rng.state());

        // `this._addLights(A)` — before `finalize`, because `A.light` registers
        // into the list `finalize` walks.
        let interior_anchors: Vec<V3> = a
            .interior_lights
            .iter()
            .map(|p| V3::new(f64::from(p.x), f64::from(p.y), f64::from(p.z)))
            .collect();
        let lamp_anchors: Vec<V3> = a
            .lamp_anchors
            .iter()
            .map(|p| V3::new(f64::from(p.x), f64::from(p.y), f64::from(p.z)))
            .collect();
        let (bulbs, lamps) = add_lights(&mut a);

        let result = a.finalize();
        a.release_cache();

        // -------------------------------------------------------- queries --
        let xform = level_xform();
        let inv = xform.invert();

        let spawn_points = SPAWNS
            .iter()
            .map(|(x, z, yaw, tag)| {
                // `A.toWorld(x, 0, z)` — the Assembler's own `f32` transform,
                // which is what the source calls here.
                let p = a.to_world(*x as f32, 0.0, *z as f32);
                SpawnPoint {
                    position: V3::new(f64::from(p.x), f64::from(p.y), f64::from(p.z)),
                    yaw: yaw + LEVEL_YAW,
                    tag,
                }
            })
            .collect();

        let bounds = transform_box(BOUNDS_MIN, BOUNDS_MAX, xform);

        WorldSystem {
            buildings: infos,
            statics: result
                .statics
                .into_iter()
                .map(|StaticMesh { key, surface, geo }| StaticBatch { key, surface, geo })
                .collect(),
            instanced: result
                .instanced
                .into_iter()
                .map(|d: InstancedDraw| InstancedBatch {
                    proto_id: d.proto_id,
                    key: d.key,
                    surface: d.surface,
                    cast_shadow: d.cast_shadow,
                    receive_shadow: d.receive_shadow,
                    no_prepass: d.no_prepass,
                    max_dist: d.max_dist,
                    matrices: d.matrices,
                    masks: d.masks,
                })
                .collect(),
            collision: result.collision,
            bulbs,
            lamps,
            registered_lights: result
                .lights
                .into_iter()
                .map(|l| V3::new(f64::from(l.position.x), f64::from(l.position.y), f64::from(l.position.z)))
                .collect(),
            spawn_points,
            bounds,
            stats: result.stats,
            rng,
            xform,
            inv,
            lamp_mix: -1.0,
            lamp_lens_emissive: 0.0,
            interior_anchors,
            lamp_anchors,
            prototypes,
        }
    }

    /* ================================================================ */
    /* runtime (`index.js:311-331`)                                     */
    /* ================================================================ */

    /// `update(dt, ctx)` (`index.js:311-331`).
    ///
    /// `sun_altitude` is `ctx.peek('sky')?.sunAltitude ?? 0.6` — the caller
    /// resolves the optional subsystem, so pass `None` where the source would
    /// have found no `sky`. `dt` is not read by the source and is not taken.
    ///
    /// Street lamps come on as the sun goes down, driven by the sky's real
    /// solar altitude rather than a timer, so it is right at any time of day.
    pub fn update(&mut self, sun_altitude: Option<f64>) {
        // `this.A?.updateLod(ctx.camera)` — not ported; see the module doc.
        let alt = sun_altitude.unwrap_or(0.6);
        let mix = 1.0 - ((alt + 0.05) / 0.16).clamp(0.0, 1.0);
        if (mix - self.lamp_mix).abs() > 0.01 {
            self.lamp_mix = mix;
            for l in &mut self.lamps {
                l.intensity = 14.0 * mix;
            }
            self.lamp_lens_emissive = 9.0 * mix;
            // Bulbs stay on around the clock — but a 60 W bulb is NOT
            // competitive with daylight, and running it at night strength at
            // noon is what made every interior read as pure tungsten (B-R -93)
            // and sit level with the sunlit street instead of 1.5-2.5 stops
            // under it. Gate the bulb on solar altitude: a weak practical by
            // day, the room's only light after dark.
            for l in &mut self.bulbs {
                l.intensity = 5.0 + 17.0 * mix;
            }
        }
    }

    /// `this._lampMix` (`index.js:193`).
    pub fn lamp_mix(&self) -> f64 {
        self.lamp_mix
    }

    /// The value the source writes to `lampLens.emissiveIntensity`
    /// (`index.js:323`). See the module doc.
    pub fn lamp_lens_emissive(&self) -> f64 {
        self.lamp_lens_emissive
    }

    /* ================================================================ */
    /* queries (`index.js:409-432`)                                     */
    /* ================================================================ */

    /// `spawn(i = 0)` (`index.js:409-412`), with the source's wrap-around
    /// index arithmetic (`((i % n) + n) % n`), which is correct for a negative
    /// `i` where a bare `%` is not.
    pub fn spawn(&self, i: i64) -> SpawnPoint {
        let n = self.spawn_points.len() as i64;
        self.spawn_points[(((i % n) + n) % n) as usize]
    }

    /// `levelToWorld(x, y, z, out)` (`index.js:414-416`).
    pub fn level_to_world(&self, x: f64, y: f64, z: f64) -> V3 {
        V3::new(x, y, z).apply_matrix4(self.xform)
    }

    /// `worldToLevel(x, y, z, out)` (`index.js:418-420`).
    pub fn world_to_level(&self, x: f64, y: f64, z: f64) -> V3 {
        V3::new(x, y, z).apply_matrix4(self.inv)
    }

    /// `groundHeight(x, z)` (`index.js:423-426`) — the analytic floor height.
    /// Physics owns the exact answer; this is a hint.
    pub fn ground_height(&self, x: f64, z: f64) -> f64 {
        let p = self.world_to_level(x, 0.0, z);
        ground_y(p.x, p.z)
    }

    /// `isOpen(x, z, margin = 0.4)` (`index.js:429-432`) — true where a
    /// character can stand outdoors (street, pavement, alley).
    pub fn is_open(&self, x: f64, z: f64, margin: f64) -> bool {
        let p = self.world_to_level(x, 0.0, z);
        is_open(p.x, p.z, margin)
    }

    /// `isOpen`'s defaulted margin.
    pub const DEFAULT_MARGIN: f64 = 0.4;
}

/// `_addLights(A)` (`index.js:169-196`), minus `_addBallast` (see the module
/// doc) and `A.mat('lamp_lens')`.
///
/// Punctual lights the world owns: the bare bulbs inside the enterable
/// buildings — what makes an interior read as lived-in against cool skylight —
/// and the street lamps, which only draw power after dusk.
fn add_lights(a: &mut Assembler) -> (Vec<WorldLight>, Vec<WorldLight>) {
    let mut bulbs = Vec::new();
    let mut lamps = Vec::new();

    let interior: Vec<axiom_math::Vec3> = a
        .interior_lights
        .iter()
        .take(MAX_INTERIOR_BULBS)
        .copied()
        .collect();
    for b in interior {
        // A bare 60 W bulb in an unlit room: the only thing separating an
        // interior from a black hole, so it has to actually carry the room.
        // Intensity is re-driven every `update` off the solar altitude; this
        // is the daylight value so a frame captured before the first update is
        // right.
        //
        // `A.light` MUTATES the light it is handed —
        // `if (!this._identity) light.position.applyMatrix4(this.xform)`
        // (`builder.js:311`) — so `this.bulbs` ends up holding WORLD-space
        // positions even though `A.interiorLights` is authored in LEVEL space.
        // Storing the level-space anchor here instead puts every interior bulb
        // ~2 m from where the source puts it.
        let w = a.to_world(b.x, b.y, b.z);
        let position = V3::new(f64::from(w.x), f64::from(w.y), f64::from(w.z));
        a.light(b, ());
        bulbs.push(WorldLight {
            color: 0x00ff_c07a & 0x00ff_ffff,
            intensity: 5.0,
            distance: 13.0,
            decay: 2.0,
            position,
            cast_shadow: false,
            range: 13.0,
            priority: 2,
        });
    }

    let anchors: Vec<axiom_math::Vec3> = a.lamp_anchors.clone();
    for p in anchors {
        // `l.position.set(p.x, p.y - 0.12, p.z)` then `A.light(l, …)`, which
        // transforms it into world space in place — see the bulb loop above.
        let w = a.to_world(p.x, p.y - 0.12, p.z);
        let position = V3::new(f64::from(w.x), f64::from(w.y), f64::from(w.z));
        a.light(axiom_math::Vec3::new(p.x, p.y - 0.12, p.z), ());
        lamps.push(WorldLight {
            color: 0x00ff_b765 & 0x00ff_ffff,
            intensity: 0.0,
            distance: 22.0,
            decay: 2.0,
            position,
            cast_shadow: false,
            range: 22.0,
            priority: 3,
        });
    }

    (bulbs, lamps)
}

/// `A.xform` in `f64` — `trs(LEVEL_TX, 0, LEVEL_TZ, LEVEL_YAW, 1, 1, 1, 0, 0)`.
///
/// [`Assembler`] keeps its own copy in `f32` (`axiom_math::Mat4`) and does not
/// expose it, and `index.js` uses the matrix at JavaScript's `f64` for
/// `levelToWorld`/`worldToLevel`/`bounds`. A yaw-only rotation composes
/// identically under every Euler order, so this is `Matrix4.compose(
/// (tx, 0, tz), quaternionFromEuler(0, ry, 0), (1,1,1) )` exactly.
fn level_xform() -> M4 {
    M4::compose(
        V3::new(LEVEL_TX, 0.0, LEVEL_TZ),
        Q::from_euler_xyz(0.0, LEVEL_YAW, 0.0),
        V3::new(1.0, 1.0, 1.0),
    )
}

/// `Box3.applyMatrix4(m)` — transform all eight corners and re-bound, which is
/// what `three` does and is *not* the same as transforming min and max.
fn transform_box(min: [f64; 3], max: [f64; 3], m: M4) -> Bounds {
    // `Box3.js`'s own corner order, kept so the folds see the same sequence.
    let corners = [
        V3::new(min[0], min[1], min[2]),
        V3::new(min[0], min[1], max[2]),
        V3::new(min[0], max[1], min[2]),
        V3::new(min[0], max[1], max[2]),
        V3::new(max[0], min[1], min[2]),
        V3::new(max[0], min[1], max[2]),
        V3::new(max[0], max[1], min[2]),
        V3::new(max[0], max[1], max[2]),
    ];
    let mut lo = V3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY);
    let mut hi = V3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for c in corners {
        let p = c.apply_matrix4(m);
        lo = V3::new(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z));
        hi = V3::new(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z));
    }
    Bounds { min: lo, max: hi }
}

/// The registry face of [`WorldSystem`] — `world/index.js:86`.
///
/// **Why a second type and not `impl Subsystem for WorldSystem`.**
///
/// [`WorldSystem::init`] is a *constructor*: it takes `&mut Rng`, forks, builds
/// the level and returns a fully-formed system. [`Subsystem::init`] is a
/// *phase*: it takes `&mut self` and a [`Ctx`], and the registry calls it in
/// topological order. Those are not the same shape, and the difference is not
/// cosmetic — it decides **who owns the fork**.
///
/// The fork has to happen inside `Subsystem::init`, because the registry is
/// what sequences init, and the sequence is the level (every subsystem forks
/// the root once; `crate::registry::Registry` breaks sort ties on insertion
/// order). Constructing the world *before* registering it would put the fork in
/// registration order and let the registry re-order init around it, which is
/// the silent-reshuffle failure
/// `crate::scene::game::tests::the_root_stream_is_consumed_in_the_registrys_order`
/// exists to catch.
///
/// So this holds the built world in an `Option` and fills it from `ctx.rng`
/// when the registry says so. The `Option` is not defensive: it is the honest
/// representation of a system that exists in the graph before it has been
/// initialised, which is exactly the state the registry resolves over.
pub struct WorldSubsystem {
    built: Option<WorldSystem>,
    /// The last solar altitude `update` was handed, so the dusk ramp can be
    /// re-driven without re-reading a subsystem this one may not be able to see.
    sun_altitude: Option<f64>,
}

impl Default for WorldSubsystem {
    fn default() -> Self {
        WorldSubsystem::new()
    }
}

impl WorldSubsystem {
    /// An unbuilt world. Cheap: no fork, no geometry, nothing drawn.
    pub const fn new() -> Self {
        WorldSubsystem {
            built: None,
            sun_altitude: None,
        }
    }

    /// The built world, or `None` before the registry has run `init`.
    pub const fn get(&self) -> Option<&WorldSystem> {
        self.built.as_ref()
    }

    /// The built world, mutably.
    pub const fn get_mut(&mut self) -> Option<&mut WorldSystem> {
        self.built.as_mut()
    }

    /// Hand this system the solar altitude its dusk ramp reads.
    ///
    /// `update` takes it from here rather than from `ctx.get("sky")`, because
    /// the source's `world` does not declare `sky` in its `deps` — it peeks, and
    /// tolerates absence (`ctx.peek('sky')?.sunAltitude ?? 0.6`). Threading it
    /// keeps the declared graph honest rather than adding an edge the source
    /// does not have.
    pub const fn set_sun_altitude(&mut self, altitude: f64) {
        self.sun_altitude = Some(altitude);
    }
}

impl Subsystem for WorldSubsystem {
    fn id(&self) -> &'static str {
        "world"
    }

    /// `static deps = ['materials', 'physics']` (`world/index.js:87`).
    fn deps(&self) -> &'static [&'static str] {
        &["materials", "physics"]
    }

    fn phases(&self) -> &'static [Phase] {
        &[Phase::Update]
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    /// `init(ctx)` — the fork, taken here so the registry owns its place in the
    /// order.
    fn init(&mut self, ctx: &Ctx<'_>) -> Result<(), crate::error::CoreError> {
        let mut root = ctx.rng.borrow_mut();
        self.built = Some(WorldSystem::init(&mut root));
        Ok(())
    }

    /// `update(dt, ctx)` (`index.js:311-331`) — the street lamps and the
    /// interior bulbs against the sun's real altitude.
    fn update(&mut self, _dt: axiom_kernel::Seconds, _ctx: &Ctx<'_>) {
        let altitude = self.sun_altitude;
        self.built
            .as_mut()
            .map(|world| world.update(altitude))
            .unwrap_or_default();
    }
}

#[cfg(test)]
mod subsystem_tests {
    use super::*;

    /// The id and deps are what let `player` and `ai` resolve: both name
    /// `"world"`, and until this existed `Registry::resolve` failed on it.
    #[test]
    fn it_answers_to_the_id_player_and_ai_depend_on() {
        let world = WorldSubsystem::new();
        assert_eq!(world.id(), "world");
        assert_eq!(world.deps(), &["materials", "physics"]);
        assert!(world.get().is_none(), "an unregistered world is not built");
    }


    /// **The graph `scene::wiring::physics_player` said could not be built.**
    ///
    /// Its module doc records the exact failure: *"`PlayerSystem::deps()` is
    /// `["physics", "world", "render"]`, and neither `world` nor `render` is a
    /// ported `Subsystem`. The moment `player` is registered,
    /// `Registry::resolve` fails with "player" depends on unregistered
    /// subsystem "world"."* Faced with that, the port grew a second composition
    /// root in `scene::game::Game`, and every hand-inlined duplicate this port
    /// has found is downstream of it.
    ///
    /// Two missing files held it shut. This is the assertion that says they no
    /// longer do — and it asserts the ORDER, not just that resolution
    /// succeeded, because the order is the level.
    #[test]
    fn the_graph_resolves_now_that_world_and_render_exist() {
        let mut registry = crate::registry::Registry::new();
        registry
            .add(crate::render::system::RenderSystem::new())
            .expect("render is a root");
        registry
            .add(crate::materials::system::MaterialSystem::new(None))
            .expect("materials depends only on render");
        registry
            .add(crate::physics::system::PhysicsSystem::new(
                crate::physics::system::StaticRegistry::default(),
            ))
            .expect("physics is a root");
        registry
            .add(crate::world::system::WorldSubsystem::new())
            .expect("world depends on materials and physics, both registered");

        registry
            .add(crate::scene::wiring::look::SkySubsystem::new(
                crate::config::Quality::High,
                crate::scene::wiring::look::HOUR,
            ))
            .expect("sky depends on render and materials");
        registry
            .add(crate::scene::wiring::fx_audio::FxSubsystem::new(
                crate::config::Config::default(),
                None,
            ))
            .expect("fx depends on render and materials");
        let order: Vec<String> = registry
            .resolve()
            .expect("every declared dependency is registered")
            .iter()
            .map(|s| s.borrow().id().to_owned())
            .collect();
        assert_eq!(
            order,
            vec!["render", "materials", "physics", "world", "sky", "fx"],
            "the topological order is not the source's"
        );
    }
    /// **The fork is not taken until the registry says so.** A world that built
    /// itself at construction would draw from the root in registration order,
    /// and the registry would then re-order init around a fork already spent.
    #[test]
    fn construction_draws_nothing_from_the_root_stream() {
        let before = crate::rng::Rng::new(7).state();
        let rng = crate::rng::Rng::new(7);
        let _world = WorldSubsystem::new();
        assert_eq!(rng.state(), before, "constructing the world moved the stream");
    }
}
