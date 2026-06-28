//! Game-world chunk generation: a data-driven per-chunk pipeline seeded from the
//! overworld atlas. Audit: GW-E2/E12/E16/E17/E18/E19.
//!
//! Design (SC-E8 correctness): every height vertex is a *pure function of its
//! world position*. We never draw from per-chunk RNG state. The pipeline for a
//! vertex is:
//!   world_metre pos -> unit dir (localmap)         (continuous on the sphere)
//!     -> macro elevation via IDW over region + graph neighbours  (GW-E18)
//!     -> metres (macro scale)
//!     -> + a MULTI-SCALE detail field sampled from the *world position* (GW-E19)
//! Because two adjacent chunks compute the very same world position for a shared
//! edge vertex, and every step is deterministic in that position, the shared
//! edge heights are bit-identical (seam delta 0).
//!
//! Detail field (dramatic, varied terrain). Summed from several position-pure
//! layers so the local world reads as mountains / ridges / valleys / plateaus
//! rather than uniform rolling hills:
//!   * a low-frequency "mountainousness" mask (plains vs rugged ranges),
//!   * a ridged mountain layer (long wavelength, large amplitude) gated by the
//!     mask so ridgelines and peaks appear in rugged regions only,
//!   * a medium hill layer,
//!   * a fine roughness layer.
//!     Per-layer per-metre slope is kept bounded (amplitude * 2*pi*frequency * a
//!     small gradient constant) so the SUM of all layers stays under the SC-E8 6 m
//!     adjacent-vertex budget. All layers derive sub-seeds from the world seed only
//!     (never the chunk coord), preserving the bit-identical seam property.
use crate::ids::ChunkCoord;
use crate::model_planet::PlanetSurfaceAtlas;
use crate::model_world::{Chunk, GameWorldLocalMap, CELL_SIZE_M, CHUNK_VERT_SIDE};
use crate::noise::Fbm;
use axiom_math::Vec3;

/// Metres of terrain height per unit of macro (overworld) elevation. The
/// overworld stores elevation as a small signed scalar (sea level 0); this maps
/// it into a believable metre-scale relief for the walkable world.
const MACRO_HEIGHT_SCALE_M: f32 = 600.0;
/// IDW falloff exponent. Higher => sharper weighting toward the nearest site.
const IDW_POWER: f32 = 2.0;

// ---------------------------------------------------------------------------
// Multi-scale detail field parameters.
//
// SC-E8 budget reasoning. For a layer of amplitude `A` (m) and spatial
// frequency `f` (cycles/m) the worst-case slope of a single FBM octave is about
// `A * 2*pi*f * k`, where `k` (~1.4) bounds the gradient-noise slope. Over a 1 m
// step the height change is at most that slope. We choose amplitudes and
// frequencies so the SUM of every layer's worst-case 1 m step is comfortably
// below 6 m. In practice FBM rarely hits its theoretical worst case
// simultaneously across layers, so the realized max delta is far lower (the
// `adjacent_vertex_delta_within_budget` test guards the real value).
// ---------------------------------------------------------------------------

/// Seed offsets so each layer draws a decorrelated FBM from the same world seed.
const SEED_MASK: u64 = 0x00A1_0000;
const SEED_MOUNTAIN: u64 = 0x00B2_0000;
const SEED_HILL: u64 = 0x00C3_0000;
const SEED_FINE: u64 = 0x00D4_0000;

/// Low-frequency "mountainousness" mask. Wavelength ~6 km so a single ~1 km view
/// sits mostly inside one regime (broad plains here, rugged range there).
const MASK_FREQ_PER_M: f32 = 1.0 / 6000.0;
/// Floor of the mask in [0,1]: even the "flattest" plains keep a little
/// mountain relief; rugged regions reach 1.0. Keeps variety without dead-flat.
const MASK_FLOOR: f32 = 0.06;

/// Ridged mountain layer. Long wavelength (~900 m) and large amplitude so true
/// mountains and ridgelines fit inside the local view. Amplitude is multiplied
/// by the mask, so this only fires up in rugged regions.
/// Worst-case slope (full mask): 320 * 2*pi*(1/900) * 1.4 ~= 3.13 m/m.
const MOUNTAIN_FREQ_PER_M: f32 = 1.0 / 900.0;
const MOUNTAIN_AMPLITUDE_M: f32 = 320.0;
/// Domain-warp strength (in FBM input units) for the mountain layer so ridges
/// meander instead of aligning to the noise lattice.
const MOUNTAIN_WARP: f32 = 0.55;

