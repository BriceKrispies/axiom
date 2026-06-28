//! Native terrain-mesh generation shared by the browser viewer (`web.rs`, wasm)
//! and the headless screenshot path (`ground::GroundSim::capture_rgba`, native).
//!
//! The far **scenic** mesh — the distant Everest-scale massif plus the atmospheric
//! far ground — is a pure function of the generated planet + the composed vista, so
//! it lives here (native, testable) rather than trapped in the wasm-only viewer.
//! The browser streams it appended to the live terrain; the screenshot path renders
//! it directly through the off-screen GPU backend. Same mesh, one source of truth.
//!
//! Vertex layout is the engine's standard 12 floats: position(3) · normal(3) ·
//! uv(2) · colour(4) — exactly what both `run_web_streaming` and
//! `GpuBackendApi::render_offscreen_rgba` consume.

use axiom::prelude::*;

use crate::gameworld::sample_height_m_lod_vista;
use crate::model_world::GameWorldLocalMap;
use crate::sampler::biome;
use crate::vista::{self, MountainVistaPlan};
use crate::Growth;

/// Floats per terrain vertex: position(3) + normal(3) + uv(2) + colour(4).
pub const VERT_FLOATS: usize = 12;

/// World-space size, in metres, of one biome-atlas cell tile on the terrain.
pub const BIOME_TILE_M: f32 = 3.0;

/// Coarse vertex spacing (m) of the far scenic mesh — fine enough for a stable
/// silhouette, coarse enough to stay cheap (built once).
const FAR_FIELD_SPACING_M: f32 = 160.0;
/// Extra ground (m) drawn beyond the mountain footprint so the horizon reads as
/// continuous land, not a cut edge.
const FAR_FIELD_MARGIN_M: f32 = 1600.0;
/// Downward offset (m) so the near full-detail chunks always occlude the coarse
/// far mesh where they overlap (invisible at the far mesh's distances).
const FAR_FIELD_DROP_M: f32 = 3.0;
/// The sky/clear colour in linear RGB (matches the viewer's clear), the target the
/// atmospheric perspective fades distant geometry toward.
const SKY_LINEAR: [f32; 3] = [0.45, 0.62, 0.85];

/// Snapshot grid radius (m): a camera-centred field wide enough to frame the whole
/// massif and the surrounding ground from a summit vantage, and to fill the view
/// when looking down from the peak (so the field edge stays past the horizon).
const SNAPSHOT_RADIUS_M: f32 = 9000.0;
/// Snapshot vertex spacing (m): finer than the streamed far field so the flanks
/// near the camera read as smooth terrain rather than coarse facets.
const SNAPSHOT_SPACING_M: f32 = 35.0;

