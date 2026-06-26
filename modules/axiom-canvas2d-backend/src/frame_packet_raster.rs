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
//! * **Near-plane straddle** (a vertex at/in front of the camera's near plane) ⇒
//!   the triangle is *clipped* against the near plane in clip space, **before** the
//!   perspective divide, yielding 0–2 screen triangles. This is the structural fix
//!   for the Canvas-only "giant phantom wall": a vertex with a near-zero `cw`
//!   divided by `1/cw` smears across the screen, where the GPU's hardware clip
//!   never does. A triangle entirely at/behind the near plane yields 0 triangles
//!   and is counted `skipped_invalid_projection_triangles` — never a NaN pixel.
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
use crate::projection::{clip_coords, clip_to_screen};
use crate::raster_triangle::RasterTriangle;
use crate::raster_vertex::RasterVertex;

/// Signed area below which a triangle is degenerate (zero/near-zero) and dropped.
const AREA_EPS: f32 = 1e-6;
/// Minimum screen area (px²) for a *non-critical* triangle to be worth
/// rasterizing; smaller ones are sub-pixel (invisible) and culled. Critical
/// coverage is exempt (its small triangles are handled by LOD, never lost).
const MIN_TRIANGLE_AREA: f32 = 0.5;

/// The near-plane clip threshold in homogeneous `w`. A clip-space vertex is kept
/// only where `cw >= W_NEAR`; the rest of its triangle is clipped away at the
/// `cw = W_NEAR` plane. For a standard perspective projection `cw` is the world
/// distance in front of the camera, so this is a near plane ~`W_NEAR` world units
/// out — small enough never to over-clip a real app's near plane (retro FPS's is 0.05),
/// large enough that the surviving `1/cw` divide stays finite and bounded. It is
/// the clip-space analogue of the GPU's hardware near-plane clip, which is why the
/// GPU path never showed the phantom-wall smear this prevents.
const W_NEAR: f32 = 1e-2;

/// A clip-space vertex carrying its resolved flat colour, threaded through the
/// near-plane clip so interpolated (clipped) vertices keep a correct colour.
#[derive(Debug, Clone, Copy)]
struct ClipVertex {
    clip: [f32; 4],
    color: [f32; 4],
}

/// Linear interpolation between two clip-space vertices at parameter `t` — the
/// new vertex an edge contributes where it crosses the near plane.
fn lerp_clip(a: &ClipVertex, b: &ClipVertex, t: f32) -> ClipVertex {
    let mix = |x: f32, y: f32| x + (y - x) * t;
    ClipVertex {
        clip: [
            mix(a.clip[0], b.clip[0]),
            mix(a.clip[1], b.clip[1]),
            mix(a.clip[2], b.clip[2]),
            mix(a.clip[3], b.clip[3]),
        ],
        color: [
            mix(a.color[0], b.color[0]),
            mix(a.color[1], b.color[1]),
            mix(a.color[2], b.color[2]),
            mix(a.color[3], b.color[3]),
        ],
    }
}

/// Clip a triangle against the near plane `cw >= W_NEAR` (Sutherland–Hodgman, one
/// plane), returning the clipped convex polygon as 0, 3, or 4 clip-space vertices.
/// Walking the three edges, each emits its start vertex when that vertex is inside,
/// then the near-plane intersection when the edge crosses — so an all-outside
/// triangle yields nothing, an all-inside one yields its 3 vertices unchanged, and
/// a straddling one yields the 3-or-4-vertex front piece with finite `cw`.
fn clip_near(tri: &[ClipVertex; 3]) -> Vec<ClipVertex> {
    (0..3)
        .flat_map(|i| {
            let cur = tri[i];
            let nxt = tri[(i + 1) % 3];
            let cur_in = cur.clip[3] >= W_NEAR;
            let nxt_in = nxt.clip[3] >= W_NEAR;
            let crosses = cur_in ^ nxt_in;
            // Parameter where this edge meets cw = W_NEAR. Only used when the edge
            // crosses, where the endpoints straddle the plane so the denominator is
            // non-zero; otherwise the value is discarded with the `then`.
            let t = (W_NEAR - cur.clip[3]) / (nxt.clip[3] - cur.clip[3]);
            [
                cur_in.then_some(cur),
                crosses.then(|| lerp_clip(&cur, &nxt, t)),
            ]
            .into_iter()
            .flatten()
        })
        .collect()
}

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
                let kept_opt = keep.then_some(dc.triangles);
                let kept = kept_opt.unwrap_or_default();
                let cost = [0, dc.est_cost][usize::from(keep)];
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
            // 0, 1, or 2 screen triangles after the near-plane clip; empty means the
            // whole triangle was at/behind the near plane (counted invalid).
            let tris =
                project_triangle_cued(geo, tri, &mvp, &world, color, object_id, cues, light, w, h);
            acc.invalid += u32::from(tris.is_empty());
            tris.into_iter().for_each(|pt| {
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
        acc.candidates.sort_by(|a, b| {
            b.area
                .partial_cmp(&a.area)
                .unwrap_or(core::cmp::Ordering::Equal)
        })
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
    let overlay =
        (is_gameplay & !triangles.is_empty()).then(|| draw_overlay(&triangles, object_id));

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
        lit: [0, n][usize::from(cues.lighting.enabled)],
        height_tinted: [0, n][usize::from(cues.enable_height_tint)],
        falloff: [0, n][usize::from(cues.enable_distance_detail_falloff)],
    }
}