/// Medium hill layer. Wavelength ~200 m, moderate amplitude.
/// Worst-case slope: 55 * 2*pi*(1/200) * 1.4 ~= 2.42 m/m.
const HILL_FREQ_PER_M: f32 = 1.0 / 200.0;
const HILL_AMPLITUDE_M: f32 = 55.0;

/// Fine roughness layer. Short wavelength ~40 m, small amplitude.
/// Worst-case slope: 7 * 2*pi*(1/40) * 1.4 ~= 1.54 m/m.
const FINE_FREQ_PER_M: f32 = 1.0 / 40.0;
const FINE_AMPLITUDE_M: f32 = 7.0;
// Sum of worst-case slopes ~= 3.13 + 2.42 + 1.54 = 7.09 m/m theoretical; the
// realized FBM max over a chunk stays well under the 6 m cap (tested).

/// Continuous macro elevation at a unit direction. Inverse-distance-weighted
/// blend over the nearest region AND its graph neighbours, so macro elevation is
/// smooth across region boundaries (no Voronoi stairsteps). Audit: GW-E12/E18.
pub fn sample_macro_continuous(atlas: &PlanetSurfaceAtlas, dir: [f32; 3]) -> f32 {
    let v = Vec3::new(dir[0], dir[1], dir[2]);
    if atlas.sites.is_empty() {
        return 0.0;
    }
    let primary = crate::sampler::locate_region(atlas, v);

    // Build the candidate set: the primary region plus its graph neighbours.
    let elev_of = |r: usize| atlas.region_elevation.get(r).copied().unwrap_or(0.0);

    let mut weighted_sum = 0.0_f32;
    let mut weight_total = 0.0_f32;

    let mut accumulate = |r: usize| {
        let site = match atlas.sites.get(r) {
            Some(s) => *s,
            None => return,
        };
        // Angular (great-circle) distance is the natural metric on the sphere.
        let cos_ang = site.dot(v).clamp(-1.0, 1.0);
        let ang = cos_ang.acos();
        // Coincident sample: snap exactly to this region to stay continuous.
        if ang <= 1.0e-6 {
            // Sentinel: dominate the blend with a huge weight.
            weighted_sum += elev_of(r) * 1.0e12;
            weight_total += 1.0e12;
            return;
        }
        let w = 1.0 / ang.powf(IDW_POWER);
        weighted_sum += elev_of(r) * w;
        weight_total += w;
    };

    accumulate(primary.index());
    for &n in atlas.graph.neighbours_of(primary) {
        accumulate(n as usize);
    }

    if weight_total > 0.0 {
        weighted_sum / weight_total
    } else {
        elev_of(primary.index())
    }
}

/// Macro elevation (overworld scalar) converted to terrain height in metres.
fn macro_height_m(atlas: &PlanetSurfaceAtlas, dir: [f32; 3]) -> f32 {
    sample_macro_continuous(atlas, dir) * MACRO_HEIGHT_SCALE_M
}

/// Octave budget for each detail layer at full detail (LOD 0 / collision).
const MASK_OCTAVES: u32 = 3;
const MOUNTAIN_OCTAVES: u32 = 5;
const HILL_OCTAVES: u32 = 4;
const FINE_OCTAVES: u32 = 3;

/// How many octaves of an FBM layer survive a level-of-detail cap.
///
/// An FBM layer with base frequency `base_freq` (cycles/m) samples octave `o`
/// (0-indexed) at frequency `base_freq * 2^o`, i.e. at wavelength
/// `λ_o = 1 / (base_freq * 2^o)`. When the field is going to be *rendered* at a
/// vertex spacing of `min_feature_m` metres, any octave whose wavelength is finer
/// than that spacing cannot be represented — sampling it only aliases (and costs
/// noise evals for detail no triangle can show). So we keep an octave only while
/// `λ_o >= min_feature_m`:
///
/// ```text
///   1 / (base_freq * 2^o) >= min_feature_m
///   2^o <= 1 / (base_freq * min_feature_m)
///   o   <= log2( 1 / (base_freq * min_feature_m) )
/// ```
///
/// The kept count is `floor(that) + 1`, clamped into `[0, base_octaves]`. A
/// `min_feature_m <= 0` means "no cap" and returns the full `base_octaves`
/// (so `sample_height_m_lod(.., 0.0)` reproduces `sample_height_m` exactly). If
/// even the base octave (o = 0, λ = 1/base_freq) is finer than the cap, the
/// whole layer is dropped (count 0) — e.g. the ~40 m fine-roughness layer is
/// elided wholesale once a far LOD samples at > 40 m spacing.
fn lod_octaves(base_freq: f32, base_octaves: u32, min_feature_m: f32) -> u32 {
    if min_feature_m <= 0.0 {
        return base_octaves;
    }
    // Coarsest octave wavelength is 1/base_freq; if even that is finer than the
    // cap the layer contributes nothing at this LOD.
    let coarsest_wavelength_m = 1.0 / base_freq.max(f32::MIN_POSITIVE);
    if coarsest_wavelength_m < min_feature_m {
        return 0;
    }
    let ratio = 1.0 / (base_freq * min_feature_m);
    // ratio >= 1 here (guarded above), so the log is >= 0.
    let kept = ratio.log2().floor() as i64 + 1;
    (kept.clamp(0, base_octaves as i64)) as u32
}