/// Build one composited terrain field: a square grid of side `radius` (m) at
/// `spacing` (m) **centred on `(cx, cz)`**, heights from the vista-composited
/// sampler (recentred by `anchor_h`, dropped by `drop`), per-vertex colour carrying
/// the snow/rock/vegetation bands, distance-from-centre atmospheric perspective,
/// the cloud-band fade, and the pale route line. The standard 12-float layout.
///
/// Both the browser's far scenic mesh (spawn-centred, coarse) and the headless
/// snapshot mesh (camera-centred, fine) are this one routine under different
/// configs, so they stay one source of truth.
fn build_field(
    growth: &Growth,
    localmap: &GameWorldLocalMap,
    seed: u64,
    plan: &MountainVistaPlan,
    anchor_h: f32,
    cx: f32,
    cz: f32,
    radius: f32,
    spacing: f32,
    drop: f32,
) -> (Vec<f32>, Vec<u32>) {
    let side: usize = (2.0 * radius / spacing).ceil() as usize + 1;

    // Absolute composited height at a point (detail cap = spacing).
    let h_abs = |x: f32, z: f32| {
        sample_height_m_lod_vista(&growth.atlas, localmap, seed, x, z, spacing, Some(plan))
    };

    let mut vertices: Vec<f32> = Vec::with_capacity(side * side * VERT_FLOATS);
    (0..side).for_each(|jz| {
        let z = cz - radius + jz as f32 * spacing;
        (0..side).for_each(|ix| {
            let x = cx - radius + ix as f32 * spacing;
            let abs = h_abs(x, z);
            let y = abs - anchor_h - drop;

            // Central-difference normal from the field.
            let hx0 = h_abs(x - spacing, z);
            let hx1 = h_abs(x + spacing, z);
            let hz0 = h_abs(x, z - spacing);
            let hz1 = h_abs(x, z + spacing);
            let nx = -(hx1 - hx0);
            let nz = -(hz1 - hz0);
            let ny = 2.0 * spacing;
            let len = (nx * nx + ny * ny + nz * nz).sqrt().max(1.0e-6);

            // Per-vertex colour: altitude bands × atmospheric perspective (faded by
            // distance from the field centre, i.e. the viewer) × cloud-band fade,
            // then the pale route line where the trail runs.
            let dist = (x - cx).hypot(z - cz);
            let band = vista::band_color(abs, plan);
            let lit = vista::apply_atmosphere(band, dist, abs, plan, SKY_LINEAR);
            let (trail_w, _) = plan.massif.route_override(x, z);
            let col = vista::trail_tint(lit, trail_w * 0.6);

            // A neutral pale biome cell so the baked per-vertex colour dominates
            // (the distant texture detail is washed out by aerial perspective).
            let (cell_u, cell_v) = Texture::biome_cell_origin(biome::TUNDRA);
            let u = cell_u + (x / BIOME_TILE_M).rem_euclid(1.0) * 0.5;
            let v = cell_v + (z / BIOME_TILE_M).rem_euclid(1.0) * 0.5;

            vertices.extend_from_slice(&[
                x,
                y,
                z,
                nx / len,
                ny / len,
                nz / len,
                u,
                v,
                col[0],
                col[1],
                col[2],
                col[3],
            ]);
        });
    });

    let mut indices: Vec<u32> = Vec::with_capacity((side - 1) * (side - 1) * 6);
    (0..side - 1).for_each(|jz| {
        (0..side - 1).for_each(|ix| {
            let i0 = (jz * side + ix) as u32;
            let i1 = i0 + 1;
            let i2 = i0 + side as u32;
            let i3 = i2 + 1;
            // Same winding as the streamed surface quads.
            indices.extend_from_slice(&[i0, i2, i1, i1, i2, i3]);
        });
    });

    (vertices, indices)
}

/// The browser's far scenic mesh: a coarse grid centred on the spawn covering the
/// mountain and the surrounding far ground, dropped a few metres so the streamed
/// near chunks win the depth test where they overlap.
pub fn build_far_field_mesh(
    growth: &Growth,
    localmap: &GameWorldLocalMap,
    seed: u64,
    plan: &MountainVistaPlan,
    anchor_h: f32,
) -> (Vec<f32>, Vec<u32>) {
    let (cx, cz) = plan.spawn_xz;
    let radius = plan.distance_m + plan.massif.base_radius_m + FAR_FIELD_MARGIN_M;
    build_field(
        growth,
        localmap,
        seed,
        plan,
        anchor_h,
        cx,
        cz,
        radius,
        FAR_FIELD_SPACING_M,
        FAR_FIELD_DROP_M,
    )
}

/// The terrain mesh for an off-screen snapshot from camera position `(cam_x,
/// cam_z)`: a finer, camera-centred field framing the whole massif and the ground
/// dropping away on every side. The only mesh in the shot, so no depth-drop.
pub fn build_snapshot_mesh(
    growth: &Growth,
    localmap: &GameWorldLocalMap,
    seed: u64,
    plan: &MountainVistaPlan,
    anchor_h: f32,
    cam_x: f32,
    cam_z: f32,
) -> (Vec<f32>, Vec<u32>) {
    build_field(
        growth,
        localmap,
        seed,
        plan,
        anchor_h,
        cam_x,
        cam_z,
        SNAPSHOT_RADIUS_M,
        SNAPSHOT_SPACING_M,
        0.0,
    )
}
