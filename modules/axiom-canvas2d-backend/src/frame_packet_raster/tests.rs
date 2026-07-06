use super::*;
use crate::canvas_policy::CanvasDebugOverlay;
use axiom_host::{FrameCamera, FrameFeatureSet, FramePacket, FrameViewport};

/// Test wrapper for [`super::convert`]: supplies the zero clock + discarding deep
/// sink (the browser profiler hooks are the backend's job), so every existing
/// 3-arg call site stays unchanged. Shadows the glob-imported 5-arg `convert`.
fn convert(
    packet: &FramePacket,
    cache: &MeshCache,
    options: &LowPolyRasterOptions,
) -> ConvertedFrame {
    super::convert(packet, cache, &[], options, || 0.0, super::discard_deep)
}

const IDENTITY: [f32; 16] = [
    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
];

fn vertex(pos: [f32; 3], color: [f32; 4]) -> [f32; 12] {
    [
        pos[0], pos[1], pos[2], 0.0, 1.0, 0.0, 0.0, 0.0, color[0], color[1], color[2], color[3],
    ]
}

/// A full-screen quad (2 triangles) over NDC [-1,1]² → big coverage → critical.
fn ground(id: u64, color: [f32; 4]) -> (u64, Vec<f32>, Vec<u32>) {
    let mut v = Vec::new();
    v.extend_from_slice(&vertex([-1.0, -1.0, 0.0], color));
    v.extend_from_slice(&vertex([1.0, -1.0, 0.0], color));
    v.extend_from_slice(&vertex([1.0, 1.0, 0.0], color));
    v.extend_from_slice(&vertex([-1.0, 1.0, 0.0], color));
    (id, v, vec![0, 1, 2, 0, 2, 3])
}

/// A dense n×n ground grid (2n² triangles), all critical coverage.
fn dense_ground(id: u64, n: usize) -> (u64, Vec<f32>, Vec<u32>) {
    let color = [0.2, 0.6, 0.3, 1.0];
    let mut verts = Vec::new();
    (0..=n).for_each(|iy| {
        (0..=n).for_each(|ix| {
            let x = -1.0 + 2.0 * ix as f32 / n as f32;
            let y = -1.0 + 2.0 * iy as f32 / n as f32;
            verts.extend_from_slice(&vertex([x, y, 0.0], color));
        })
    });
    let stride = (n + 1) as u32;
    let mut indices = Vec::new();
    (0..n).for_each(|iy| {
        (0..n).for_each(|ix| {
            let i = iy as u32 * stride + ix as u32;
            indices.extend_from_slice(&[
                i,
                i + 1,
                i + stride,
                i + 1,
                i + 1 + stride,
                i + stride,
            ]);
        })
    });
    (id, verts, indices)
}

/// A tiny triangle near a corner — sub-pixel (≈0.1 px²) and non-critical.
fn tiny(id: u64) -> (u64, Vec<f32>, Vec<u32>) {
    let c = [1.0, 1.0, 1.0, 1.0];
    let d = 0.002;
    let mut v = Vec::new();
    v.extend_from_slice(&vertex([-1.0, -1.0, 0.0], c));
    v.extend_from_slice(&vertex([-1.0 + d, -1.0, 0.0], c));
    v.extend_from_slice(&vertex([-1.0, -1.0 + d, 0.0], c));
    (id, v, vec![0, 1, 2])
}

/// A mesh entirely off the right of the screen (NDC x in [2,3]).
fn offscreen(id: u64) -> (u64, Vec<f32>, Vec<u32>) {
    let c = [1.0, 1.0, 1.0, 1.0];
    let mut v = Vec::new();
    v.extend_from_slice(&vertex([2.0, -1.0, 0.0], c));
    v.extend_from_slice(&vertex([3.0, -1.0, 0.0], c));
    v.extend_from_slice(&vertex([2.0, 1.0, 0.0], c));
    (id, v, vec![0, 1, 2])
}

fn draw(object_id: u64, mesh_id: u64) -> FrameDrawItem {
    FrameDrawItem::new(object_id, mesh_id, 9, IDENTITY, IDENTITY, [1.0; 4], false)
}

fn packet(draws: Vec<FrameDrawItem>) -> FramePacket {
    FramePacket::new(
        3,
        180,
        FrameViewport::new(320, 180),
        [0.4, 0.6, 0.9, 1.0],
        Some(FrameCamera::new(IDENTITY, IDENTITY, IDENTITY)),
        draws,
        Vec::new(),
        IDENTITY,
        FrameFeatureSet::new(false, false, 0, 0),
    )
}