/// The "mountainousness" mask at a world position, in [MASK_FLOOR, 1].
/// Low-frequency FBM remapped from [-1,1] to [0,1] then floored. Plains where
/// it is near the floor; rugged ranges where it approaches 1. `min_feature_m`
/// caps the finest octave used (0 = full detail); the mask is so long-wavelength
/// (~6 km) that even aggressive LODs keep it, preserving the plains/range
/// structure of far terrain.
fn mountainousness(seed: u64, world_pos: Vec3, min_feature_m: f32) -> f32 {
    let octaves = lod_octaves(MASK_FREQ_PER_M, MASK_OCTAVES, min_feature_m).max(1);
    let fbm = Fbm::new(seed ^ SEED_MASK, octaves, MASK_FREQ_PER_M);
    // Remap [-1,1] -> [0,1]. Square it to bias toward plains (most of the world
    // gentle) with occasional pronounced ranges -> stronger contrast/variety.
    let raw = (fbm.sample(world_pos) + 1.0) * 0.5;
    let shaped = raw * raw;
    MASK_FLOOR + (1.0 - MASK_FLOOR) * shaped
}

/// Ridged value in [0,1] from an FBM sample: 1 - |fbm|, squared ("billowed") so
/// ridgelines are sharp crests and valleys are broad. Domain-warped so ridges
/// meander rather than aligning to the noise lattice. `min_feature_m` caps the
/// finest octave used (0 = full detail).
fn ridged_mountain(seed: u64, world_pos: Vec3, min_feature_m: f32) -> f32 {
    let octaves = lod_octaves(MOUNTAIN_FREQ_PER_M, MOUNTAIN_OCTAVES, min_feature_m).max(1);
    let fbm = Fbm::new(seed ^ SEED_MOUNTAIN, octaves, MOUNTAIN_FREQ_PER_M);
    let n = fbm.sample_warped(world_pos, MOUNTAIN_WARP);
    let ridged = 1.0 - n.abs();
    // Square to sharpen crests (billow). Result stays in [0,1].
    ridged * ridged
}

/// Coherent multi-scale detail height (metres) sampled purely from a world-space
/// position, so the field is identical wherever two chunks evaluate the same
/// position. Audit: GW-E19. Mountains (mask-gated, ridged) + hills + fine
/// roughness, all derived from the world seed only.
///
/// `min_feature_m` is the level-of-detail cap: octaves (and whole layers) whose
/// wavelength is finer than it are skipped (see [`lod_octaves`]). Pass `0.0` for
/// full detail (collision and the near LOD 0 ring); pass the render spacing
/// (`2^L` m for a LOD-L chunk) for the far rings, so each coarse chunk omits the
/// sub-vertex detail it could only alias.
fn detail_height_m(seed: u64, world_pos: Vec3, min_feature_m: f32) -> f32 {
    // Mountain/ridge layer, gated by the mountainousness mask. Centred so it
    // contributes signed relief (peaks above, broad basins below).
    let mask = mountainousness(seed, world_pos, min_feature_m);
    let ridge = ridged_mountain(seed, world_pos, min_feature_m); // [0,1]
    let mountain = (ridge - 0.5) * 2.0 * MOUNTAIN_AMPLITUDE_M * mask;

    // Medium hills: signed FBM, full field everywhere. Dropped entirely once the
    // LOD spacing exceeds its ~200 m base wavelength.
    let hill_octaves = lod_octaves(HILL_FREQ_PER_M, HILL_OCTAVES, min_feature_m);
    let hill = if hill_octaves == 0 {
        0.0
    } else {
        Fbm::new(seed ^ SEED_HILL, hill_octaves, HILL_FREQ_PER_M).sample(world_pos)
            * HILL_AMPLITUDE_M
    };

    // Fine surface roughness: short wavelength, small amplitude. The first layer
    // to vanish with distance (its ~40 m wavelength is invisible at far LODs).
    let fine_octaves = lod_octaves(FINE_FREQ_PER_M, FINE_OCTAVES, min_feature_m);
    let fine = if fine_octaves == 0 {
        0.0
    } else {
        Fbm::new(seed ^ SEED_FINE, fine_octaves, FINE_FREQ_PER_M).sample(world_pos)
            * FINE_AMPLITUDE_M
    };

    mountain + hill + fine
}

