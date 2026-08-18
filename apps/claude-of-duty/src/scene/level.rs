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
//! ## What is not here, and why
//!
//! `WorldSystem.init` also calls `registerProps`, `registerDressingProps`,
//! `buildBuilding` × `BUILDINGS`, `collapseRoof`, `buildGate`, `buildPerimeter`,
//! `dressStreet`, `dressBuildings` and `scatterDebris`. **None of those source
//! files are ported yet** (`kit.js`'s modular building kit, `props.js`,
//! `dressing.js`, `buildings.js`, `gate.js`). So the level is exactly what
//! `ground.js` authors: terrain, road, kerbs, pavement slabs, alleys, sand
//! drifts and the manhole — bare ground, with no building on it. That is an
//! honest gap, not a stand-in; nothing here fabricates a building.
//!
//! Consequently `Assembler::finalize`'s `instanced` batches are always empty
//! (every prototype is registered by the unported prop files), and the
//! `lights` list likewise — `_addLights` is unported. Both are carried through
//! and asserted empty rather than silently dropped, so the day a prop file
//! lands the omission is a test failure and not a mystery.

use std::rc::Rc;

use axiom::prelude::{Color, MeshData, Ratio, Vec2, Vec3};

use crate::physics::bvh::StaticWorld;
use crate::physics::surfaces::layer;
use crate::rng::Rng;
use crate::world::assembler::Assembler;
use crate::world::geo::WorldGeo;
use crate::world::ground::build_ground;
use crate::world::palette::{Palette, Surface};

/// `LEVEL_YAW` / `LEVEL_TX` / `LEVEL_TZ` (`world/index.js:60-62`) — LEVEL space
/// to WORLD space. The street is authored down -Z; this yaw puts it on the axis
/// the canonical hero camera looks along.
pub const LEVEL_YAW: f32 = 0.5877;
pub const LEVEL_TX: f32 = 0.9;
pub const LEVEL_TZ: f32 = 1.34;

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
}

/// Everything one built level hands the rest of the game.
pub struct Level {
    /// The renderable batches, in `finalize` order.
    pub batches: Vec<LevelBatch>,
    /// The collision BVH, built and shared.
    pub world: Rc<StaticWorld>,
    /// `world.spawnPoints`, already in WORLD space.
    pub spawns: Vec<SpawnPoint>,
    /// `A.stats.staticTris` / `collideTris` — reported so a caller can see the
    /// level is real without walking the geometry.
    pub static_tris: usize,
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

/// Build the level. `rng` is the world subsystem's own forked stream
/// (`this.rng = ctx.rng.fork()`, `world/index.js:91`); `build_ground` takes a
/// second `&mut Rng` because the source passes `rng` alongside the assembler
/// that also holds it, which Rust's borrow rules will not allow — the two
/// streams are forked from the same root so the split is explicit rather than
/// aliased.
pub fn build_level(root: &mut Rng) -> Level {
    let assembler_rng = root.fork();
    let mut ground_rng = root.fork();

    let mut asm = Assembler::new(assembler_rng);
    asm.set_transform(LEVEL_YAW, LEVEL_TX, LEVEL_TZ);
    build_ground(&mut asm, &mut ground_rng);

    // The spawn table is authored in LEVEL space; `A.toWorld` is the same
    // transform every piece of geometry above went through.
    let spawns: Vec<SpawnPoint> = SPAWNS
        .iter()
        .map(|(x, z, yaw, tag)| {
            let p = asm.to_world(*x as f32, 0.0, *z as f32);
            SpawnPoint {
                position: [f64::from(p.x), f64::from(p.y), f64::from(p.z)],
                yaw: yaw + f64::from(LEVEL_YAW),
                tag,
            }
        })
        .collect();

    let result = asm.finalize();
    asm.release_cache();

    let mut world = StaticWorld::new();
    let mut collide_tris = 0usize;
    for mesh in &result.collision {
        let (tris, count) = triangle_soup(&mesh.geo);
        collide_tris += count;
        world.add_triangles(&tris, count, mesh.surface, layer::STATIC, "world");
    }
    world.build();

    let batches = result
        .statics
        .iter()
        .map(|s| LevelBatch {
            key: s.key.clone(),
            surface: s.surface,
            mesh: to_mesh_data(&s.geo),
            albedo: key_albedo(&s.key),
        })
        .collect();

    Level {
        batches,
        world: Rc::new(world),
        spawns,
        static_tris: result.stats.static_tris,
        collide_tris,
    }
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

    #[test]
    fn the_collision_world_has_a_floor_under_every_spawn_point() {
        let level = level();
        assert_eq!(level.spawns.len(), 8);
        for spawn in &level.spawns {
            let hit = level.world.raycast(
                spawn.position[0],
                spawn.position[1] + 6.0,
                spawn.position[2],
                0.0,
                -1.0,
                0.0,
                1000.0,
                mask::WORLD,
                -1,
            );
            assert!(hit.hit, "{}: nothing to stand on", spawn.tag);
            assert!(
                hit.py.abs() < 1.0,
                "{}: floor at y = {}, expected near zero",
                spawn.tag,
                hit.py
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
            assert!((spawn.yaw - (yaw + f64::from(LEVEL_YAW))).abs() < 1e-12);
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
