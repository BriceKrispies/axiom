//! **The level, built once.** The composition step `world/index.js`'s
//! `WorldSystem.init` performs (`src/world/index.js:88-160`), reduced to what
//! this port actually has: the [`Assembler`] is set to the level transform, the
//! ported [`build_ground`] runs against it, and [`Assembler::finalize`]'s two
//! outputs are split to the two consumers that need them —
//!
//! * `statics` → engine meshes (one [`MeshData`] + one material per batch key),
//! * `collision` → the [`StaticWorld`] BVH the character controller and every
//!   world probe query.
//!
//! ## The call order, and why it is this order
//!
//! `WorldSystem.init` (`world/index.js:105-127`) runs, in order:
//! `registerProps` → `registerDressingProps` → `buildGround` →
//! `buildBuilding` per spec (+ `collapseRoof` where flagged) → `buildGate` →
//! `buildPerimeter` → `dressStreet` → `dressBuildings` → `scatterDebris` →
//! `_addLights`. Prototypes come first because the level references them by id
//! as it builds; everything after that is a draw against the same shared `rng`,
//! so the order is part of the level's identity, not a preference.
//!
//! This port runs the four of those that exist: `register_props`,
//! `build_ground`, `build_building` × 20, and `collapse_roof` for the one spec
//! flagged for it.
//!
//! ## What is still not here, and why
//!
//! * **`buildGate` / `buildPerimeter`** — neither name exists in
//!   `buildings.js` (`crate::world::buildings`'s module doc records the check),
//!   and no other file carrying them is ported. The street's far end is
//!   therefore open rather than closed by the arched gate.
//! * **`registerDressingProps`, `dressStreet`, `dressBuildings`,
//!   `scatterDebris`** — all of `src/world/dressing.js`, unported. In its place
//!   [`crate::scene::furniture`] puts a deliberately small, clearly-labelled set
//!   of prototypes on the authored `SET_PIECES` positions so the prop library is
//!   not dead. That file is a placeholder and says so; it is not a dressing
//!   pass.
//! * **`_addLights`** — unported, so `finalize`'s `lights` list is empty and the
//!   scene's only light is the sun ([`crate::scene::sky_look`]). The practicals
//!   (lamp lenses, window glow) are geometry with an emissive palette key and no
//!   light behind them.
//! * **`interiors.js`'s `furnishRoom`** — `build_interior` ports the partitions
//!   and stairs but not the furnishing (see `crate::world::buildings`).
//!
//! ## Instancing is the draw-call budget, and it is honoured
//!
//! `finalize` returns two kinds of renderable: `statics`, one merged mesh per
//! palette key, and `instanced`, one batch per prototype per 64 m chunk. Both
//! are carried into [`Level::batches`] as [`LevelBatch`]es —
//! [`LevelBatch::instances`] is what tells them apart. An instanced batch
//! uploads its prototype geometry **once** and spawns one node per instance
//! sharing that one mesh handle and one material handle, which is exactly the
//! shape the engine collapses into a single draw. Uploading a per-instance copy
//! would multiply the level's draw calls by ~150 and defeat the design.

use std::rc::Rc;

use axiom::prelude::{Color, MeshData, Ratio, Transform, Vec2, Vec3};
use axiom_math::{Mat4, Quat};

use crate::physics::bvh::StaticWorld;
use crate::physics::surfaces::layer;
use crate::rng::Rng;
use crate::world::geo::WorldGeo;
use crate::world::palette::{Palette, Surface};
use crate::world::buildings::BuildingInfo;
use crate::world::system::{WorldLight, WorldSystem};

/// `LEVEL_YAW` / `LEVEL_TX` / `LEVEL_TZ` (`world/index.js:60-62`) — LEVEL space
/// to WORLD space. The street is authored down -Z; this yaw puts it on the axis
/// the canonical hero camera looks along.
///
/// Re-exported from `world::system` rather than re-declared. This module used
/// to carry its own `f32` copies of the same three numbers, and they were not
/// the same numbers: `f64::from(0.5877_f32)` is `0.58770000934600830078125`,
/// so a spawn yaw computed here disagreed with one computed there in the
/// twelfth decimal place. The source has one value; so does this port.
pub use crate::world::system::{LEVEL_TX, LEVEL_TZ, LEVEL_YAW};

