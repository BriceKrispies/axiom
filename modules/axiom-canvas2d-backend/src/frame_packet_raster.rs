//! Convert a backend-neutral [`FramePacket`] into screen-space raster triangles,
//! doing all the cheap pre-raster work that keeps the pixel loop small:
//! projection, flat-colour resolution, near-plane culling, **off-screen and
//! sub-pixel culling**, coverage-preserving terrain LOD, and a frame **pixel
//! budget** that degrades by importance.
//!
//! For every [`FrameDrawItem`] this looks the mesh up in the backend's resource
//! table, projects each triangle's vertices through the draw's `mvp` (perspective
//! divide + NDC→framebuffer map, in `projection`), resolves the flat fill colour
//! (mesh vertex colour × draw colour), and emits a [`RasterTriangle`] preserving
//! the object id — but only for triangles that will actually contribute pixels:
//!
//! * **Missing mesh** ⇒ the draw is skipped (`skipped_draws`).
//! * **Invalid projection** (a vertex at/behind the near plane) ⇒ that triangle
//!   is culled (`skipped_invalid_projection_triangles`) — never a NaN pixel.
//! * **Degenerate** (near-zero area) ⇒ culled (`skipped_degenerate_triangles`).
//! * **Off-screen** (bounding box entirely outside the framebuffer) ⇒ culled
//!   (`culled_triangles`).
//! * **Sub-pixel** (area below the minimum) on a **non-critical** draw ⇒ culled
//!   (`culled_triangles`); critical coverage keeps even sub-pixel triangles.
//! * **Critical-coverage terrain over the cap** ⇒ deterministic LOD: keep the
//!   `cap` LARGEST-area triangles, drop the smallest (`terrain_triangles_decimated`).
//!   Coverage is preserved (the dropped ones are sub-pixel) — never holes, never
//!   a skipped terrain draw, so `critical_coverage_skipped` stays zero.
//! * **Pixel budget**: cumulative estimated cost is tracked; once it exceeds the
//!   budget, further **Decorative** draws are skipped (`skipped_decorative_draws`,
//!   `budget_exhausted`). GameplayObject and CriticalCoverage are never skipped.
//!
//! Draw and triangle order is preserved, so the output is deterministic.

use std::collections::HashSet;

use axiom_host::{FrameDrawItem, FramePacket};

use crate::canvas_depth_cue::{
    face_normal_world, height_factor, lighting_brightness, normalize3, shade_triangle, world_y,
};
use crate::canvas_depth_cue_profile::CanvasDepthCueProfile;
use crate::canvas_policy::{classify, CanvasFallbackImportance};
use crate::low_poly_raster_options::LowPolyRasterOptions;
use crate::mesh_cache::{MeshCache, MeshGeometry};
use crate::projection::project_vertex;
use crate::raster_triangle::RasterTriangle;
use crate::raster_vertex::RasterVertex;

/// Signed area below which a triangle is degenerate (zero/near-zero) and dropped.
const AREA_EPS: f32 = 1e-6;
/// Minimum screen area (px²) for a *non-critical* triangle to be worth
/// rasterizing; smaller ones are sub-pixel (invisible) and culled. Critical
/// coverage is exempt (its small triangles are handled by LOD, never lost).
const MIN_TRIANGLE_AREA: f32 = 0.5;

/// Geometry-conversion accounting for one frame. All neutral counts/flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct ConversionStats {
    pub(crate) projected_draws: u32,
    pub(crate) skipped_draws: u32,
    pub(crate) projected_triangles: u32,
    pub(crate) skipped_invalid_projection_triangles: u32,
    pub(crate) skipped_degenerate_triangles: u32,
    pub(crate) culled_triangles: u32,
    pub(crate) terrain_draws_preserved: u32,
    pub(crate) terrain_triangles_decimated: u32,
    pub(crate) critical_coverage_skipped: u32,
    pub(crate) rasterized_objects: u32,
    pub(crate) skipped_decorative_draws: u32,
    pub(crate) budget_exhausted: bool,
    pub(crate) lit_triangles: u32,
    pub(crate) height_tinted_triangles: u32,
    pub(crate) distance_falloff_applied_triangles: u32,
}

