//! Deterministic tree scatter **baked into** Growth's streamed terrain.
//!
//! This is the forest-walk / generia tree, ported onto Growth's world. Those apps
//! placed an authored `[scatter]` block of trunk-cylinder + canopy-blob trees over a
//! fixed diorama patch and drew them as separate instanced batches. Growth has no
//! fixed patch and no multi-batch draw: its walkable ground is ONE combined mesh
//! streamed a chunk at a time through `run_web_streaming` (12-float vertices:
//! position · normal · uv · colour, one biome-atlas texture, one draw). So instead of
//! a separate batch, a cell's trees are emitted as **world-space geometry appended to
//! that cell's terrain chunk** — the trees stream, unload, and light with the ground
//! for free.
//!
//! Placement is the ported idea from `forest-walk/src/visual_target/scatter.rs`: a
//! deterministic per-cell entropy stream draws a few candidate sites, each rejected by
//! biome (no trees in ocean / desert / tundra), by the vista treeline
//! (`MountainVistaPlan::vegetation_line_m`), by ground slope, and by a minimum spacing,
//! then seated on the real vista-composited surface. It is a pure function of
//! `(seed, cell, atlas, vista)`, so a cell grows the same forest every time it streams
//! in — and never a tree in the sea.
//!
//! Trees are baked only into the **full-detail LOD-0 near ring** (the caller's job):
//! the near cells are 16 m — one world cell — so a cell's trees never double with a
//! coarse ring covering the same ground, and the tree count stays bounded to the ring
//! around the player. Deliberately **sparse** (a couple of candidates per cell), per
//! the consolidation brief.

use axiom::prelude::Texture;
use axiom_entropy::{EntropyApi, EntropyStream};
use axiom_math::Vec3;
use axiom_space::{Address, SpaceApi};

use crate::curves::{lerp, lerp3};
use crate::gameworld::sample_height_m_lod_vista;
use crate::model_planet::PlanetSurfaceAtlas;
use crate::model_world::GameWorldLocalMap;
use crate::sampler::{self, biome};
use crate::vista::MountainVistaPlan;

/// Floats per emitted vertex: position(3) · normal(3) · uv(2) · colour(4) — the exact
/// layout `run_web_streaming` and the terrain chunks use, so trees append verbatim.
const VERT_FLOATS: usize = 12;

/// Fixed address segment keying the tree entropy stream ("growtree"), distinct from
/// the worldgen streams so the scatter never correlates with terrain.
const TREE_SEGMENT: u64 = 0x_67_72_6F_77_74_72_65_65;
/// Entropy stream version for the tree scatter (bump to re-roll every forest).
const TREE_VERSION: u32 = 1;

/// Candidate sites drawn per 16 m cell. Sparse by design — most are rejected by the
/// biome / slope / spacing gates, so a qualifying cell grows ~0–2 trees.
const CANDIDATES_PER_CELL: u32 = 3;
/// Minimum spacing (m) between two trees seated in the same cell.
const MIN_SPACING_M: f32 = 4.0;
/// Reject sites whose terrain slope (rise/run) exceeds this — no trees on cliffs.
const SLOPE_LIMIT: f32 = 0.8;

/// Radial segments in a trunk cylinder.
const TRUNK_SEGMENTS: u32 = 8;
/// Rings / sectors in a canopy blob (a low-poly UV sphere). Coarser than the
/// forest-walk instanced canopy because these are baked per cell, not instanced.
const CANOPY_RINGS: u32 = 6;
const CANOPY_SECTORS: u32 = 10;

/// Trunk height range `[min, max]` (m).
const TRUNK_HEIGHT_M: [f32; 2] = [3.0, 6.5];
/// Trunk radius range `[min, max]` (m).
const TRUNK_RADIUS_M: [f32; 2] = [0.14, 0.28];
/// Canopy radius range `[min, max]` (m).
const CANOPY_RADIUS_M: [f32; 2] = [1.4, 2.8];

/// Base bark colour (linear RGB); jittered per tree.
const BARK: [f32; 3] = [0.30, 0.22, 0.15];