/// Spawn points in LEVEL space: `[x, z, yaw]` plus a tag. `SPAWNS`,
/// `world/index.js:74-83`. They live in `index.js` rather than `layout.js` in
/// the source, and `index.js` is not otherwise ported — this const is the whole
/// of what this file needs from it, carried with its citation.
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

/// One spawn point, already in WORLD space. `world.spawnPoints[i]`
/// (`world/index.js:144-148`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpawnPoint {
    pub position: [f64; 3],
    pub yaw: f64,
    pub tag: &'static str,
}

/// One renderable batch: the geometry of a single palette key, and the flat
/// albedo this port can give it.
///
/// **Flat, deliberately.** Each key's real appearance is a `materials/` shader
/// graph — 19 procedural surface generators over a triplanar-ish UV, none of
/// which has a GPU path in this port. The albedo here is the palette entry's
/// own authored `tint` (`palette.js`'s `tint:` field), which is the base colour
/// that shader modulates. So the level reads with the right *palette* and no
/// texture; see the notes file.
pub struct LevelBatch {
    pub key: String,
    pub surface: Surface,
    pub mesh: MeshData,
    pub albedo: Color,
    /// Where to put it. A `statics` batch is already baked into world space, so
    /// it carries exactly one identity transform. An `instanced` batch carries
    /// one transform per placed instance of its prototype, and every one of
    /// them shares this batch's single mesh and single material — which is what
    /// makes the whole batch one engine draw.
    pub instances: Vec<Transform>,
}

/// Everything one built level hands the rest of the game.
pub struct Level {
    /// The renderable batches, in `finalize` order.
    pub batches: Vec<LevelBatch>,
    /// The collision BVH, built and shared.
    pub world: Rc<StaticWorld>,
    /// `world.spawnPoints`, already in WORLD space.
    pub spawns: Vec<SpawnPoint>,
    /// The world's punctual lights — `this.bulbs` then `this.lamps`
    /// (`_addLights`, `world/index.js:169-196`), in WORLD space.
    pub practicals: Vec<WorldLight>,
    /// Every building's resolved spec and anchors.
    ///
    /// Carried because the minimap's vector bake reads `world.buildings`
    /// (`minimap.js:184-201`) through `ui::minimap::LayoutSource`, and this
    /// struct used to drop the whole `WorldSystem` the moment it had copied the
    /// geometry out — so nothing reachable from `Game` could answer that
    /// question and the minimap silently fell through to its procedural plate.
    ///
    /// Only the *anchors and specs* are kept, not the system: `BuildingInfo`
    /// carries no vertex data, so this is metadata, whereas retaining the whole
    /// `WorldSystem` would pin every merged batch's geometry for the run.
    pub buildings: Vec<BuildingInfo>,
    /// `A.stats` — reported so a caller can see the level is real, and see what
    /// it costs, without walking the geometry.
    pub static_tris: usize,
    pub instanced_tris: usize,
    pub instances: usize,
    /// `A.stats.drawCalls`: the Assembler's own count of merged static meshes
    /// plus instanced batches. The engine's own per-frame figure is
    /// `FrameOutcome::mesh_batches().len()`, which should agree with this plus
    /// the rifle's buckets.
    pub draw_calls: usize,
    pub collide_tris: usize,
}

impl Level {
    /// `world.spawn(i)` (`world/index.js:409-412`), with the source's
    /// wrap-around index arithmetic.
    pub fn spawn(&self, i: i64) -> SpawnPoint {
        let n = self.spawns.len() as i64;
        self.spawns[(((i % n) + n) % n) as usize]
    }
}