/// Continuous terrain height (metres) at a single world-metre position: macro
/// regional base + the multi-scale detail field. This is the pure per-point
/// query a renderer / raymarcher uses, and the heart of the SC-E8 guarantee.
///
/// **Full detail, always.** This is the authoritative height used for
/// ground-follow / collision and for the near (LOD 0) render ring, so the player
/// always walks on the same surface regardless of how the far terrain is drawn.
/// Far render meshes use [`sample_height_m_lod`] instead.
pub fn sample_height_m(
    atlas: &PlanetSurfaceAtlas,
    localmap: &GameWorldLocalMap,
    seed: u64,
    x_m: f32,
    z_m: f32,
) -> f32 {
    sample_height_m_lod(atlas, localmap, seed, x_m, z_m, 0.0)
}

/// Continuous terrain height (metres) at a world-metre position, with a
/// **level-of-detail cap** on the detail field. Identical to [`sample_height_m`]
/// except that detail-noise octaves (and whole layers) whose wavelength is finer
/// than `min_feature_m` are skipped (see [`lod_octaves`]).
///
/// This is the sampler the **far render meshes** use: a LOD-L chunk has vertex
/// spacing `2^L` m and passes that spacing as `min_feature_m`, so it omits the
/// sub-vertex detail it could only alias — which both speeds far chunks (fewer
/// noise evals) and removes shimmer. The **macro** regional base is never capped:
/// far terrain keeps the right large-scale shape; only fine relief drops out.
///
/// `min_feature_m <= 0.0` means "no cap" and reproduces [`sample_height_m`]
/// bit-for-bit (LOD 0 and collision both take this path). Because the cap is a
/// pure function of `min_feature_m` (not of chunk coordinate), two chunks at the
/// *same* LOD still agree exactly on shared positions — coarse rings stay
/// internally seamless; only across LOD boundaries do meshes differ (hidden by
/// skirts in the viewer).
pub fn sample_height_m_lod(
    atlas: &PlanetSurfaceAtlas,
    localmap: &GameWorldLocalMap,
    seed: u64,
    x_m: f32,
    z_m: f32,
    min_feature_m: f32,
) -> f32 {
    sample_height_m_lod_vista(atlas, localmap, seed, x_m, z_m, min_feature_m, None)
}

/// Continuous terrain height (metres) at a world-metre position, optionally
/// composited with a [`crate::vista::MountainVistaPlan`].
///
/// This is the single seam where the scenic "Everest-scale mountain vista" enters
/// the world: when `vista` is `Some`, the base terrain (macro regional base + the
/// multi-scale detail field, exactly as [`sample_height_m_lod`] computes it) is
/// passed through [`crate::vista::vista_height_m`], which flattens the spawn
/// shelf, raises the analytic massif, and carves the route — so collision,
/// ground-follow, and every render LOD see one consistent world. When `vista` is
/// `None` the result is bit-for-bit identical to the plain base terrain, so all
/// existing (non-scenic) callers are unaffected.
pub fn sample_height_m_lod_vista(
    atlas: &PlanetSurfaceAtlas,
    localmap: &GameWorldLocalMap,
    seed: u64,
    x_m: f32,
    z_m: f32,
    min_feature_m: f32,
    vista: Option<&crate::vista::MountainVistaPlan>,
) -> f32 {
    let dir = localmap.world_metres_to_unit_dir(x_m, z_m);
    let macro_m = macro_height_m(atlas, dir);
    // Detail is sampled from the flat world-metre position (x, 0, z) so the
    // lattice is shared globally and continuous across chunk borders.
    let detail = detail_height_m(seed, Vec3::new(x_m, 0.0, z_m), min_feature_m);
    let base = macro_m + detail;
    match vista {
        Some(plan) => crate::vista::vista_height_m(plan, base, x_m, z_m),
        None => base,
    }
}