/// A gameplay object's screen footprint, derived from its projected triangles —
/// the anchor the rasterizer uses for its contact-shadow blob and silhouette
/// outline post-passes. Only emitted for `GameplayObject`-importance draws.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct DrawOverlay {
    /// Screen bounding box `[minx, miny, maxx, maxy]` (device pixels).
    pub(crate) bbox: [f32; 4],
    /// Mean NDC depth of the object (drives outline alpha; smaller = nearer).
    pub(crate) mean_depth: f32,
    /// The object's stable id.
    pub(crate) object_id: u64,
}

/// The triangles + overlay anchors + stats produced from a packet.
pub(crate) struct ConvertedFrame {
    pub(crate) triangles: Vec<RasterTriangle>,
    pub(crate) overlays: Vec<DrawOverlay>,
    pub(crate) stats: ConversionStats,
}

/// One draw's surviving triangles plus its per-draw accounting.
struct DrawConversion {
    triangles: Vec<RasterTriangle>,
    overlay: Option<DrawOverlay>,
    importance: CanvasFallbackImportance,
    is_critical: bool,
    projected: u32,
    invalid: u32,
    degenerate: u32,
    culled: u32,
    decimated: u32,
    est_cost: u64,
    lit: u32,
    height_tinted: u32,
    falloff: u32,
}

/// A projected triangle plus the geometry the depth cues need: the screen
/// vertices, the fake-lighting brightness, the mean world-space elevation, and
/// the mean depth.
struct Candidate {
    area: f32,
    verts: [RasterVertex; 3],
    brightness: f32,
    world_y: f32,
    mean_depth: f32,
}

/// Single-pass projection accumulator: surviving candidates, the per-draw world-Y
/// extent (for height tint), and the cull counts. One `Vec`, pre-reserved.
struct DrawAcc {
    candidates: Vec<Candidate>,
    y_min: f32,
    y_max: f32,
    projected: u32,
    invalid: u32,
    degenerate: u32,
    offscreen: u32,
}

impl Default for DrawAcc {
    fn default() -> Self {
        DrawAcc {
            candidates: Vec::new(),
            y_min: f32::INFINITY,
            y_max: f32::NEG_INFINITY,
            projected: 0,
            invalid: 0,
            degenerate: 0,
            offscreen: 0,
        }
    }
}

/// The frame's directional lighting input for the per-triangle shading cue: a
/// normalized world to-light direction, a linear-RGB colour, and an intensity.
/// Taken from the frame's real scene light, so the Canvas backend shades from
/// the same sun the GPU path does instead of a fixed fake direction.
struct SceneLight {
    dir: [f32; 3],
    color: [f32; 3],
    intensity: f32,
}

/// The frame's primary directional light (the first `kind == 0` light in the
/// packet) as a [`SceneLight`], or — when the frame authored no directional
/// light — the profile's configured fallback direction at neutral white, unit
/// intensity. Resolved once per frame.
fn scene_light(packet: &FramePacket, cues: &CanvasDepthCueProfile) -> SceneLight {
    packet
        .lights()
        .iter()
        .find(|l| l.kind() == 0)
        .map(|l| {
            let ci = l.color_intensity();
            SceneLight {
                dir: normalize3(l.vec()),
                color: [ci[0], ci[1], ci[2]],
                intensity: ci[3],
            }
        })
        .unwrap_or_else(|| SceneLight {
            dir: normalize3(cues.lighting.direction),
            color: [1.0, 1.0, 1.0],
            intensity: 1.0,
        })
}