/// Build the level by running [`WorldSystem::init`] — the port of
/// `WorldSystem.init` (`world/index.js:88-161`) — and translating what it
/// produces into the renderable, collidable shape the rest of the game wants.
///
/// **This function used to re-implement that build inline**, and the copy had
/// drifted: it took two `root.fork()`s where the source takes one
/// (`world/index.js:91`), so every content pass ran off a different stream and
/// the level was not the level the source describes; and it never called
/// `_addLights`, so the world had no practicals at all. `WorldSystem` — a
/// complete, checkpointed port of the same eleven passes — was sitting unused
/// in `world/system.rs` the whole time, with **zero** references outside its
/// own file.
///
/// That is the fifth hand-inlined duplicate this port has produced, and it is
/// the reason the lights were missing: the faithful version ran them, and
/// nothing called the faithful version. Delegating is what keeps the pass
/// order, the stream discipline and the light registration in ONE place, so
/// the next pass added to the world cannot be missing from the level.
pub fn build_level(root: &mut Rng) -> Level {
    let world_system = WorldSystem::init(root);

    // `WorldSystem` states a spawn's position as a `V3`; this module's own
    // `SpawnPoint` states it as `[f64; 3]`. One value conversion at one seam,
    // rather than a second type threaded through every consumer.
    let spawns: Vec<SpawnPoint> = world_system
        .spawn_points
        .iter()
        .map(|s| SpawnPoint {
            position: [s.position.x, s.position.y, s.position.z],
            yaw: s.yaw,
            tag: s.tag,
        })
        .collect();

    // `this.bulbs` then `this.lamps` (`world/index.js:170-190`) — the world's
    // practicals, in the order `_addLights` registers them. Both lists are
    // carried whole; which of them a frame can actually afford is the
    // renderer's budget question, answered where the camera is known.
    let practicals: Vec<WorldLight> = world_system
        .bulbs
        .iter()
        .chain(world_system.lamps.iter())
        .copied()
        .collect();

    let mut world = StaticWorld::new();
    let mut collide_tris = 0usize;
    for mesh in &world_system.collision {
        let (tris, count) = triangle_soup(&mesh.geo);
        collide_tris += count;
        world.add_triangles(&tris, count, mesh.surface, layer::STATIC, "world");
    }
    world.build();

    let statics = world_system.statics.iter().map(|s| LevelBatch {
        key: s.key.clone(),
        surface: s.surface,
        mesh: to_mesh_data(&s.geo),
        albedo: key_albedo(&s.key),
        instances: vec![Transform::IDENTITY],
    });
    // One batch per *prototype*, not per prototype-per-chunk. `finalize`
    // splits a heavily-placed prototype into 64 m chunks so a renderer can
    // frustum- and distance-cull them separately; this port has no per-batch
    // distance cull, so keeping the split would cost one extra draw per chunk
    // and buy nothing. The instances are concatenated back in `finalize` order,
    // so the level is identical — only the batching is coarser.
    //
    // `b.masks` (the per-instance weathering triple `instanceColor` would
    // modulate the shader with) is dropped throughout: the engine's instance
    // data carries no per-instance colour, and the material it would tint is
    // the unported one. Every instance of a prototype weathers identically.
    let mut instanced: Vec<LevelBatch> = Vec::new();
    for b in &world_system.instanced {
        let placements = b.matrices.iter().map(transform_of);
        match instanced.iter_mut().find(|x| x.key == b.proto_id) {
            Some(existing) => existing.instances.extend(placements),
            None => {
                // BOTH prototype tables. `register_props` registers the
                // world's own props and `register_dressing_props` the dressing
                // pass's; looking in only the first panicked the moment the
                // dressing was actually called, because every gate, planter and
                // debris piece it places lives in the second.
                let geo = world_system
                    .prototypes
                    .iter()
                    .find(|p| p.id == b.proto_id)
                    .map(|p| &p.geo)
                    .expect("every placed prototype was registered by one of the two tables");
                instanced.push(LevelBatch {
                    // Keyed on the *prototype* id so the merge above is by
                    // prototype; the palette key drives the albedo instead.
                    key: b.proto_id.clone(),
                    surface: b.surface,
                    mesh: to_mesh_data(geo),
                    albedo: key_albedo(&b.key),
                    instances: placements.collect(),
                });
            }
        }
    }
    let batches: Vec<LevelBatch> = statics.chain(instanced).collect();

    Level {
        batches,
        world: Rc::new(world),
        spawns,
        practicals,
        buildings: world_system.buildings,
        static_tris: world_system.stats.static_tris,
        instanced_tris: world_system.stats.inst_tris,
        instances: world_system.stats.instances,
        draw_calls: world_system.stats.draw_calls,
        collide_tris,
    }
}