/// Build the tree geometry for the LOD-0 cell at grid coord `(cell_x, cell_z)` (world
/// origin `(cell * cell_size_m)`, side `cell_size_m` metres). Returns interleaved
/// 12-float vertices in **world space, recentred by `anchor_h`** (matching the terrain
/// surface), plus triangle indices **local to the returned block** (base 0) — the
/// caller offsets them by the chunk's existing vertex count when appending.
///
/// Empty when the cell holds no qualifying site (ocean / desert / tundra / above the
/// treeline / all sites too steep or crowded), so an unforested cell costs nothing.
pub fn build_cell_trees(
    atlas: &PlanetSurfaceAtlas,
    localmap: &GameWorldLocalMap,
    seed: u64,
    plan: &MountainVistaPlan,
    anchor_h: f32,
    cell_x: i32,
    cell_z: i32,
    cell_size_m: f32,
) -> (Vec<f32>, Vec<u32>) {
    let mut stream = EntropyApi::stream(seed, &tree_address(cell_x, cell_z), TREE_VERSION);
    let origin_x = cell_x as f32 * cell_size_m;
    let origin_z = cell_z as f32 * cell_size_m;

    let mut placed: Vec<(f32, f32)> = Vec::new();
    let mut vertices: Vec<f32> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    for _ in 0..CANDIDATES_PER_CELL {
        // Draw the site and every per-tree parameter up front so a candidate consumes
        // the same stripe of the stream whether or not it is accepted — candidate k's
        // roll never depends on candidate k-1's acceptance.
        let x = origin_x + unit(&mut stream) * cell_size_m;
        let z = origin_z + unit(&mut stream) * cell_size_m;
        let trunk_height = lerp(TRUNK_HEIGHT_M[0], TRUNK_HEIGHT_M[1], unit(&mut stream));
        let trunk_radius = lerp(TRUNK_RADIUS_M[0], TRUNK_RADIUS_M[1], unit(&mut stream));
        let canopy_radius = lerp(CANOPY_RADIUS_M[0], CANOPY_RADIUS_M[1], unit(&mut stream));
        let colour_pick = unit(&mut stream);
        let bark_jitter = unit(&mut stream);

        // Biome gate: sample the overworld surface under this site.
        let dir = localmap.world_metres_to_unit_dir(x, z);
        let surface = sampler::sample_surface(atlas, Vec3::new(dir[0], dir[1], dir[2]));
        let b = surface.biome.0;
        let tree_biome = surface.elevation.get() >= 0.0
            && b != biome::OCEAN
            && b != biome::DESERT
            && b != biome::TUNDRA;
        if !tree_biome {
            continue;
        }

        // Spacing gate: reject a site too close to one already seated in this cell.
        let crowded = placed
            .iter()
            .any(|&(px, pz)| (px - x) * (px - x) + (pz - z) * (pz - z) < MIN_SPACING_M * MIN_SPACING_M);
        if crowded {
            continue;
        }

        // Height + treeline gate: absolute composited surface height, then reject above
        // the vista's vegetation line (bare rock / snow carry no trees).
        let abs_h = height_at(atlas, localmap, seed, plan, x, z);
        if abs_h > plan.vegetation_line_m {
            continue;
        }

        // Slope gate: no trees on steep flanks.
        if slope_at(atlas, localmap, seed, plan, x, z) > SLOPE_LIMIT {
            continue;
        }

        placed.push((x, z));
        let base_y = abs_h - anchor_h;
        let leaf = canopy_colour(b, surface.moisture.get(), colour_pick);
        let bark = bark_colour(bark_jitter);
        emit_trunk(&mut vertices, &mut indices, x, base_y, z, trunk_height, trunk_radius, bark);
        emit_canopy(
            &mut vertices,
            &mut indices,
            x,
            base_y + trunk_height,
            z,
            canopy_radius,
            leaf,
        );
    }

    (vertices, indices)
}

/// The per-cell entropy address: `root / TREE_SEGMENT / packed(cell_x, cell_z)`, so the
/// stream is a pure, reproducible function of the cell.
fn tree_address(cell_x: i32, cell_z: i32) -> Address {
    let key = ((cell_x as u32 as u64) << 32) | (cell_z as u32 as u64);
    SpaceApi::child(&SpaceApi::child(&SpaceApi::root(), TREE_SEGMENT), key)
}

/// A uniform `[0, 1)` sample off the deterministic stream.
fn unit(stream: &mut EntropyStream) -> f32 {
    stream.unit().get()
}

/// Absolute (un-recentred) vista-composited surface height (m) at world `(x, z)`,
/// sampled at full detail — the same height the surface vertices and collision use.
fn height_at(
    atlas: &PlanetSurfaceAtlas,
    localmap: &GameWorldLocalMap,
    seed: u64,
    plan: &MountainVistaPlan,
    x: f32,
    z: f32,
) -> f32 {
    sample_height_m_lod_vista(atlas, localmap, seed, x, z, 0.0, Some(plan))
}

/// Terrain slope magnitude (rise/run) at `(x, z)` via central difference — the same
/// shape `visual_target::scene::Terrain::slope_at` uses, over the composited sampler.
fn slope_at(
    atlas: &PlanetSurfaceAtlas,
    localmap: &GameWorldLocalMap,
    seed: u64,
    plan: &MountainVistaPlan,
    x: f32,
    z: f32,
) -> f32 {
    let e = 1.0;
    let h = |px: f32, pz: f32| height_at(atlas, localmap, seed, plan, px, pz);
    let dhx = (h(x + e, z) - h(x - e, z)) / (2.0 * e);
    let dhz = (h(x, z + e) - h(x, z - e)) / (2.0 * e);
    (dhx * dhx + dhz * dhz).sqrt()
}