/// Project + classify + cull every draw in `packet` against `cache`, applying
/// terrain LOD and the frame pixel budget, into screen-space triangles.
pub(crate) fn convert(
    packet: &FramePacket,
    cache: &MeshCache,
    options: &LowPolyRasterOptions,
) -> ConvertedFrame {
    let w = options.framebuffer_width();
    let h = options.framebuffer_height();
    let screen_px2 = w as f32 * h as f32;
    let cap = options.max_triangles_per_terrain_draw();
    let budget = options.pixel_budget();
    let cues = options.depth_cues();
    // Resolve the real scene directional light once per frame (its direction,
    // colour, and intensity drive the per-triangle shading below).
    let light = scene_light(packet, &cues);

    let (mut frame, _spent) = packet.draws().iter().fold(
        (
            ConvertedFrame {
                triangles: Vec::new(),
                overlays: Vec::new(),
                stats: ConversionStats::default(),
            },
            0_u64,
        ),
        |(mut frame, spent), draw| {
            let drawn = cache
                .get(draw.mesh_id())
                .map(|geo| convert_draw(geo, draw, w, h, screen_px2, cap, &cues, &light));
            frame.stats.skipped_draws += u32::from(drawn.is_none());
            let next = drawn.into_iter().fold(spent, |spent, dc| {
                frame.stats.projected_draws += 1;
                frame.stats.projected_triangles += dc.projected;
                frame.stats.skipped_invalid_projection_triangles += dc.invalid;
                frame.stats.skipped_degenerate_triangles += dc.degenerate;
                frame.stats.culled_triangles += dc.culled;
                frame.stats.terrain_draws_preserved += u32::from(dc.is_critical);
                frame.stats.terrain_triangles_decimated += dc.decimated;
                // Budget: once cumulative cost is over, skip Decorative draws.
                let over = spent > budget;
                let skip = (dc.importance == CanvasFallbackImportance::Decorative) & over;
                frame.stats.budget_exhausted |= over;
                frame.stats.skipped_decorative_draws += u32::from(skip);
                // Cue counts + overlays count only for draws actually kept.
                let keep = !skip;
                frame.stats.lit_triangles += u32::from(keep) * dc.lit;
                frame.stats.height_tinted_triangles += u32::from(keep) * dc.height_tinted;
                frame.stats.distance_falloff_applied_triangles += u32::from(keep) * dc.falloff;
                keep.then(|| dc.overlay.map(|o| frame.overlays.push(o)));
                let kept = keep.then_some(dc.triangles).unwrap_or_default();
                let cost = keep.then_some(dc.est_cost).unwrap_or(0);
                frame.triangles.extend(kept);
                spent + cost
            });
            (frame, next)
        },
    );
    let distinct: HashSet<u64> = frame.triangles.iter().map(|t| t.object_id()).collect();
    frame.stats.rasterized_objects = distinct.len() as u32;
    frame
}