/// A projected triangle plus the depth-cue inputs derived during projection.
struct ProjectedTriangle {
    verts: [RasterVertex; 3],
    brightness: f32,
    world_y: f32,
    mean_depth: f32,
}

/// Project one triangle, **near-plane-clipping it in clip space before the
/// perspective divide**, into the 0, 1, or 2 screen-space triangles the clip
/// yields (0 = the whole triangle is at/behind the near plane; 2 = it straddled
/// and the front piece is a quad). Clipping here — not culling whole straddling
/// triangles, and not dividing a near-zero `cw` — is what keeps a wall edge that
/// crosses the camera plane from smearing across the screen. The flat per-triangle
/// cue inputs (face-normal brightness, world elevation) are computed once from the
/// model triangle and shared by every clipped piece.
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
) -> Vec<ProjectedTriangle> {
    let model = [
        geo.position(tri[0]),
        geo.position(tri[1]),
        geo.position(tri[2]),
    ];
    // Resolve each vertex to clip space + its flat colour, then clip the triangle
    // against the near plane (still homogeneous — no divide yet).
    let clip_verts: [ClipVertex; 3] = [0, 1, 2].map(|k| {
        let mc = geo.color(tri[k]);
        ClipVertex {
            clip: clip_coords(mvp, model[k]),
            color: [
                mc[0] * draw_color[0],
                mc[1] * draw_color[1],
                mc[2] * draw_color[2],
                mc[3] * draw_color[3],
            ],
        }
    });
    // Flat per-triangle cue inputs (shared by every clipped piece).
    let normal = face_normal_world(&model, world);
    let brightness = lighting_brightness(normal, light.dir, light.intensity, cues);
    let mean_model = [
        (model[0][0] + model[1][0] + model[2][0]) / 3.0,
        (model[0][1] + model[1][1] + model[2][1]) / 3.0,
        (model[0][2] + model[1][2] + model[2][2]) / 3.0,
    ];
    let elevation = world_y(mean_model, world);

    // Fan-triangulate the clipped convex polygon (0/3/4 verts) from vertex 0, and
    // only now perspective-divide each surviving (cw >= W_NEAR) vertex to screen.
    let poly = clip_near(&clip_verts);
    let fan = 1..poly.len().saturating_sub(1);
    fan.map(|i| {
        let verts = [poly[0], poly[i], poly[i + 1]].map(|cv| {
            let p = clip_to_screen(cv.clip, w, h);
            RasterVertex::new(p[0], p[1], p[2], cv.color, object_id)
        });
        ProjectedTriangle {
            verts,
            brightness,
            world_y: elevation,
            mean_depth: (verts[0].depth() + verts[1].depth() + verts[2].depth()) / 3.0,
        }
    })
    .collect()
}

/// A gameplay object's screen footprint (bbox + mean depth) from its triangles.
fn draw_overlay(triangles: &[RasterTriangle], object_id: u64) -> DrawOverlay {
    let (minx, miny, maxx, maxy, dsum, count) =
        triangles.iter().flat_map(|t| t.vertices().iter()).fold(
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
    [total, cap as usize][usize::from(over)]
}

/// Screen-space area (px²) of a projected triangle.
fn triangle_area(v: &[RasterVertex; 3]) -> f32 {
    let cross = (v[1].x() - v[0].x()) * (v[2].y() - v[0].y())
        - (v[2].x() - v[0].x()) * (v[1].y() - v[0].y());
    0.5 * cross.abs()
}

#[cfg(test)]
mod tests;