/// The per-tree canopy tint (linear RGB): a biome base — deep green in rainforest,
/// dark blue-green in taiga, temperate green elsewhere — with the temperate/rainforest
/// crowns occasionally tipping into the warm autumn tone the forest-walk look carried,
/// then a small moisture darkening.
fn canopy_colour(biome_id: u32, moisture: f32, pick: f32) -> [f32; 3] {
    let base = if biome_id == biome::RAINFOREST {
        [0.10, 0.42, 0.14]
    } else if biome_id == biome::TAIGA {
        [0.13, 0.30, 0.20]
    } else {
        [0.20, 0.42, 0.18]
    };
    // Temperate + rainforest crowns tip warm ~28% of the time (the autumn accent);
    // taiga stays evergreen.
    let autumn = [0.72, 0.42, 0.12];
    let warm = if biome_id != biome::TAIGA && pick > 0.72 { 0.8 } else { 0.0 };
    let tinted = lerp3(base, autumn, warm);
    let m = 0.85 + 0.15 * moisture.clamp(0.0, 1.0);
    [tinted[0] * m, tinted[1] * m, tinted[2] * m]
}

/// The per-tree bark tint (linear RGB): [`BARK`] scaled by a per-tree brightness jitter.
fn bark_colour(jitter: f32) -> [f32; 3] {
    let s = 0.85 + 0.30 * jitter;
    [BARK[0] * s, BARK[1] * s, BARK[2] * s]
}

/// A neutral (near-white) UV into the biome atlas so a tree's per-vertex colour shows
/// true. The atlas is a 2×2 sand/grass/rock/snow packing; the snow cell (id 3) is the
/// whitest, so its centre is the least-tinting multiplier available on the one bound
/// texture. Sampling the cell centre (`+0.25`) avoids bleeding into neighbour cells.
fn neutral_uv() -> [f32; 2] {
    let (u0, v0) = Texture::biome_cell_origin(3);
    [u0 + 0.25, v0 + 0.25]
}

/// Push one interleaved 12-float vertex.
fn push_vertex(v: &mut Vec<f32>, pos: [f32; 3], normal: [f32; 3], uv: [f32; 2], colour: [f32; 3]) {
    v.extend_from_slice(&[
        pos[0], pos[1], pos[2], normal[0], normal[1], normal[2], uv[0], uv[1], colour[0], colour[1],
        colour[2], 1.0,
    ]);
}

/// Emit a straight `TRUNK_SEGMENTS`-gon trunk cylinder (radius `radius`, from `base_y`
/// to `base_y + height`) at world `(x, z)`, outward radial normals, bark-tinted and
/// darkened toward the base. Indices are appended relative to the current vertex count.
fn emit_trunk(
    v: &mut Vec<f32>,
    idx: &mut Vec<u32>,
    x: f32,
    base_y: f32,
    z: f32,
    height: f32,
    radius: f32,
    bark: [f32; 3],
) {
    let seg = TRUNK_SEGMENTS;
    let start = (v.len() / VERT_FLOATS) as u32;
    let uv = neutral_uv();
    for s in 0..=seg {
        let a = (s as f32 / seg as f32) * std::f32::consts::TAU;
        let (nx, nz) = (a.cos(), a.sin());
        // Bottom then top; the base reads darker (root/ambient occlusion).
        for (ring, yy) in [(0.0, base_y), (1.0, base_y + height)] {
            let shade = 0.7 + 0.3 * ring;
            let c = [bark[0] * shade, bark[1] * shade, bark[2] * shade];
            push_vertex(v, [x + nx * radius, yy, z + nz * radius], [nx, 0.0, nz], uv, c);
        }
    }
    for s in 0..seg {
        let b = start + s * 2;
        idx.extend_from_slice(&[b, b + 2, b + 1, b + 1, b + 2, b + 3]);
    }
}