/// Project + cull + decimate + **depth-cue-shade** one draw's mesh into
/// screen-space triangles, and (for gameplay objects) emit a contact-shadow /
/// outline anchor.
#[allow(clippy::too_many_arguments)]
fn convert_draw(
    geo: &MeshGeometry,
    draw: &FrameDrawItem,
    w: u32,
    h: u32,
    screen_px2: f32,
    cap: u32,
    cues: &CanvasDepthCueProfile,
    light: &SceneLight,
) -> DrawConversion {
    let mvp = draw.mvp();
    let world = draw.world();
    let color = draw.color();
    let object_id = draw.object_id();

    // Single pass: project, compute the per-triangle cue inputs (brightness,
    // world-Y, depth), area, and cull (invalid / degenerate / off-screen) into
    // one pre-reserved candidates `Vec`, tracking the draw's world-Y extent.
    let tri_count = geo.indices().len() / 3;
    let mut acc = geo.indices().chunks_exact(3).fold(
        DrawAcc {
            candidates: Vec::with_capacity(tri_count),
            ..DrawAcc::default()
        },
        |mut acc, tri| {
            let proj = project_triangle_cued(
                geo, tri, &mvp, &world, color, object_id, cues, light, w, h,
            );
            acc.invalid += u32::from(proj.is_none());
            proj.into_iter().for_each(|pt| {
                acc.projected += 1;
                let area = triangle_area(&pt.verts);
                let degenerate = area < AREA_EPS;
                let onscr = on_screen(&pt.verts, w, h);
                acc.degenerate += u32::from(degenerate);
                acc.offscreen += u32::from((!degenerate) & !onscr);
                ((!degenerate) & onscr).then(|| {
                    acc.y_min = acc.y_min.min(pt.world_y);
                    acc.y_max = acc.y_max.max(pt.world_y);
                    acc.candidates.push(Candidate {
                        area,
                        verts: pt.verts,
                        brightness: pt.brightness,
                        world_y: pt.world_y,
                        mean_depth: pt.mean_depth,
                    });
                });
            });
            acc
        },
    );

    // Classify by total on-screen coverage.
    let coverage: f32 = acc.candidates.iter().map(|c| c.area).sum();
    let importance = classify(coverage, screen_px2);
    let is_critical = importance == CanvasFallbackImportance::CriticalCoverage;

    // Sub-pixel cull for non-critical draws (in place); critical keeps all.
    let before_min = acc.candidates.len();
    acc.candidates
        .retain(|c| is_critical | (c.area >= MIN_TRIANGLE_AREA));
    let min_culled = (before_min - acc.candidates.len()) as u32;

    // Coverage-preserving LOD: keep the `cap` largest when a critical draw is
    // over the cap (sort + truncate in place; sort only then).
    let pre = acc.candidates.len();
    let keep = decimation_keep(is_critical, pre, cap);
    (pre > keep).then(|| {
        acc.candidates
            .sort_by(|a, b| b.area.partial_cmp(&a.area).unwrap_or(core::cmp::Ordering::Equal))
    });
    acc.candidates.truncate(keep);
    let decimated = (pre - acc.candidates.len()) as u32;
    let est_cost: u64 = acc.candidates.iter().map(|c| c.area as u64).sum();

    // Bake the per-triangle depth cues into each flat colour (lighting → height
    // tint → distance falloff, the documented order).
    let (y_min, y_max) = (acc.y_min, acc.y_max);
    let triangles: Vec<RasterTriangle> = acc
        .candidates
        .iter()
        .map(|c| {
            let base = RasterTriangle::base_color(&c.verts);
            let hf = height_factor(c.world_y, y_min, y_max);
            let (shaded, _applied) =
                shade_triangle(base, c.brightness, light.color, hf, c.mean_depth, cues);
            RasterTriangle::shaded(c.verts, shaded)
        })
        .collect();

    let n = triangles.len() as u32;
    let is_gameplay = importance == CanvasFallbackImportance::GameplayObject;
    let overlay = (is_gameplay & !triangles.is_empty()).then(|| draw_overlay(&triangles, object_id));

    DrawConversion {
        triangles,
        overlay,
        importance,
        is_critical,
        projected: acc.projected,
        invalid: acc.invalid,
        degenerate: acc.degenerate,
        culled: acc.offscreen + min_culled,
        decimated,
        est_cost,
        lit: cues.lighting.enabled.then_some(n).unwrap_or(0),
        height_tinted: cues.enable_height_tint.then_some(n).unwrap_or(0),
        falloff: cues.enable_distance_detail_falloff.then_some(n).unwrap_or(0),
    }
}

/// A projected triangle plus the depth-cue inputs derived during projection.
struct ProjectedTriangle {
    verts: [RasterVertex; 3],
    brightness: f32,
    world_y: f32,
    mean_depth: f32,
}