/// Decompose one of the Assembler's placement matrices into the engine's
/// [`Transform`].
///
/// The Assembler composes every placement as `translate * rotate * scale`
/// ([`crate::world::kit::trs`]), so the decomposition is exact rather than a
/// best fit: the translation is the fourth column, each scale is a basis
/// column's length, and the rotation is the matrix of the normalised columns.
/// A zero-scale column cannot occur (`put` scales by `s` and `put_s` by an
/// authored triple, none of them zero), but it is guarded so a degenerate
/// prototype placement yields identity rather than a NaN quaternion in a
/// vertex buffer.
fn transform_of(m: &Mat4) -> Transform {
    let c = m.as_cols_array();
    let len = |a: usize| (c[a] * c[a] + c[a + 1] * c[a + 1] + c[a + 2] * c[a + 2]).sqrt();
    let (sx, sy, sz) = (len(0), len(4), len(8));
    let unit = |a: usize, l: f32| {
        let inv = if l > 1e-9 { 1.0 / l } else { 0.0 };
        [c[a] * inv, c[a + 1] * inv, c[a + 2] * inv]
    };
    let r = [unit(0, sx), unit(4, sy), unit(8, sz)];
    Transform::new(
        Vec3::new(c[12], c[13], c[14]),
        quat_of_basis(r),
        Vec3::new(sx, sy, sz),
    )
}

/// A unit quaternion from three orthonormal basis columns (`r[col][row]`).
///
/// Shepperd's method: take the branch whose divisor is largest, so the
/// square root never runs near zero. A basis that is not a rotation (a
/// degenerate placement, guarded above) falls out as the identity.
fn quat_of_basis(r: [[f32; 3]; 3]) -> Quat {
    let (m00, m11, m22) = (r[0][0], r[1][1], r[2][2]);
    let trace = m00 + m11 + m22;
    let q = if trace > 0.0 {
        let s = (trace + 1.0).max(0.0).sqrt() * 2.0;
        [
            (r[1][2] - r[2][1]) / s,
            (r[2][0] - r[0][2]) / s,
            (r[0][1] - r[1][0]) / s,
            0.25 * s,
        ]
    } else if m00 > m11 && m00 > m22 {
        let s = (1.0 + m00 - m11 - m22).max(0.0).sqrt() * 2.0;
        [
            0.25 * s,
            (r[1][0] + r[0][1]) / s,
            (r[2][0] + r[0][2]) / s,
            (r[1][2] - r[2][1]) / s,
        ]
    } else if m11 > m22 {
        let s = (1.0 + m11 - m00 - m22).max(0.0).sqrt() * 2.0;
        [
            (r[1][0] + r[0][1]) / s,
            0.25 * s,
            (r[2][1] + r[1][2]) / s,
            (r[2][0] - r[0][2]) / s,
        ]
    } else {
        let s = (1.0 + m22 - m00 - m11).max(0.0).sqrt() * 2.0;
        [
            (r[2][0] + r[0][2]) / s,
            (r[2][1] + r[1][2]) / s,
            0.25 * s,
            (r[0][1] - r[1][0]) / s,
        ]
    };
    Quat::new(q[0], q[1], q[2], q[3])
        .normalize()
        .unwrap_or(Quat::IDENTITY)
}

/// Flatten an indexed (or soup) [`WorldGeo`] into the `f64` `a.xyz b.xyz c.xyz`
/// triples [`StaticWorld::add_triangles`] registers, and the triangle count.
///
/// The BVH stores its triangles as `f32` internally, so widening here loses
/// nothing — it is the shape the registration API takes.
fn triangle_soup(geo: &WorldGeo) -> (Vec<f64>, usize) {
    let vertex = |i: usize| {
        [
            f64::from(geo.pos[i * 3]),
            f64::from(geo.pos[i * 3 + 1]),
            f64::from(geo.pos[i * 3 + 2]),
        ]
    };
    let indices: Vec<u32> = if geo.index.is_empty() {
        (0..geo.vert_count() as u32).collect()
    } else {
        geo.index.clone()
    };
    let count = indices.len() / 3;
    let mut out = Vec::with_capacity(count * 9);
    for t in 0..count {
        for k in 0..3 {
            out.extend_from_slice(&vertex(indices[t * 3 + k] as usize));
        }
    }
    (out, count)
}