/// Emit a low-poly UV-sphere canopy blob (radius `r`, centred at world `(cx, cy, cz)`),
/// normals = normalized offset, leaf-tinted. Indices appended relative to current count.
fn emit_canopy(v: &mut Vec<f32>, idx: &mut Vec<u32>, cx: f32, cy: f32, cz: f32, r: f32, leaf: [f32; 3]) {
    let rings = CANOPY_RINGS;
    let sectors = CANOPY_SECTORS;
    let start = (v.len() / VERT_FLOATS) as u32;
    let uv = neutral_uv();
    for ri in 0..=rings {
        let phi = (ri as f32 / rings as f32) * std::f32::consts::PI;
        let (sp, cp) = (phi.sin(), phi.cos());
        for si in 0..=sectors {
            let theta = (si as f32 / sectors as f32) * std::f32::consts::TAU;
            let (st, ct) = (theta.sin(), theta.cos());
            let n = [sp * ct, cp, sp * st];
            push_vertex(v, [cx + n[0] * r, cy + n[1] * r, cz + n[2] * r], n, uv, leaf);
        }
    }
    let stride = sectors + 1;
    for ri in 0..rings {
        for si in 0..sectors {
            let a = start + ri * stride + si;
            let b = a + stride;
            idx.extend_from_slice(&[a, b, a + 1, a + 1, b, b + 1]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_world::GameWorldLocalMap;
    use crate::presets::PlanetPreset;
    use crate::vista::{VistaConfig, VistaDirector};
    use crate::Growth;

    /// A vista plan anchored at a **forested land** descent spot: the overworld is
    /// scanned cheaply for candidate directions whose biome grows trees, then the (few)
    /// candidates are planned in turn until one whose near band actually seats trees is
    /// found. This proves the whole pipeline — placement + biome/slope/treeline gating +
    /// geometry — yields a forest somewhere on an Earthlike planet, deterministically.
    fn forested_fixture() -> (Growth, GameWorldLocalMap, MountainVistaPlan) {
        let growth = Growth::generate("tree-seed", PlanetPreset::Earthlike, 2048);
        // Cheap first pass: overworld directions whose biome is a tree biome on land.
        let mut candidates: Vec<Vec3> = Vec::new();
        for i in 0..48 {
            for j in 0..48 {
                let u = (i as f32 + 0.5) / 48.0;
                let v = (j as f32 + 0.5) / 48.0;
                let dir = crate::ground::map_pick_to_dir(u, v);
                let s = sampler::sample_surface(&growth.atlas, dir);
                let b = s.biome.0;
                let tree = s.elevation.get() >= 0.0
                    && b != biome::OCEAN
                    && b != biome::DESERT
                    && b != biome::TUNDRA;
                if tree {
                    candidates.push(dir);
                }
            }
        }
        assert!(!candidates.is_empty(), "an Earthlike planet has forested land");
        // Plan each candidate (heavy) only until one grows trees in its near band.
        for dir in candidates {
            let localmap = GameWorldLocalMap::anchored_at(&growth.atlas, dir);
            let plan = VistaDirector::plan(
                &growth.atlas,
                &localmap,
                growth.seed.value,
                VistaConfig::default(),
            );
            let grows = (-4..4).any(|cx| {
                (-4..4).any(|cz| {
                    !build_cell_trees(
                        &growth.atlas,
                        &localmap,
                        growth.seed.value,
                        &plan,
                        plan.shelf_height_m,
                        cx,
                        cz,
                        16.0,
                    )
                    .0
                    .is_empty()
                })
            });
            if grows {
                return (growth, localmap, plan);
            }
        }
        panic!("no forested descent spot grew a tree in its near band");
    }

    #[test]
    fn same_cell_reproduces_the_same_forest() {
        let (g, lm, plan) = forested_fixture();
        let seed = g.seed.value;
        let anchor = plan.shelf_height_m;
        let a = build_cell_trees(&g.atlas, &lm, seed, &plan, anchor, 2, -1, 16.0);
        let b = build_cell_trees(&g.atlas, &lm, seed, &plan, anchor, 2, -1, 16.0);
        assert_eq!(a.0, b.0, "same cell → byte-identical tree vertices");
        assert_eq!(a.1, b.1, "same cell → byte-identical tree indices");
    }

    #[test]
    fn emitted_geometry_is_well_formed_and_a_forest_grows() {
        let (g, lm, plan) = forested_fixture();
        let seed = g.seed.value;
        let anchor = plan.shelf_height_m;
        // Every emitted block is structurally valid (whole vertices/triangles, indices
        // in range) and — by construction of the fixture — at least one cell forests.
        let mut any = false;
        for cx in -4..4 {
            for cz in -4..4 {
                let (v, idx) = build_cell_trees(&g.atlas, &lm, seed, &plan, anchor, cx, cz, 16.0);
                assert_eq!(v.len() % VERT_FLOATS, 0, "whole vertices");
                assert_eq!(idx.len() % 3, 0, "whole triangles");
                let verts = (v.len() / VERT_FLOATS) as u32;
                assert!(idx.iter().all(|&i| i < verts), "indices stay in the block");
                any |= !v.is_empty();
            }
        }
        assert!(any, "the forested fixture seats trees in its near band");
    }
}