/// Project one triangle's vertices and gather its depth-cue inputs; `None` if any
/// vertex is at/behind the near plane (the triangle is culled rather than
/// producing NaN pixels). The world-space face normal (for fake lighting) and
/// the world-space mean elevation (for the height tint) come from the model
/// positions read here, so projection pays for them once.
#[allow(clippy::too_many_arguments)]
fn project_triangle_cued(
    geo: &MeshGeometry,
    tri: &[u32],
    mvp: &[f32; 16],
    world: &[f32; 16],
    draw_color: [f32; 4],
    object_id: u64,
    cues: &CanvasDepthCueProfile,
    light: &SceneLight,
    w: u32,
    h: u32,
) -> Option<ProjectedTriangle> {
    let model = [
        geo.position(tri[0]),
        geo.position(tri[1]),
        geo.position(tri[2]),
    ];
    let vertex = |k: usize| {
        project_vertex(mvp, model[k], w, h).map(|p| {
            let mc = geo.color(tri[k]);
            let color = [
                mc[0] * draw_color[0],
                mc[1] * draw_color[1],
                mc[2] * draw_color[2],
                mc[3] * draw_color[3],
            ];
            RasterVertex::new(p[0], p[1], p[2], color, object_id)
        })
    };
    vertex(0).zip(vertex(1)).zip(vertex(2)).map(|((a, b), c)| {
        let verts = [a, b, c];
        let normal = face_normal_world(&model, world);
        let brightness = lighting_brightness(normal, light.dir, light.intensity, cues);
        let mean_model = [
            (model[0][0] + model[1][0] + model[2][0]) / 3.0,
            (model[0][1] + model[1][1] + model[2][1]) / 3.0,
            (model[0][2] + model[1][2] + model[2][2]) / 3.0,
        ];
        ProjectedTriangle {
            verts,
            brightness,
            world_y: world_y(mean_model, world),
            mean_depth: (a.depth() + b.depth() + c.depth()) / 3.0,
        }
    })
}

/// A gameplay object's screen footprint (bbox + mean depth) from its triangles.
fn draw_overlay(triangles: &[RasterTriangle], object_id: u64) -> DrawOverlay {
    let (minx, miny, maxx, maxy, dsum, count) = triangles
        .iter()
        .flat_map(|t| t.vertices().iter())
        .fold(
            (
                f32::INFINITY,
                f32::INFINITY,
                f32::NEG_INFINITY,
                f32::NEG_INFINITY,
                0.0_f32,
                0_u32,
            ),
            |(mnx, mny, mxx, mxy, ds, n), v| {
                (
                    mnx.min(v.x()),
                    mny.min(v.y()),
                    mxx.max(v.x()),
                    mxy.max(v.y()),
                    ds + v.depth(),
                    n + 1,
                )
            },
        );
    DrawOverlay {
        bbox: [minx, miny, maxx, maxy],
        mean_depth: dsum / count.max(1) as f32,
        object_id,
    }
}

/// Whether a triangle's screen bounding box overlaps the framebuffer at all.
fn on_screen(v: &[RasterVertex; 3], w: u32, h: u32) -> bool {
    let xs = [v[0].x(), v[1].x(), v[2].x()];
    let ys = [v[0].y(), v[1].y(), v[2].y()];
    let minx = xs.iter().copied().fold(f32::INFINITY, f32::min);
    let maxx = xs.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let miny = ys.iter().copied().fold(f32::INFINITY, f32::min);
    let maxy = ys.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    (maxx >= 0.0) & (minx < w as f32) & (maxy >= 0.0) & (miny < h as f32)
}

/// How many triangles to keep: all of them, unless the draw is critical coverage
/// and over `cap`, when the `cap` largest-area triangles are kept.
fn decimation_keep(is_critical: bool, total: usize, cap: u32) -> usize {
    let over = is_critical & (total > cap as usize);
    over.then_some(cap as usize).unwrap_or(total)
}