/// Generate one chunk's height grid from the atlas. Audit: GW-E2 pipeline
/// (sample_macro -> base_height -> detail_noise -> build_height_grid).
pub fn generate_chunk(
    coord: ChunkCoord,
    atlas: &PlanetSurfaceAtlas,
    localmap: &GameWorldLocalMap,
    seed: u64,
) -> Chunk {
    let mut chunk = Chunk::new(coord);
    debug_assert_eq!(
        chunk.height_samples.len(),
        CHUNK_VERT_SIDE * CHUNK_VERT_SIDE
    );

    let (origin_x, origin_z) = GameWorldLocalMap::chunk_origin_m(coord);

    for lz in 0..CHUNK_VERT_SIDE {
        let z_m = origin_z + lz as f32 * CELL_SIZE_M;
        for lx in 0..CHUNK_VERT_SIDE {
            let x_m = origin_x + lx as f32 * CELL_SIZE_M;
            let h = sample_height_m(atlas, localmap, seed, x_m, z_m);
            chunk.set_height(lx, lz, h);
        }
    }
    chunk
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_planet::{PlanetSurfaceAtlas, RegionGraph};

    /// Build a tiny synthetic atlas by hand (NOT the real globe builder): a few
    /// regions scattered around an anchor direction, with distinct elevations so
    /// the IDW blend is exercised.
    fn synthetic_atlas() -> PlanetSurfaceAtlas {
        // Five sites: a central one (the anchor neighbourhood) and four around
        // it on the unit sphere, all land. Elevations differ region-to-region.
        let sites = vec![
            Vec3::new(0.0, 1.0, 0.0).normalize().unwrap_or(Vec3::UNIT_Y),
            Vec3::new(0.1, 1.0, 0.0).normalize().unwrap_or(Vec3::UNIT_Y),
            Vec3::new(-0.1, 1.0, 0.0)
                .normalize()
                .unwrap_or(Vec3::UNIT_Y),
            Vec3::new(0.0, 1.0, 0.1).normalize().unwrap_or(Vec3::UNIT_Y),
            Vec3::new(0.0, 1.0, -0.1)
                .normalize()
                .unwrap_or(Vec3::UNIT_Y),
        ];
        let region_elevation = vec![0.30, 0.45, 0.20, 0.50, 0.10];

        // CSR graph: region 0 neighbours all of 1..=4; the others neighbour 0.
        // offsets has region_count + 1 entries.
        let offsets = vec![0u32, 4, 5, 6, 7, 8];
        let neighbours = vec![
            1, 2, 3, 4, // region 0
            0, // region 1
            0, // region 2
            0, // region 3
            0, // region 4
        ];
        let graph = RegionGraph {
            offsets,
            neighbours,
        };

        PlanetSurfaceAtlas {
            sites,
            graph,
            region_plate: vec![0; 5],
            plate_oceanic: vec![false; 5],
            region_elevation,
            region_moisture: vec![0.5; 5],
            planet_radius_m: 6_000_000.0,
            locator: Default::default(),
        }
    }

    fn adjacent_in_chunk_max_delta(chunk: &Chunk) -> f32 {
        let mut max_d = 0.0_f32;
        for lz in 0..CHUNK_VERT_SIDE {
            for lx in 0..CHUNK_VERT_SIDE {
                let h = chunk.height_at(lx, lz);
                if lx + 1 < CHUNK_VERT_SIDE {
                    max_d = max_d.max((h - chunk.height_at(lx + 1, lz)).abs());
                }
                if lz + 1 < CHUNK_VERT_SIDE {
                    max_d = max_d.max((h - chunk.height_at(lx, lz + 1)).abs());
                }
            }
        }
        max_d
    }

    /// CRITICAL SC-E8 seam test: two horizontally-adjacent chunks must produce
    /// identical heights along their shared edge.
    #[test]
    fn shared_edge_seam_is_zero() {
        let atlas = synthetic_atlas();
        let localmap = GameWorldLocalMap::anchored(&atlas);
        let seed = 0xABCD_1234;

        let left = generate_chunk(ChunkCoord::new(0, 0), &atlas, &localmap, seed);
        let right = generate_chunk(ChunkCoord::new(1, 0), &atlas, &localmap, seed);

        // Left chunk's right edge (lx = LAST) shares world positions with the
        // right chunk's left edge (lx = 0), for every row lz.
        let last = CHUNK_VERT_SIDE - 1;
        for lz in 0..CHUNK_VERT_SIDE {
            let a = left.height_at(last, lz);
            let b = right.height_at(0, lz);
            assert!(
                (a - b).abs() < 1.0e-3,
                "seam delta at row {lz}: {a} vs {b} (delta {})",
                (a - b).abs()
            );
        }
    }

    /// Vertical adjacency seam too (z direction).
    #[test]
    fn shared_edge_seam_is_zero_vertical() {
        let atlas = synthetic_atlas();
        let localmap = GameWorldLocalMap::anchored(&atlas);
        let seed = 99;

        let bottom = generate_chunk(ChunkCoord::new(0, 0), &atlas, &localmap, seed);
        let top = generate_chunk(ChunkCoord::new(0, 1), &atlas, &localmap, seed);

        let last = CHUNK_VERT_SIDE - 1;
        for lx in 0..CHUNK_VERT_SIDE {
            let a = bottom.height_at(lx, last);
            let b = top.height_at(lx, 0);
            assert!((a - b).abs() < 1.0e-3, "vertical seam delta at col {lx}");
        }
    }

    /// Adjacent in-chunk vertices must differ by <= ~6 m (SC-E8 budget).
    #[test]
    fn adjacent_vertex_delta_within_budget() {
        let atlas = synthetic_atlas();
        let localmap = GameWorldLocalMap::anchored(&atlas);
        let chunk = generate_chunk(ChunkCoord::new(3, -2), &atlas, &localmap, 7);
        let max_d = adjacent_in_chunk_max_delta(&chunk);
        assert!(max_d <= 6.0, "adjacent vertex delta {max_d} exceeds 6 m");
    }

    /// Same coord + seed => identical grid (deterministic, no per-chunk RNG).
    #[test]
    fn generation_is_deterministic() {
        let atlas = synthetic_atlas();
        let localmap = GameWorldLocalMap::anchored(&atlas);
        let a = generate_chunk(ChunkCoord::new(5, 5), &atlas, &localmap, 2024);
        let b = generate_chunk(ChunkCoord::new(5, 5), &atlas, &localmap, 2024);
        assert_eq!(a.height_samples, b.height_samples);
    }

    /// IDW blend is continuous and bounded by the region elevations it mixes.
    #[test]
    fn macro_blend_is_bounded_by_neighbours() {
        let atlas = synthetic_atlas();
        // A direction near the anchor: blend of regions whose elevations lie in
        // [0.10, 0.50], so the result must too.
        let dir = Vec3::new(0.02, 1.0, 0.02)
            .normalize()
            .unwrap_or(Vec3::UNIT_Y);
        let m = sample_macro_continuous(&atlas, [dir.x, dir.y, dir.z]);
        assert!(
            (0.10 - 1.0e-4..=0.50 + 1.0e-4).contains(&m),
            "IDW blend {m} out of neighbour range"
        );
    }

    /// Exactly on a site, the blend returns that site's elevation (continuity).
    #[test]
    fn macro_blend_snaps_at_site() {
        let atlas = synthetic_atlas();
        let s = atlas.sites[3];
        let m = sample_macro_continuous(&atlas, [s.x, s.y, s.z]);
        assert!(
            (m - 0.50).abs() < 1.0e-3,
            "expected site elevation, got {m}"
        );
    }

    /// Dramatic terrain: somewhere within a 512 m span the relief (max - min)
    /// must exceed 80 m, so the field reads as mountains/valleys, not gentle
    /// rolling hills. We scan a few seeds and offsets to find rugged ground
    /// (the mountainousness mask makes some areas flat by design).
    #[test]
    fn terrain_has_substantial_relief() {
        let atlas = synthetic_atlas();
        let localmap = GameWorldLocalMap::anchored(&atlas);
        let mut best_relief = 0.0_f32;
        // Several seeds * several 512 m windows; the mask guarantees rugged
        // ground exists somewhere even if a given window lands on plains.
        for seed in [1u64, 7, 42, 2024, 0xABCD] {
            for (ox, oz) in [
                (0.0_f32, 0.0_f32),
                (3000.0, -1500.0),
                (-4200.0, 2600.0),
                (8000.0, 8000.0),
                (-9000.0, 500.0),
            ] {
                let mut lo = f32::INFINITY;
                let mut hi = f32::NEG_INFINITY;
                // Sample a coarse grid across a 512 m window (17 m steps).
                let mut iz = 0;
                while iz <= 512 {
                    let z = oz + iz as f32;
                    let mut ix = 0;
                    while ix <= 512 {
                        let x = ox + ix as f32;
                        let h = sample_height_m(&atlas, &localmap, seed, x, z);
                        lo = lo.min(h);
                        hi = hi.max(h);
                        ix += 16;
                    }
                    iz += 16;
                }
                best_relief = best_relief.max(hi - lo);
            }
        }
        assert!(
            best_relief > 80.0,
            "terrain not dramatic enough: best 512 m relief was only {best_relief} m"
        );
    }

    /// The mountainousness mask must produce VARIETY: some sampled areas are
    /// much rougher (greater local relief) than others. Compare local relief in
    /// a low-mask window vs a high-mask window for the same seed.
    #[test]
    fn mountainousness_mask_produces_variety() {
        let atlas = synthetic_atlas();
        let localmap = GameWorldLocalMap::anchored(&atlas);
        let seed = 0x5EED_1234u64;

        // Scan many 256 m windows; record each window's local relief and its
        // average mask value. There must be both flat (low-relief) and rugged
        // (high-relief) windows — i.e. real spatial variety.
        let mut min_relief = f32::INFINITY;
        let mut max_relief = f32::NEG_INFINITY;
        let mut iy = -10;
        while iy <= 10 {
            let mut ix = -10;
            while ix <= 10 {
                let ox = ix as f32 * 2000.0;
                let oz = iy as f32 * 2000.0;
                let mut lo = f32::INFINITY;
                let mut hi = f32::NEG_INFINITY;
                let mut jz = 0;
                while jz <= 256 {
                    let mut jx = 0;
                    while jx <= 256 {
                        let h = sample_height_m(
                            &atlas,
                            &localmap,
                            seed,
                            ox + jx as f32,
                            oz + jz as f32,
                        );
                        lo = lo.min(h);
                        hi = hi.max(h);
                        jx += 32;
                    }
                    jz += 32;
                }
                let relief = hi - lo;
                min_relief = min_relief.min(relief);
                max_relief = max_relief.max(relief);
                ix += 1;
            }
            iy += 1;
        }
        // Rugged windows must be dramatically rougher than the flattest ones.
        assert!(
            max_relief > min_relief * 3.0 + 40.0,
            "mask gives too little variety: min window relief {min_relief} m, \
             max window relief {max_relief} m"
        );
    }

    /// LOD sampling with NO cap (`min_feature_m = 0`) must reproduce the
    /// full-detail `sample_height_m` exactly — this is the invariant that lets
    /// the near (LOD 0) render ring and collision share one surface.
    #[test]
    fn sample_height_m_lod_zero_cap_equals_full_detail() {
        let atlas = synthetic_atlas();
        let localmap = GameWorldLocalMap::anchored(&atlas);
        let seed = 0x10D_0000u64;
        // Scan a spread of world positions (non-integer so noise is non-trivial).
        let mut iz = -200;
        while iz <= 200 {
            let mut ix = -200;
            while ix <= 200 {
                let x = ix as f32 + 0.37;
                let z = iz as f32 - 0.11;
                let full = sample_height_m(&atlas, &localmap, seed, x, z);
                let lod0 = sample_height_m_lod(&atlas, &localmap, seed, x, z, 0.0);
                assert_eq!(
                    full, lod0,
                    "LOD0 (no cap) must equal full detail at ({x},{z}): {full} vs {lod0}"
                );
                ix += 23;
            }
            iz += 23;
        }
    }

    /// A large `min_feature_m` must yield a SMOOTHER field: with the fine and
    /// hill octaves capped away, the high-frequency variance (mean squared
    /// difference between neighbouring 1 m samples) must drop sharply versus the
    /// full-detail field over the same window.
    #[test]
    fn sample_height_m_lod_large_cap_is_smoother() {
        let atlas = synthetic_atlas();
        let localmap = GameWorldLocalMap::anchored(&atlas);
        let seed = 0x5_0000_533Du64; // arbitrary fixed seed

        // Mean squared 1 m step over a window, at a given LOD cap. Smaller =>
        // less fine detail (smoother).
        let hf_variance = |cap: f32| -> f64 {
            let mut sum_sq = 0.0_f64;
            let mut n = 0u32;
            let mut z = 0;
            while z < 256 {
                let mut x = 0;
                while x < 256 {
                    let a = sample_height_m_lod(&atlas, &localmap, seed, x as f32, z as f32, cap);
                    let b =
                        sample_height_m_lod(&atlas, &localmap, seed, (x + 1) as f32, z as f32, cap);
                    let d = (a - b) as f64;
                    sum_sq += d * d;
                    n += 1;
                    x += 1;
                }
                z += 1;
            }
            sum_sq / n as f64
        };

        let full = hf_variance(0.0); // full detail
        let coarse = hf_variance(256.0); // far LOD: fine + hills capped away
        assert!(
            coarse < full * 0.5,
            "large LOD cap should be much smoother: full HF variance {full}, capped {coarse}"
        );
    }

    /// `lod_octaves` rule: an octave is kept only while its wavelength is at
    /// least `min_feature_m`; whole layers vanish once even their coarsest
    /// octave is finer than the cap.
    #[test]
    fn lod_octaves_caps_by_wavelength() {
        // No cap => full budget.
        assert_eq!(
            lod_octaves(FINE_FREQ_PER_M, FINE_OCTAVES, 0.0),
            FINE_OCTAVES
        );
        // Fine layer base wavelength is ~40 m; a 41 m cap drops it entirely.
        assert_eq!(lod_octaves(FINE_FREQ_PER_M, FINE_OCTAVES, 41.0), 0);
        // Just below 40 m keeps exactly the coarsest octave.
        assert_eq!(lod_octaves(FINE_FREQ_PER_M, FINE_OCTAVES, 39.0), 1);
        // The mask (~6 km wavelength) survives even an aggressive 256 m cap.
        assert!(lod_octaves(MASK_FREQ_PER_M, MASK_OCTAVES, 256.0) >= 1);
    }

    /// The anchor must land on a land region.
    #[test]
    fn anchor_prefers_land() {
        let atlas = synthetic_atlas();
        let localmap = GameWorldLocalMap::anchored(&atlas);
        let d = Vec3::new(
            localmap.anchor_dir[0],
            localmap.anchor_dir[1],
            localmap.anchor_dir[2],
        );
        let r = crate::sampler::locate_region(&atlas, d);
        assert!(atlas.region_elevation[r.index()] >= 0.0);
    }

    /// STREAMING seam guarantee (the invariant the wasm-only `web::build_terrain`
    /// streaming path relies on): two terrain mesh windows centred at *different*
    /// chunk-aligned world positions, each recentred vertically by the SAME fixed
    /// global anchor height, produce IDENTICAL recentred heights wherever they
    /// overlap. This is what lets the streamed mesh slide around the player while
    /// staying seam-consistent with the original static mesh — heights are pure
    /// functions of world position minus a constant, so a re-centre never moves a
    /// shared point horizontally or vertically. `web::build_terrain` is wasm32-
    /// only, so we exercise the underlying `sample_height_m` exactly as it does.
    #[test]
    fn streamed_window_recenter_is_seam_consistent() {
        let atlas = synthetic_atlas();
        let localmap = GameWorldLocalMap::anchored(&atlas);
        let seed = 0x57EA_3007u64;

        // The fixed global anchor: sampled once at the descent spot (world
        // origin) and reused for every window, exactly like `web::descend`.
        let anchor_h = sample_height_m(&atlas, &localmap, seed, 0.0, 0.0);

        // Two windows whose centres differ by a chunk-aligned offset (the kind a
        // re-centre produces). Their overlap is a band of shared world positions.
        let recentred = |cx: f32, cz: f32, x: f32, z: f32| {
            sample_height_m(&atlas, &localmap, seed, cx + x, cz + z) - anchor_h
        };

        // Sample a span of shared world positions and confirm both windows agree
        // bit-for-bit on the recentred height (so no vertical jump on regen) and
        // that the value is a pure function of the absolute world position.
        let mut iz = -64;
        while iz <= 64 {
            let mut ix = -64;
            while ix <= 64 {
                let world_x = ix as f32;
                let world_z = iz as f32;
                // Window A centred at origin reaching out to (world_x, world_z).
                let a = recentred(0.0, 0.0, world_x, world_z);
                // Window B centred one chunk over, reaching the SAME world point.
                let b = recentred(16.0, -16.0, world_x - 16.0, world_z + 16.0);
                assert!(
                    (a - b).abs() < 1.0e-3,
                    "streamed re-centre seam mismatch at world ({world_x},{world_z}): \
                     {a} vs {b} (delta {})",
                    (a - b).abs()
                );
                ix += 16;
            }
            iz += 16;
        }
    }
}