/// One batch's [`WorldGeo`] as engine-registerable geometry.
///
/// [`MeshData`] validation wants one normal and one UV per vertex; an `Accum`
/// batch built from geometry that never carried a UV has an empty `uv`, so the
/// missing attribute is filled rather than left short. An un-indexed batch is a
/// triangle soup, so the identity index is synthesised — the same two fixes
/// `crate::viewer::to_mesh_data` makes for the weapon kit's `Geo`.
fn to_mesh_data(geo: &WorldGeo) -> MeshData {
    let positions: Vec<Vec3> = geo
        .pos
        .chunks_exact(3)
        .map(|c| Vec3::new(c[0], c[1], c[2]))
        .collect();
    let mut normals: Vec<Vec3> = geo
        .normal
        .chunks_exact(3)
        .map(|c| Vec3::new(c[0], c[1], c[2]))
        .collect();
    normals.resize(positions.len(), Vec3::new(0.0, 1.0, 0.0));
    let mut uvs: Vec<Vec2> = geo
        .uv
        .chunks_exact(2)
        .map(|c| Vec2::new(c[0], c[1]))
        .collect();
    uvs.resize(positions.len(), Vec2::new(0.0, 0.0));
    let indices = if geo.index.is_empty() {
        (0..positions.len() as u32).collect()
    } else {
        geo.index.clone()
    };
    MeshData::new(positions, normals, uvs, indices)
}

/// A linear colour channel from a known-finite authored literal.
fn ch(value: f32) -> Ratio {
    Ratio::new(value).expect("authored colour channel is finite")
}

/// The linear albedo for a palette key — its authored `tint` hex, de-gamma'd.
/// A key with no tint (the palette leaves `tint: None` where the material's own
/// generator owns the colour) falls back to a neutral mid grey, which is what
/// an untinted `MeshStandardMaterial` is.
pub fn key_albedo(key: &str) -> Color {
    let hex = Palette::ALL
        .iter()
        .find(|(name, _)| *name == key)
        .and_then(|(_, entry)| entry.opts.tint)
        .unwrap_or(0xb0b0b0);
    hex_to_linear(hex)
}

/// One packed `0xRRGGBB` as a linear [`Color`].
///
/// The source authors every colour — a palette tint, a light, an emissive — as
/// a hex literal, and three decodes all of them through the sRGB transfer
/// function (`THREE.ColorManagement`, on by default in r180). Shared so a
/// second caller cannot quietly grow a third copy of the same six lines: there
/// were already two.
pub fn hex_to_linear(hex: u32) -> Color {
    let srgb = |shift: u32| ((hex >> shift) & 0xff) as f32 / 255.0;
    Color::linear_rgb(
        ch(srgb_to_linear(srgb(16))),
        ch(srgb_to_linear(srgb(8))),
        ch(srgb_to_linear(srgb(0))),
    )
}