/// Screen-space area (px²) of a projected triangle.
fn triangle_area(v: &[RasterVertex; 3]) -> f32 {
    let cross = (v[1].x() - v[0].x()) * (v[2].y() - v[0].y())
        - (v[2].x() - v[0].x()) * (v[1].y() - v[0].y());
    0.5 * cross.abs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas_policy::CanvasDebugOverlay;
    use axiom_host::{FrameCamera, FrameFeatureSet, FramePacket, FrameViewport};

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
                indices.extend_from_slice(&[i, i + 1, i + stride, i + 1, i + 1 + stride, i + stride]);
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
        assert_eq!(out.stats.culled_triangles, 1, "sub-pixel decorative triangle culled");
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
        let o = LowPolyRasterOptions::new(320, 180, CanvasDebugOverlay::None, 50, 8_000_000, cues_off());
        let out = convert(&packet(vec![draw(1, 7)]), &cache, &o);
        assert_eq!(out.stats.terrain_draws_preserved, 1);
        assert_eq!(out.stats.critical_coverage_skipped, 0);
        assert_eq!(out.triangles.len(), 50);
        assert_eq!(out.stats.terrain_triangles_decimated, 150);
    }

    #[test]
    fn decimation_keep_is_all_unless_critical_and_over_cap() {
        assert_eq!(decimation_keep(false, 10_000, 50), 10_000);
        assert_eq!(decimation_keep(true, 30, 50), 30);
        assert_eq!(decimation_keep(true, 200, 50), 50);
        assert_eq!(decimation_keep(true, 200, 0), 0);
    }

    #[test]
    fn decorative_draw_skipped_once_budget_exhausted_critical_kept() {
        // Tiny budget: the first (critical) ground spends past it, so a later
        // decorative object is skipped — but terrain is never skipped.
        let cache = MeshCache::load(&[ground(7, [0.2, 0.6, 0.3, 1.0]), small_decorative(8)]);
        let o = LowPolyRasterOptions::new(320, 180, CanvasDebugOverlay::None, 200_000, 10, cues_off());
        let out = convert(&packet(vec![draw(1, 7), draw(2, 8)]), &cache, &o);
        assert!(out.stats.budget_exhausted, "budget exceeded by terrain");
        assert_eq!(out.stats.skipped_decorative_draws, 1);
        assert_eq!(out.stats.terrain_draws_preserved, 1);
        assert_eq!(out.stats.critical_coverage_skipped, 0);
        // Terrain still produced triangles; the decorative object did not.
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
        let o = LowPolyRasterOptions::new(320, 180, CanvasDebugOverlay::None, 2, 8_000_000, cues_off());
        let out = convert(&packet(vec![draw(1, 7)]), &cache, &o);
        assert_eq!(out.triangles.len(), 2);
        let kept: f32 = out.triangles.iter().map(|t| triangle_area(t.vertices())).sum();
        assert!(kept > 1000.0, "kept the big triangles, area {kept}");
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
        // Lighting scales the flat colour, so the shaded triangle differs.
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
        // The white surface is tinted by the red light: red survives, G/B vanish.
        let c = out.triangles[0].color();
        assert!(c[0] > 0.0, "red channel lit");
        assert!(c[1].abs() < 1e-6, "green removed by the red light");
        assert!(c[2].abs() < 1e-6, "blue removed by the red light");
    }

    #[test]
    fn gameplay_object_emits_one_overlay_terrain_does_not() {
        let p = CanvasDepthCueProfile::low_poly_framebuffer();
        let obj = MeshCache::load(&[gameplay_object(8)]);
        let out = convert(&packet(vec![draw(42, 8)]), &obj, &opts_cued(p));
        assert_eq!(out.overlays.len(), 1, "gameplay object emits an overlay anchor");
        assert_eq!(out.overlays[0].object_id, 42);
        // Critical terrain emits no contact-shadow/outline anchor.
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
}