/// A depth-cue profile with every cue disabled — for tests that assert raw
/// (un-shaded) colours and counts.
fn cues_off() -> CanvasDepthCueProfile {
    let mut p = CanvasDepthCueProfile::low_poly_framebuffer();
    p.fog.enabled = false;
    p.lighting.enabled = false;
    p.enable_height_tint = false;
    p.enable_contact_shadows = false;
    p.enable_depth_outlines = false;
    p.enable_distance_detail_falloff = false;
    p.enable_horizon_silhouette = false;
    p.enable_vertical_grade = false;
    p
}

fn opts() -> LowPolyRasterOptions {
    LowPolyRasterOptions::default().with_depth_cues(cues_off())
}

/// Default 320×180 options with the given cue profile.
fn opts_cued(cues: CanvasDepthCueProfile) -> LowPolyRasterOptions {
    LowPolyRasterOptions::new(320, 180, CanvasDebugOverlay::None, 200_000, 8_000_000, cues)
}

#[test]
fn one_mesh_produces_raster_triangles_with_resolved_colour() {
    let cache = MeshCache::load(&[ground(7, [1.0, 1.0, 1.0, 1.0])]);
    let d = FrameDrawItem::new(1, 7, 9, IDENTITY, IDENTITY, [1.0, 0.0, 0.0, 1.0], false);
    let out = convert(&packet(vec![d]), &cache, &opts());
    assert_eq!(out.stats.projected_draws, 1);
    assert_eq!(out.stats.projected_triangles, 2);
    assert_eq!(out.stats.skipped_draws, 0);
    assert_eq!(out.triangles.len(), 2);
    assert_eq!(out.triangles[0].color(), [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(out.triangles[0].object_id(), 1);
    assert_eq!(out.stats.rasterized_objects, 1);
}

#[test]
fn missing_mesh_increments_skipped_draws() {
    let cache = MeshCache::default();
    let out = convert(&packet(vec![draw(1, 404)]), &cache, &opts());
    assert_eq!(out.stats.projected_draws, 0);
    assert_eq!(out.stats.skipped_draws, 1);
    assert_eq!(out.stats.rasterized_objects, 0);
    assert!(out.triangles.is_empty());
}

#[test]
fn invalid_projection_is_culled_and_counted_no_nans() {
    let cache = MeshCache::load(&[ground(7, [1.0, 1.0, 1.0, 1.0])]);
    let zero_mvp = FrameDrawItem::new(1, 7, 9, IDENTITY, [0.0; 16], [1.0; 4], false);
    let out = convert(&packet(vec![zero_mvp]), &cache, &opts());
    assert_eq!(out.stats.projected_draws, 1);
    assert_eq!(out.stats.projected_triangles, 0);
    assert_eq!(out.stats.skipped_invalid_projection_triangles, 2);
    assert!(out.triangles.is_empty());
}

#[test]
fn offscreen_triangle_is_culled_before_raster() {
    let cache = MeshCache::load(&[offscreen(7)]);
    let out = convert(&packet(vec![draw(1, 7)]), &cache, &opts());
    assert_eq!(out.stats.projected_triangles, 1);
    assert_eq!(out.stats.culled_triangles, 1);
    assert!(out.triangles.is_empty());
}

#[test]
fn sub_pixel_non_critical_triangle_is_culled() {
    let cache = MeshCache::load(&[tiny(7)]);
    let out = convert(&packet(vec![draw(1, 7)]), &cache, &opts());
    assert_eq!(out.stats.projected_triangles, 1);
    assert_eq!(
        out.stats.culled_triangles, 1,
        "a genuinely negligible draw (total coverage well below MIN_VISIBLE_COVERAGE) \
         is not rescued — its lone sub-pixel triangle is culled"
    );
    assert!(out.triangles.is_empty());
}

#[test]
fn critical_terrain_is_preserved_and_not_decimated_under_the_cap() {
    let cache = MeshCache::load(&[ground(7, [0.2, 0.6, 0.3, 1.0])]);
    let out = convert(&packet(vec![draw(1, 7)]), &cache, &opts());
    assert_eq!(out.stats.terrain_draws_preserved, 1);
    assert_eq!(out.stats.terrain_triangles_decimated, 0);
    assert_eq!(out.stats.critical_coverage_skipped, 0);
    assert_eq!(out.triangles.len(), 2);
}

#[test]
fn critical_terrain_over_cap_is_decimated_but_preserved() {
    // 10×10 grid = 200 triangles; cap at 50 → keep the 50 largest.
    let cache = MeshCache::load(&[dense_ground(7, 10)]);
    let o = LowPolyRasterOptions::new(
        320,
        180,
        CanvasDebugOverlay::None,
        50,
        8_000_000,
        cues_off(),
    );
    let out = convert(&packet(vec![draw(1, 7)]), &cache, &o);
    assert_eq!(out.stats.terrain_draws_preserved, 1);
    assert_eq!(out.stats.critical_coverage_skipped, 0);
    assert_eq!(out.triangles.len(), 50);
    assert_eq!(out.stats.terrain_triangles_decimated, 150);
}

#[test]
fn decimation_keep_is_all_unless_preserved_and_over_cap() {
    assert_eq!(decimation_keep(false, 10_000, 50), 10_000);
    assert_eq!(decimation_keep(true, 30, 50), 30);
    assert_eq!(decimation_keep(true, 200, 50), 50);
    assert_eq!(decimation_keep(true, 200, 0), 0);
}

#[test]
fn decorative_draw_skipped_once_budget_exhausted_critical_kept() {
    // Tiny budget: the first (critical) draw spends past it, so the later
    // decorative draw is skipped; terrain is never skipped.
    let cache = MeshCache::load(&[ground(7, [0.2, 0.6, 0.3, 1.0]), small_decorative(8)]);
    let o =
        LowPolyRasterOptions::new(320, 180, CanvasDebugOverlay::None, 200_000, 10, cues_off());
    let out = convert(&packet(vec![draw(1, 7), draw(2, 8)]), &cache, &o);
    assert!(out.stats.budget_exhausted, "budget exceeded by terrain");
    assert_eq!(out.stats.skipped_decorative_draws, 1);
    assert_eq!(out.stats.terrain_draws_preserved, 1);
    assert_eq!(out.stats.critical_coverage_skipped, 0);
    assert!(!out.triangles.is_empty());
    assert!(out.triangles.iter().all(|t| t.object_id() == 1));
}

/// A small decorative quad: visible, several px² per triangle (so it is not
/// sub-pixel-culled), but its total coverage fraction is below the gameplay
/// threshold ⇒ Decorative (skippable under budget).
fn small_decorative(id: u64) -> (u64, Vec<f32>, Vec<u32>) {
    let c = [0.9, 0.2, 0.2, 1.0];
    let s = 0.03;
    let mut v = Vec::new();
    v.extend_from_slice(&vertex([-s, -s, 0.0], c));
    v.extend_from_slice(&vertex([s, -s, 0.0], c));
    v.extend_from_slice(&vertex([s, s, 0.0], c));
    v.extend_from_slice(&vertex([-s, s, 0.0], c));
    (id, v, vec![0, 1, 2, 0, 2, 3])
}

#[test]
fn deterministic_for_same_inputs() {
    let cache = MeshCache::load(&[dense_ground(7, 8)]);
    let p = packet(vec![draw(1, 7)]);
    let a = convert(&p, &cache, &opts());
    let b = convert(&p, &cache, &opts());
    assert_eq!(a.triangles, b.triangles);
    assert_eq!(a.stats, b.stats);
}

#[test]
fn decimation_keeps_the_largest_triangles_first() {
    // Two big triangles + many tiny ones, critical, tiny cap → big ones survive.
    let big = [1.0, 1.0, 1.0, 1.0];
    let mut verts = Vec::new();
    verts.extend_from_slice(&vertex([-1.0, -1.0, 0.0], big));
    verts.extend_from_slice(&vertex([1.0, -1.0, 0.0], big));
    verts.extend_from_slice(&vertex([0.0, 1.0, 0.0], big));
    verts.extend_from_slice(&vertex([-1.0, 1.0, 0.0], big));
    verts.extend_from_slice(&vertex([1.0, 1.0, 0.0], big));
    verts.extend_from_slice(&vertex([0.0, -1.0, 0.0], big));
    (0..30).for_each(|k| {
        let o = -1.0 + 0.001 * k as f32;
        verts.extend_from_slice(&vertex([o, -1.0, 0.0], big));
        verts.extend_from_slice(&vertex([o + 0.02, -1.0, 0.0], big));
        verts.extend_from_slice(&vertex([o, -0.98, 0.0], big));
    });
    let mut indices = vec![0, 1, 2, 3, 4, 5];
    (0..30u32).for_each(|k| {
        let i = 6 + k * 3;
        indices.extend_from_slice(&[i, i + 1, i + 2]);
    });
    let cache = MeshCache::load(&[(7, verts, indices)]);
    let o =
        LowPolyRasterOptions::new(320, 180, CanvasDebugOverlay::None, 2, 8_000_000, cues_off());
    let out = convert(&packet(vec![draw(1, 7)]), &cache, &o);
    assert_eq!(out.triangles.len(), 2);
    let kept: f32 = out
        .triangles
        .iter()
        .map(|t| triangle_area(t.vertices()))
        .sum();
    assert!(kept > 1000.0, "kept the big triangles, area {kept}");
}

/// A finely-tessellated small object: an n×n grid over a tiny quad (NDC
/// half-size `s`) so its TOTAL on-screen coverage is visible (≥ MIN_VISIBLE_COVERAGE)
/// but every one of its 2n² triangles is individually sub-pixel (< MIN_TRIANGLE_AREA).
/// This is the screen shape a distant sphere takes — the case the per-triangle
/// sub-pixel cull would erase entirely, and the rescue must preserve.
fn fine_object(id: u64, n: usize, s: f32) -> (u64, Vec<f32>, Vec<u32>) {
    let color = [0.8, 0.4, 0.3, 1.0];
    let mut verts = Vec::new();
    (0..=n).for_each(|iy| {
        (0..=n).for_each(|ix| {
            let x = -s + 2.0 * s * ix as f32 / n as f32;
            let y = -s + 2.0 * s * iy as f32 / n as f32;
            verts.extend_from_slice(&vertex([x, y, 0.0], color));
        })
    });
    let stride = (n + 1) as u32;
    let mut indices = Vec::new();
    (0..n).for_each(|iy| {
        (0..n).for_each(|ix| {
            let i = iy as u32 * stride + ix as u32;
            indices.extend_from_slice(&[i, i + 1, i + stride, i + 1, i + 1 + stride, i + stride]);
        })
    });
    (id, verts, indices)
}

#[test]
fn finely_tessellated_visible_draw_is_rescued_not_emptied() {
    // 6×6 grid over an s=0.014 quad → 72 triangles, total coverage ≈11 px²
    // (visible, ≥ MIN_VISIBLE_COVERAGE), each triangle ≈0.16 px² (< MIN_TRIANGLE_AREA).
    // Classifies Decorative — NOT critical — so the old per-triangle cull would
    // empty it; the rescue keeps every triangle so the object still paints.
    let cache = MeshCache::load(&[fine_object(7, 6, 0.014)]);
    let out = convert(&packet(vec![draw(1, 7)]), &cache, &opts());
    assert_eq!(out.stats.projected_triangles, 72);
    assert_eq!(out.triangles.len(), 72, "rescued: all triangles kept");
    assert_eq!(out.stats.culled_triangles, 0, "nothing sub-pixel-culled");
    assert_eq!(out.stats.terrain_draws_preserved, 0, "rescued, not critical");
    assert!(
        out.triangles
            .iter()
            .all(|t| triangle_area(t.vertices()) < MIN_TRIANGLE_AREA),
        "every kept triangle is individually sub-pixel — kept only by the rescue"
    );
    assert_eq!(out.stats.rasterized_objects, 1);
}

#[test]
fn rescued_over_tessellated_draw_is_decimated_to_cap() {
    // Same finely-tessellated visible draw (72 sub-pixel triangles), but a tiny
    // cap of 8 → the rescue keeps it visible while decimation bounds its cost to
    // the 8 largest triangles (silhouette-preserving), exactly as for terrain.
    let cache = MeshCache::load(&[fine_object(7, 6, 0.014)]);
    let o = LowPolyRasterOptions::new(320, 180, CanvasDebugOverlay::None, 8, 8_000_000, cues_off());
    let out = convert(&packet(vec![draw(1, 7)]), &cache, &o);
    assert_eq!(out.triangles.len(), 8, "rescued draw bounded to the cap");
    assert_eq!(out.stats.terrain_triangles_decimated, 64);
    assert_eq!(out.stats.culled_triangles, 0, "decimation is not culling");
    assert_eq!(out.stats.rasterized_objects, 1);
}

/// A mid-coverage object — between the gameplay and critical fractions, so it
/// classifies as a `GameplayObject` (emits a contact-shadow / outline anchor).
fn gameplay_object(id: u64) -> (u64, Vec<f32>, Vec<u32>) {
    let c = [0.8, 0.3, 0.2, 1.0];
    let s = 0.15;
    let mut v = Vec::new();
    v.extend_from_slice(&vertex([-s, -s, 0.0], c));
    v.extend_from_slice(&vertex([s, -s, 0.0], c));
    v.extend_from_slice(&vertex([s, s, 0.0], c));
    v.extend_from_slice(&vertex([-s, s, 0.0], c));
    (id, v, vec![0, 1, 2, 0, 2, 3])
}

#[test]
fn cue_counters_count_triangles_when_enabled_and_zero_when_off() {
    let cache = MeshCache::load(&[ground(7, [0.2, 0.6, 0.3, 1.0])]);
    let on = convert(
        &packet(vec![draw(1, 7)]),
        &cache,
        &opts_cued(CanvasDepthCueProfile::low_poly_framebuffer()),
    );
    assert_eq!(on.stats.lit_triangles, 2);
    assert_eq!(on.stats.height_tinted_triangles, 2);
    assert_eq!(on.stats.distance_falloff_applied_triangles, 2);
    let off = convert(&packet(vec![draw(1, 7)]), &cache, &opts());
    assert_eq!(off.stats.lit_triangles, 0);
    assert_eq!(off.stats.height_tinted_triangles, 0);
    assert_eq!(off.stats.distance_falloff_applied_triangles, 0);
}

#[test]
fn lighting_changes_the_triangle_colour_versus_base() {
    let cache = MeshCache::load(&[ground(7, [0.6, 0.6, 0.6, 1.0])]);
    let mut lit = CanvasDepthCueProfile::low_poly_framebuffer();
    lit.enable_height_tint = false;
    lit.enable_distance_detail_falloff = false;
    let on = convert(&packet(vec![draw(1, 7)]), &cache, &opts_cued(lit));
    let off = convert(&packet(vec![draw(1, 7)]), &cache, &opts());
    assert_ne!(on.triangles[0].color(), off.triangles[0].color());
    assert_eq!(off.triangles[0].color(), [0.6, 0.6, 0.6, 1.0]);
}

#[test]
fn convert_shades_from_the_scene_directional_light() {
    use axiom_host::FrameLight;
    // A white ground quad (normal +Z) lit by a RED directional light whose
    // to-light direction is +Z (straight at the quad).
    let cache = MeshCache::load(&[ground(7, [1.0, 1.0, 1.0, 1.0])]);
    let mut cues = CanvasDepthCueProfile::low_poly_framebuffer();
    cues.enable_height_tint = false;
    cues.enable_distance_detail_falloff = false;
    cues.lighting.banded = false;
    let red_light = FrameLight::new(0, [0.0, 0.0, 1.0], [1.0, 0.0, 0.0, 1.0]);
    let p = FramePacket::new(
        3,
        180,
        FrameViewport::new(320, 180),
        [0.4, 0.6, 0.9, 1.0],
        Some(FrameCamera::new(IDENTITY, IDENTITY, IDENTITY)),
        vec![draw(1, 7)],
        vec![red_light],
        IDENTITY,
        FrameFeatureSet::new(false, false, 1, 0),
    );
    let out = convert(&p, &cache, &opts_cued(cues));
    // The red light tints only the diffuse term; the hemisphere ambient (sky/
    // ground, untinted by the scene light) still reaches green/blue, so red is
    // the brightest channel and green/blue retain just the ambient floor.
    let c = out.triangles[0].color();
    assert!(c[0] > c[1], "red lit brightest by the red light");
    assert!(c[0] > c[2], "red lit brighter than blue");
    assert!(c[1] > 0.0, "green retains hemisphere ambient");
    assert!(c[2] > 0.0, "blue retains hemisphere ambient");
}

#[test]
fn gameplay_object_emits_one_overlay_terrain_does_not() {
    let p = CanvasDepthCueProfile::low_poly_framebuffer();
    let obj = MeshCache::load(&[gameplay_object(8)]);
    let out = convert(&packet(vec![draw(42, 8)]), &obj, &opts_cued(p));
    assert_eq!(
        out.overlays.len(),
        1,
        "gameplay object emits an overlay anchor"
    );
    assert_eq!(out.overlays[0].object_id, 42);
    let terrain = MeshCache::load(&[ground(7, [0.2, 0.6, 0.3, 1.0])]);
    let t = convert(&packet(vec![draw(1, 7)]), &terrain, &opts_cued(p));
    assert!(t.overlays.is_empty());
}

#[test]
fn deterministic_with_cues_enabled() {
    let cache = MeshCache::load(&[dense_ground(7, 8)]);
    let p = packet(vec![draw(1, 7)]);
    let o = opts_cued(CanvasDepthCueProfile::low_poly_framebuffer());
    let a = convert(&p, &cache, &o);
    let b = convert(&p, &cache, &o);
    assert_eq!(a.triangles, b.triangles);
    assert_eq!(a.stats, b.stats);
}

/// Column-major MVP whose clip `w` (and `z`) equal the vertex's model `z`, so a
/// test can place a vertex at/inside the near plane by choosing a small `z`.
const CW_IS_Z: [f32; 16] = [
    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0,
];

#[test]
fn clip_near_keeps_all_drops_all_and_splits_a_straddle() {
    let cv = |cw: f32| ClipVertex {
        clip: [0.2, 0.3, cw, cw],
        color: [1.0; 4],
    };
    // All in front of the near plane (cw >= W_NEAR): unchanged 3-vertex triangle.
    assert_eq!(clip_near(&[cv(1.0), cv(1.0), cv(1.0)]).len(), 3);
    // All at/behind the near plane: nothing survives.
    assert_eq!(clip_near(&[cv(0.0), cv(0.0), cv(0.0)]).len(), 0);
    // One vertex in front → a 3-vertex front piece (inside vertex + 2 crossings).
    assert_eq!(clip_near(&[cv(1.0), cv(0.0), cv(0.0)]).len(), 3);
    // Two vertices in front → a 4-vertex quad (fan-triangulated to 2 triangles).
    let quad = clip_near(&[cv(1.0), cv(1.0), cv(0.0)]);
    assert_eq!(quad.len(), 4);
    // Every surviving vertex sits at or in front of the near plane, and the two
    // interpolated crossings land exactly on it — `lerp_clip` placed them there.
    assert!(quad.iter().all(|v| v.clip[3] >= W_NEAR - 1e-6));
    assert_eq!(
        quad.iter()
            .filter(|v| (v.clip[3] - W_NEAR).abs() < 1e-6)
            .count(),
        2
    );
}

#[test]
fn straddling_triangle_is_clipped_not_exploded() {
    // Two vertices well in front of the near plane, one just inside it (cw = z
    // via CW_IS_Z). The near-plane clip splits the triangle at the plane into a
    // bounded front piece (a quad → 2 triangles) instead of culling it whole or
    // dividing the near-zero-cw vertex into a huge off-screen coordinate.
    let c = [1.0, 1.0, 1.0, 1.0];
    let mut verts = Vec::new();
    verts.extend_from_slice(&vertex([-0.5, -0.5, 1.0], c));
    verts.extend_from_slice(&vertex([0.5, -0.5, 1.0], c));
    verts.extend_from_slice(&vertex([0.0, 0.5, 0.001], c)); // cw = 0.001 < W_NEAR
    let cache = MeshCache::load(&[(7, verts, vec![0, 1, 2])]);
    let d = FrameDrawItem::new(1, 7, 9, IDENTITY, CW_IS_Z, c, false);
    let out = convert(&packet(vec![d]), &cache, &opts());
    // Clipped (a quad → 2 triangles), not culled as invalid.
    assert_eq!(out.stats.skipped_invalid_projection_triangles, 0);
    assert_eq!(out.stats.projected_triangles, 2);
    assert!(!out.triangles.is_empty());
    // Every surviving coordinate is finite and bounded (the clipped vertices sit
    // at cw >= W_NEAR, not cw ≈ 0).
    let mut coords = out
        .triangles
        .iter()
        .flat_map(|t| t.vertices().iter())
        .flat_map(|v| [v.x(), v.y()]);
    assert!(coords.all(|c| c.is_finite() && c.abs() < 1.0e5));
}