/// The sRGB electro-optical transfer function. Three.js decodes every hex
/// colour literal this way (`THREE.ColorManagement`, on by default in r180), so
/// a tint used raw as a linear albedo would render visibly washed out.
fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physics::surfaces::mask;

    fn level() -> Level {
        build_level(&mut Rng::new(0x5eed_1234))
    }

    #[test]
    fn the_level_builds_real_geometry_and_a_real_bvh() {
        let level = level();
        assert!(!level.batches.is_empty(), "ground authored no batch");
        assert!(level.static_tris > 1000, "got {}", level.static_tris);
        assert!(level.collide_tris > 0);
        assert_eq!(level.world.tri_count(), level.collide_tris);
        assert!(level.world.node_count() > 1, "the BVH was built");
    }

    #[test]
    fn every_batch_is_valid_renderable_geometry() {
        let level = level();
        for batch in &level.batches {
            let m = &batch.mesh;
            assert!(!m.positions().is_empty(), "{}: no positions", batch.key);
            assert_eq!(m.normals().len(), m.positions().len(), "{}", batch.key);
            assert_eq!(m.uvs().len(), m.positions().len(), "{}", batch.key);
            assert_eq!(m.indices().len() % 3, 0, "{}", batch.key);
            assert!(
                m.indices().iter().all(|i| (*i as usize) < m.positions().len()),
                "{}: an index is out of range",
                batch.key
            );
        }
    }

    /// **A player put at any spawn has ground under their feet.**
    ///
    /// This used to cast down from six metres above the spawn and require the
    /// FIRST thing it hit to be the floor, which quietly assumed open sky over
    /// every spawn point. The west alley has a canopy at 4.89 m, so the old
    /// ray was measuring the canopy and reporting a missing floor. Casting
    /// from just above the feet is what the claim was always about, and it
    /// still catches both failures that matter: no floor at all, and a floor
    /// at the wrong height.
    #[test]
    fn the_collision_world_has_a_floor_under_every_spawn_point() {
        let level = level();
        assert_eq!(level.spawns.len(), 8);
        for spawn in &level.spawns {
            let down = level.world.raycast(
                spawn.position[0],
                spawn.position[1] + 0.5,
                spawn.position[2],
                0.0,
                -1.0,
                0.0,
                1000.0,
                mask::WORLD,
                -1,
            );
            assert!(down.hit, "{}: nothing to stand on", spawn.tag);
            assert!(
                down.py.abs() < 1.0,
                "{}: floor at y = {}, expected near zero",
                spawn.tag,
                down.py
            );

        }
    }

    #[test]
    fn spawn_indexes_wrap_in_both_directions() {
        let level = level();
        assert_eq!(level.spawn(0).tag, "north street");
        assert_eq!(level.spawn(8).tag, "north street");
        assert_eq!(level.spawn(-1).tag, level.spawns[7].tag);
    }

    #[test]
    fn the_level_transform_is_applied_to_the_spawn_table() {
        let level = level();
        // Every spawn's yaw carries LEVEL_YAW, and the positions are rotated
        // out of level space, so none of them equals its authored (x, z).
        for (i, spawn) in level.spawns.iter().enumerate() {
            let (x, z, yaw, _) = SPAWNS[i];
            assert!((spawn.yaw - (yaw + LEVEL_YAW)).abs() < 1e-12);
            assert!(
                (spawn.position[0] - x).abs() > 1e-6 || (spawn.position[2] - z).abs() > 1e-6,
                "spawn {i} was not transformed"
            );
        }
    }

    #[test]
    fn the_level_is_deterministic_for_a_given_root_seed() {
        let a = build_level(&mut Rng::new(7));
        let b = build_level(&mut Rng::new(7));
        assert_eq!(a.static_tris, b.static_tris);
        assert_eq!(a.collide_tris, b.collide_tris);
        assert_eq!(a.batches.len(), b.batches.len());
        for (x, y) in a.batches.iter().zip(b.batches.iter()) {
            assert_eq!(x.key, y.key);
            assert_eq!(x.mesh.positions(), y.mesh.positions());
        }
    }

    #[test]
    fn a_palette_tint_becomes_a_de_gamma_d_albedo() {
        // `asphalt`'s tint is a dark grey; de-gamma'd it must be darker still,
        // and never brighter than the raw sRGB fraction.
        // `Palette::ASPHALT`'s tint is 0x9d968a; 0x9d/255 = 0.6157 sRGB, which
        // de-gamma's to about 0.337 linear.
        let asphalt = key_albedo("asphalt").to_array();
        assert!(
            (asphalt[0] - srgb_to_linear(157.0 / 255.0)).abs() < 1e-6,
            "got {}",
            asphalt[0]
        );
        assert!(asphalt[0] < 0.6157, "de-gamma darkens the authored tint");
        assert!(asphalt[2] < asphalt[0], "and 0x8a blue is darker than 0x9d red");
        // An unknown key gets the neutral fallback rather than black.
        let unknown = key_albedo("no-such-key").to_array();
        assert!(unknown[0] > 0.3 && unknown[0] < 0.6);
        assert_eq!(srgb_to_linear(0.0), 0.0);
        assert!((srgb_to_linear(1.0) - 1.0).abs() < 1e-6);
        // The linear segment below the knee.
        assert!((srgb_to_linear(0.04) - 0.04 / 12.92).abs() < 1e-9);
    }

    #[test]
    fn triangle_soup_handles_both_indexed_and_unindexed_geometry() {
        let indexed = WorldGeo {
            pos: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            normal: Vec::new(),
            uv: Vec::new(),
            color: Vec::new(),
            index: vec![0, 1, 2],
        };
        let (tris, count) = triangle_soup(&indexed);
        assert_eq!(count, 1);
        assert_eq!(tris.len(), 9);

        let soup = WorldGeo {
            index: Vec::new(),
            ..indexed
        };
        let (tris2, count2) = triangle_soup(&soup);
        assert_eq!(count2, 1);
        assert_eq!(tris2, tris);
    }
}
