//! The pure software rasterizer: screen-space triangles → an RGBA colour buffer
//! + an f32 z-buffer, with a per-pixel depth test and flat shading.
//!
//! This is the heart of the LowPolyFramebuffer profile. Every draw is projected,
//! culled, and LOD'd in `frame_packet_raster`, then the survivors are rasterized
//! here into a small framebuffer, which the wasm binding blits to the canvas via
//! `putImageData`. Canvas 2D is the blit target, not the renderer.
//!
//! ## Hot-loop design (performance pass)
//! The inner pixel loop is deliberately lean — no per-pixel heap allocation, no
//! closures, no method calls, no division:
//!
//! * the triangle's inverse area is computed **once** (callers guarantee a
//!   non-degenerate triangle — `frame_packet_raster` culls degenerate ones), so
//!   the loop multiplies, never divides;
//! * colour + depth are written **directly into the preallocated `&mut [u8]` /
//!   `&mut [f32]`** by indexed offset;
//! * the conditional depth/colour write is a **branchless index-select**
//!   (`[old, new][pass as usize]`) rather than a branch, so a covered or rejected
//!   fragment costs the same and there is no temporary per pixel.
//!
//! ## Depth convention
//! Depth is barycentric-interpolated NDC z; **smaller = nearer**; the buffer
//! clears to `+∞`. A fragment writes iff it is strictly nearer than the stored
//! depth, so a nearer fragment wins regardless of draw order and equal-depth
//! fragments keep the earlier one — deterministic.
//!
//! Everything here is pure Rust: no `web-sys`, no DOM, no canvas. It runs and is
//! fully covered on native.

use axiom_host::FramePacket;

use crate::canvas_depth_cue::to_byte;
use crate::canvas_policy::CanvasDebugOverlay;
use crate::canvas_post_pass::{apply_fog, apply_outlines, apply_vertical_grade, clamp_axis};
use crate::depth_buffer::DepthBuffer;
use crate::frame_packet_raster::convert;
use crate::low_poly_raster_options::LowPolyRasterOptions;
use crate::mesh_cache::MeshCache;
use crate::planar_shadow::apply_planar_shadows;
use crate::raster_triangle::RasterTriangle;
use crate::sdf_raymarch::apply_sdf_raymarch;
use crate::software_framebuffer::SoftwareFramebuffer;
// The finished-frame result type lives in its own file (file-size budget) but is
// re-exported here so its long-standing `software_rasterizer::SoftwareRasterResult`
// path — used by the backend facade and a draw2d doc link — stays valid.
pub(crate) use crate::software_raster_result::SoftwareRasterResult;

/// Barycentric distance to an edge below which a pixel is "on the edge" (the
/// `TriangleEdges` wireframe overlay).
const EDGE_EPS: f32 = 0.04;

/// Per-pixel / per-triangle counters the hot loop accumulates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct PixelStats {
    rasterized_triangles: u32,
    candidate_pixels: u64,
    depth_tested_pixels: u64,
    depth_written_pixels: u64,
}

/// Per-frame raster context: framebuffer size + overlay selector. Fog is a
/// post-pass (not in the hot loop), so the loop only does integer/compare work
/// and one precomputed colour write per covered pixel.
struct RasterCtx {
    width: u32,
    height: u32,
    overlay_idx: usize,
}

/// The software rasterizer for one frame: a colour buffer, a depth buffer, and
/// the tuning options. Built fresh per frame and consumed by
/// [`SoftwareRasterizer::rasterize_packet`].
#[derive(Debug, Clone)]
pub(crate) struct SoftwareRasterizer {
    framebuffer: SoftwareFramebuffer,
    depth: DepthBuffer,
    options: LowPolyRasterOptions,
}

impl SoftwareRasterizer {
    /// A rasterizer sized to the options' framebuffer.
    pub(crate) fn new(options: LowPolyRasterOptions) -> Self {
        let w = options.framebuffer_width();
        let h = options.framebuffer_height();
        SoftwareRasterizer {
            framebuffer: SoftwareFramebuffer::new(w, h),
            depth: DepthBuffer::new(w, h),
            options,
        }
    }

    /// Clear the buffers, rasterize every surviving triangle from `packet`
    /// (projected + culled + LOD'd against `cache`), apply the optional debug
    /// overlay, and return the finished framebuffer bytes + stats.
    pub(crate) fn rasterize_packet(
        mut self,
        packet: &FramePacket,
        cache: &MeshCache,
    ) -> SoftwareRasterResult {
        let converted = convert(packet, cache, &self.options);
        let clear = packet.clear_color();
        let overlay = self.options.debug_overlay();
        let fb_w = self.framebuffer.width();
        let fb_h = self.framebuffer.height();
        let ctx = RasterCtx {
            width: fb_w,
            height: fb_h,
            overlay_idx: overlay.index(),
        };

        self.framebuffer.clear(clear);
        self.depth.clear_far();

        let mut p = PixelStats::default();
        {
            let rgba = self.framebuffer.rgba_mut();
            let dep = self.depth.slice_mut();
            converted
                .triangles
                .iter()
                .for_each(|t| rasterize_triangle(rgba, dep, &ctx, t, &mut p));
        }

        // SDF raymarch pass: composite the frame's SDF scene over the meshes,
        // depth-tested *and* depth-writing against the same buffer, so the fog /
        // shadow post-passes below still occlude against SDF surfaces.
        sdf_pass(&mut self.framebuffer, &mut self.depth, packet);

        // --- Depth-cue post-passes (the per-pixel + per-object cues; the
        // per-triangle cues — lighting, height tint, falloff — are already baked
        // into each triangle's flat colour during conversion). Documented order:
        // fog (6) → vertical grade (7) → contact shadows + outlines (8). ---
        let cues = self.options.depth_cues();

        // 6. depth fog: mix each pixel toward the fog colour by its final depth.
        let fog_opt = cues
            .fog
            .enabled
            .then(|| apply_fog(&mut self.framebuffer, &self.depth, &cues));
        let fog_px = fog_opt.unwrap_or(0);

        // 7. vertical colour grade: a faint lower-screen darkening anchor.
        let grade_opt = cues
            .enable_vertical_grade
            .then(|| apply_vertical_grade(&mut self.framebuffer, &cues));
        let grade_px = grade_opt.unwrap_or(0);

        // 8a. planar projected contact shadows for marked caster objects:
        // project each caster's geometry along the light onto the ground plane and
        // rasterize it depth-tested against the finished scene (so walls occlude
        // it and it lands on the floor, never on a wall face).
        let shadows_opt = cues.enable_contact_shadows.then(|| {
            apply_planar_shadows(
                &mut self.framebuffer,
                &self.depth,
                packet,
                cache,
                cues.contact_shadow_alpha,
                cues.contact_shadow_depth_bias,
            )
        });
        let (shadows, shadow_px) = shadows_opt.unwrap_or((0, 0));

        // 8b. depth-weighted silhouette outlines for important objects.
        let outlined_opt = cues
            .enable_depth_outlines
            .then(|| apply_outlines(&mut self.framebuffer, &converted.overlays, &cues));
        let (outlined, outline_px) = outlined_opt.unwrap_or((0, 0));

        // The far-horizon silhouette is a documented seam, disabled by default:
        // deriving a clean far-terrain band needs neutral far-band data the
        // FramePacket does not carry (see ARCHITECTURE.md). Its knobs are read so
        // they stay live, but it draws nothing yet.
        let _ = cues.horizon_alpha;
        let _ = cues.enable_horizon_silhouette;
        let horizon: u32 = 0;

        // Debug overlays applied as post-passes over the finished framebuffer.
        (overlay == CanvasDebugOverlay::DepthBuffer)
            .then(|| apply_depth_visualization(&mut self.framebuffer, &self.depth));
        (overlay == CanvasDebugOverlay::Bounds)
            .then(|| apply_bounds_overlay(&mut self.framebuffer, &converted.triangles));

        SoftwareRasterResult {
            width: fb_w,
            height: fb_h,
            rgba: self.framebuffer.into_rgba_bytes(),
            conv: converted.stats,
            rasterized_triangles: p.rasterized_triangles,
            candidate_pixels: p.candidate_pixels,
            depth_tested_pixels: p.depth_tested_pixels,
            depth_written_pixels: p.depth_written_pixels,
            depth_fog_applied_pixels: fog_px,
            vertical_grade_applied_pixels: grade_px,
            contact_shadows_drawn: shadows,
            contact_shadow_pixels: shadow_px,
            outlined_objects: outlined,
            outline_pixels: outline_px,
            horizon_silhouette_drawn: horizon,
        }
    }
}

/// Composite the frame's SDF scene over the already-rasterized meshes,
/// depth-tested and depth-writing against the same buffer. Runs only when the
/// frame carries an SDF scene; the scene is self-contained (it carries its own
/// `view_proj` for the depth projection), so no `FrameCamera` is consulted.
/// Returns the count of composited SDF pixels (`0` when the frame carries no
/// SDF scene).
pub(crate) fn sdf_pass(framebuffer: &mut SoftwareFramebuffer, depth: &mut DepthBuffer, packet: &FramePacket) -> u64 {
    packet
        .sdf()
        .map(|scene| apply_sdf_raymarch(framebuffer, depth, scene, packet.lights()))
        .unwrap_or(0)
}

/// Rasterize one **non-degenerate** triangle into the colour + depth slices,
/// updating `stats`. Pure, branchless, NaN-safe (callers guarantee area ≠ 0).
fn rasterize_triangle(
    rgba: &mut [u8],
    depth: &mut [f32],
    ctx: &RasterCtx,
    tri: &RasterTriangle,
    stats: &mut PixelStats,
) {
    let v = tri.vertices();
    let (x0, y0, z0) = (v[0].x(), v[0].y(), v[0].depth());
    let (x1, y1, z1) = (v[1].x(), v[1].y(), v[1].depth());
    let (x2, y2, z2) = (v[2].x(), v[2].y(), v[2].depth());
    let c = tri.color();
    // Flat colour → bytes ONCE per triangle.
    let base = [to_byte(c[0]), to_byte(c[1]), to_byte(c[2]), to_byte(c[3])];
    // Straight (non-premultiplied) src-over alpha, the SPEC-11 §3.4 translucency
    // fold on the software path: a covered fragment composites the draw colour
    // OVER the existing pixel by the colour's alpha (`out = src·a + dst·(1−a)`),
    // instead of overwriting it. The per-triangle src contribution (`base·a`) and
    // `1−a` are precomputed here; only the `dst·(1−a)` term is per-pixel. The
    // alpha channel composites against full opacity (`255·a`), so a translucent
    // draw over the opaque background stays opaque. An opaque draw (`a == 1`)
    // reduces to the previous straight overwrite exactly, byte-for-byte.
    let a = c[3].clamp(0.0, 1.0);
    let inv = 1.0 - a;
    let src = [
        base[0] as f32 * a,
        base[1] as f32 * a,
        base[2] as f32 * a,
        255.0 * a,
    ];
    let inv_area = 1.0 / edge(x0, y0, x1, y1, x2, y2);
    // Per-pixel (x) and per-row (y) steps of each barycentric l_i = e_i·inv_area.
    let a0 = (y1 - y2) * inv_area;
    let a1 = (y2 - y0) * inv_area;
    let a2 = (y0 - y1) * inv_area;
    let b0 = (x2 - x1) * inv_area;
    let b1 = (x0 - x2) * inv_area;
    let b2 = (x1 - x0) * inv_area;
    let dz_dx = a0 * z0 + a1 * z1 + a2 * z2;
    let (minx, maxx, miny, maxy) = screen_bbox(tri, ctx.width, ctx.height);
    let minxf = minx as f32 + 0.5;
    let w = ctx.width as usize;

    stats.rasterized_triangles += 1;
    let mut cand = 0_u64;
    let mut tested = 0_u64;
    let mut written = 0_u64;

    // Barycentrics at the top row's leftmost pixel; stepped down one row at a
    // time (no per-row edge re-evaluation).
    let fy0 = miny as f32 + 0.5;
    let mut r0 = edge(x1, y1, x2, y2, minxf, fy0) * inv_area;
    let mut r1 = edge(x2, y2, x0, y0, minxf, fy0) * inv_area;
    let mut r2 = edge(x0, y0, x1, y1, minxf, fy0) * inv_area;

    (miny..maxy + 1).for_each(|py| {
        // The x-span this row actually covers (the scanline win: iterate the
        // span, not the whole bounding box — thin slivers stop being waste).
        let (sx, ex) = row_span((r0, r1, r2), (a0, a1, a2), minx, maxx);

        let row = py as usize * w;
        let step = (sx - minx) as f32;
        let mut l0 = r0 + a0 * step;
        let mut l1 = r1 + a1 * step;
        let mut l2 = r2 + a2 * step;
        let mut dep = l0 * z0 + l1 * z1 + l2 * z2;

        (sx..ex).for_each(|px| {
            let inside = (l0 >= 0.0) & (l1 >= 0.0) & (l2 >= 0.0);
            let idx = row + px as usize;
            let cur = depth[idx];
            let pass = inside & (dep < cur);
            // Branchless depth write: keep the old value when the test fails.
            depth[idx] = [cur, dep][pass as usize];

            // Colour write honours the overlay paint mask; the depth write does
            // not (so occlusion stays correct under the wireframe overlay).
            let edge_pixel = l0.min(l1).min(l2) < EDGE_EPS;
            let mask = [true, edge_pixel, true, true][ctx.overlay_idx];
            let wi = (pass & mask) as usize;
            let off = idx * 4;
            // src-over composite each channel against the current pixel; the
            // `[old, blended][wi]` select keeps the depth/overlay masking exact.
            let blended = [
                (src[0] + rgba[off] as f32 * inv + 0.5) as u8,
                (src[1] + rgba[off + 1] as f32 * inv + 0.5) as u8,
                (src[2] + rgba[off + 2] as f32 * inv + 0.5) as u8,
                (src[3] + rgba[off + 3] as f32 * inv + 0.5) as u8,
            ];
            rgba[off] = [rgba[off], blended[0]][wi];
            rgba[off + 1] = [rgba[off + 1], blended[1]][wi];
            rgba[off + 2] = [rgba[off + 2], blended[2]][wi];
            rgba[off + 3] = [rgba[off + 3], blended[3]][wi];

            cand += 1;
            tested += u64::from(inside);
            written += u64::from(pass);
            // Step the barycentrics + depth one pixel right (no per-pixel eval).
            l0 += a0;
            l1 += a1;
            l2 += a2;
            dep += dz_dx;
        });
        r0 += b0;
        r1 += b1;
        r2 += b2;
    });

    stats.candidate_pixels += cand;
    stats.depth_tested_pixels += tested;
    stats.depth_written_pixels += written;
}

/// The covered x-span `[start, end_exclusive)` for one scanline, from the
/// barycentrics at the row's leftmost pixel (`l*_0`) and their per-pixel x-steps
/// (`a*`). Each edge with a positive x-step bounds the span from the left, a
/// negative one from the right, a zero one (horizontal edge) makes the whole row
/// empty when it is on the outside. The span is widened by one pixel and clamped
/// to the bounding box; the inner loop's exact inside-test handles the boundary.
fn row_span(l: (f32, f32, f32), a: (f32, f32, f32), minx: u32, maxx: u32) -> (u32, u32) {
    let mf = minx as f32;
    // x where each l_i crosses 0: mf - l_i0/a_i (one divide per edge; the value
    // is garbage when a_i == 0 but is then never selected).
    let xz0 = mf - l.0 / a.0;
    let xz1 = mf - l.1 / a.1;
    let xz2 = mf - l.2 / a.2;
    let left = |xz: f32, ai: f32| [f32::NEG_INFINITY, xz][(ai > 0.0) as usize];
    let right = |xz: f32, ai: f32| [f32::INFINITY, xz][(ai < 0.0) as usize];
    let xl = left(xz0, a.0).max(left(xz1, a.1)).max(left(xz2, a.2));
    let xr = right(xz0, a.0).min(right(xz1, a.1)).min(right(xz2, a.2));
    // A horizontal edge on the outside (a==0, l<0) empties the whole row.
    let h_empty =
        ((a.0 == 0.0) & (l.0 < 0.0)) | ((a.1 == 0.0) & (l.1 < 0.0)) | ((a.2 == 0.0) & (l.2 < 0.0));
    let lo = minx as i64;
    let hi = maxx as i64;
    let s = ((xl.floor() as i64) - 1).clamp(lo, hi) as u32;
    let e = ((xr.ceil() as i64) + 1).clamp(lo, hi) as u32;
    // Exclusive end; an empty row (horizontal-outside or no overlap) → s..s.
    let empty = h_empty | (s > e);
    (s, [e + 1, s][empty as usize])
}

/// The edge function: twice the signed area of triangle `(a, b, p)`.
fn edge(ax: f32, ay: f32, bx: f32, by: f32, px: f32, py: f32) -> f32 {
    (bx - ax) * (py - ay) - (by - ay) * (px - ax)
}

/// The triangle's clamped integer screen bounding box `(minx, maxx, miny, maxy)`.
fn screen_bbox(tri: &RasterTriangle, w: u32, h: u32) -> (u32, u32, u32, u32) {
    let v = tri.vertices();
    let xs = [v[0].x(), v[1].x(), v[2].x()];
    let ys = [v[0].y(), v[1].y(), v[2].y()];
    let minx = clamp_axis(xs.iter().copied().fold(f32::INFINITY, f32::min).floor(), w);
    let maxx = clamp_axis(
        xs.iter().copied().fold(f32::NEG_INFINITY, f32::max).ceil(),
        w,
    );
    let miny = clamp_axis(ys.iter().copied().fold(f32::INFINITY, f32::min).floor(), h);
    let maxy = clamp_axis(
        ys.iter().copied().fold(f32::NEG_INFINITY, f32::max).ceil(),
        h,
    );
    (minx, maxx, miny, maxy)
}

/// `DepthBuffer` overlay: paint the colour buffer as grayscale depth (nearer =
/// brighter; the far/background value = black).
fn apply_depth_visualization(fb: &mut SoftwareFramebuffer, depth: &DepthBuffer) {
    (0..fb.height()).for_each(|y| {
        (0..fb.width()).for_each(|x| {
            let d = depth.depth_at(x, y);
            let g = (1.0 - (d.clamp(-1.0, 1.0) + 1.0) * 0.5).clamp(0.0, 1.0);
            fb.set_pixel(x, y, [g, g, g, 1.0]);
        })
    });
}

/// `Bounds` overlay: stroke each triangle's screen bounding-box border (white).
fn apply_bounds_overlay(fb: &mut SoftwareFramebuffer, triangles: &[RasterTriangle]) {
    let white = [1.0, 1.0, 1.0, 1.0];
    triangles.iter().for_each(|t| {
        let (minx, maxx, miny, maxy) = screen_bbox(t, fb.width(), fb.height());
        (minx..=maxx).for_each(|x| {
            fb.set_pixel(x, miny, white);
            fb.set_pixel(x, maxy, white);
        });
        (miny..=maxy).for_each(|y| {
            fb.set_pixel(minx, y, white);
            fb.set_pixel(maxx, y, white);
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas_depth_cue_profile::CanvasDepthCueProfile;
    use crate::raster_vertex::RasterVertex;
    use axiom_host::{FrameCamera, FrameDrawItem, FrameFeatureSet, FrameViewport};

    const IDENTITY: [f32; 16] = [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ];

    /// One RGBA pixel out of a finished framebuffer's bytes.
    fn px(bytes: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * w + x) * 4) as usize;
        [bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]
    }

    /// A flat-depth screen triangle from three points (flat fill == its colour).
    fn tri(points: [[f32; 2]; 3], depth: f32, color: [f32; 4], oid: u64) -> RasterTriangle {
        let verts = [
            RasterVertex::new(points[0][0], points[0][1], depth, color, oid),
            RasterVertex::new(points[1][0], points[1][1], depth, color, oid),
            RasterVertex::new(points[2][0], points[2][1], depth, color, oid),
        ];
        RasterTriangle::shaded(verts, color)
    }

    fn ctx(w: u32, h: u32) -> RasterCtx {
        RasterCtx {
            width: w,
            height: h,
            overlay_idx: 0,
        }
    }

    /// Rasterize one triangle into fresh buffers; return (rgba bytes, stats).
    fn rasterize_one(t: &RasterTriangle, c: &RasterCtx) -> (Vec<u8>, PixelStats) {
        let mut fb = SoftwareFramebuffer::new(c.width, c.height);
        let mut depth = DepthBuffer::new(c.width, c.height);
        fb.clear([0.0, 0.0, 0.0, 1.0]);
        depth.clear_far();
        let mut stats = PixelStats::default();
        {
            let rgba = fb.rgba_mut();
            let dep = depth.slice_mut();
            rasterize_triangle(rgba, dep, c, t, &mut stats);
        }
        (fb.into_rgba_bytes(), stats)
    }

    #[test]
    fn simple_triangle_fills_expected_pixels_and_counts_candidates() {
        let t = tri(
            [[1.0, 1.0], [9.0, 1.0], [1.0, 9.0]],
            0.5,
            [1.0, 0.0, 0.0, 1.0],
            7,
        );
        let (bytes, stats) = rasterize_one(&t, &ctx(10, 10));
        assert_eq!(px(&bytes, 10, 2, 2), [255, 0, 0, 255]);
        assert_eq!(px(&bytes, 10, 8, 8), [0, 0, 0, 255]);
        assert_eq!(stats.rasterized_triangles, 1);
        assert!(stats.depth_written_pixels > 0);
        // Every bbox pixel is a candidate; the triangle's bbox is ~9×9.
        assert!(stats.candidate_pixels >= stats.depth_tested_pixels);
        assert!(stats.depth_tested_pixels >= stats.depth_written_pixels);
    }

    #[test]
    fn golden_small_triangle_matches_hand_computed_pixels() {
        // A unit-ish right triangle covering pixel centres (0.5,0.5) and (1.5,0.5).
        let t = tri(
            [[0.0, 0.0], [3.0, 0.0], [0.0, 3.0]],
            0.5,
            [0.0, 1.0, 0.0, 1.0],
            1,
        );
        let (bytes, _) = rasterize_one(&t, &ctx(4, 4));
        // (0,0): bary inside → green. (3,3): outside → black.
        assert_eq!(px(&bytes, 4, 0, 0), [0, 255, 0, 255]);
        assert_eq!(px(&bytes, 4, 3, 3), [0, 0, 0, 255]);
        // Deterministic.
        let (again, _) = rasterize_one(&t, &ctx(4, 4));
        assert_eq!(bytes, again);
    }

    #[test]
    fn z_buffer_nearer_wins_regardless_of_draw_order() {
        let pts = [[0.0, 0.0], [10.0, 0.0], [0.0, 10.0]];
        let near = tri(pts, 0.1, [1.0, 0.0, 0.0, 1.0], 1);
        let far = tri(pts, 0.9, [0.0, 0.0, 1.0, 1.0], 2);
        let c = ctx(10, 10);

        let mut fb = SoftwareFramebuffer::new(10, 10);
        let mut depth = DepthBuffer::new(10, 10);
        fb.clear([0.0, 0.0, 0.0, 1.0]);
        depth.clear_far();
        let mut s = PixelStats::default();
        {
            let (rgba, dep) = (fb.rgba_mut(), depth.slice_mut());
            rasterize_triangle(rgba, dep, &c, &far, &mut s);
            rasterize_triangle(rgba, dep, &c, &near, &mut s);
        }
        assert_eq!(
            px(&fb.into_rgba_bytes(), 10, 1, 1),
            [255, 0, 0, 255],
            "near wins"
        );

        let mut fb2 = SoftwareFramebuffer::new(10, 10);
        let mut depth2 = DepthBuffer::new(10, 10);
        fb2.clear([0.0, 0.0, 0.0, 1.0]);
        depth2.clear_far();
        let mut s2 = PixelStats::default();
        {
            let (rgba, dep) = (fb2.rgba_mut(), depth2.slice_mut());
            rasterize_triangle(rgba, dep, &c, &near, &mut s2);
            rasterize_triangle(rgba, dep, &c, &far, &mut s2);
        }
        assert_eq!(
            px(&fb2.into_rgba_bytes(), 10, 1, 1),
            [255, 0, 0, 255],
            "near still wins"
        );
    }

    #[test]
    fn translucent_triangle_composites_over_existing_pixels() {
        // An opaque white triangle (far), then a half-opacity red triangle (near)
        // over it: the covered pixel is a 50/50 src-over blend (red OVER white),
        // not a pure-red overwrite — and the result stays opaque.
        let pts = [[0.0, 0.0], [10.0, 0.0], [0.0, 10.0]];
        let white = tri(pts, 0.9, [1.0, 1.0, 1.0, 1.0], 1);
        let glass_red = tri(pts, 0.1, [1.0, 0.0, 0.0, 0.5], 2);
        let c = ctx(10, 10);
        let mut fb = SoftwareFramebuffer::new(10, 10);
        let mut depth = DepthBuffer::new(10, 10);
        fb.clear([0.0, 0.0, 0.0, 1.0]);
        depth.clear_far();
        let mut s = PixelStats::default();
        {
            let (rgba, dep) = (fb.rgba_mut(), depth.slice_mut());
            rasterize_triangle(rgba, dep, &c, &white, &mut s);
            rasterize_triangle(rgba, dep, &c, &glass_red, &mut s);
        }
        let p = px(&fb.into_rgba_bytes(), 10, 1, 1);
        // 0.5·red(255,0,0) + 0.5·white(255,255,255) ≈ (255, 128, 128), opaque.
        assert_eq!(p[0], 255, "red full {p:?}");
        assert!((120..=135).contains(&p[1]), "green mid {p:?}");
        assert!((120..=135).contains(&p[2]), "blue mid {p:?}");
        assert_eq!(p[3], 255, "result stays opaque {p:?}");
        // A pure overwrite would have been [255,0,0,255]; the blend differs.
        assert_ne!(p, [255, 0, 0, 255]);
    }

    #[test]
    fn fully_transparent_triangle_leaves_the_pixel_unchanged() {
        // alpha 0 → src·0 + dst·1 = dst: the covered pixel keeps the background,
        // even though the fragment passes the depth test.
        let pts = [[0.0, 0.0], [10.0, 0.0], [0.0, 10.0]];
        let ghost = tri(pts, 0.5, [1.0, 0.0, 0.0, 0.0], 1);
        let c = ctx(10, 10);
        let mut fb = SoftwareFramebuffer::new(10, 10);
        let mut depth = DepthBuffer::new(10, 10);
        fb.clear([0.0, 0.0, 1.0, 1.0]);
        depth.clear_far();
        let mut s = PixelStats::default();
        {
            let (rgba, dep) = (fb.rgba_mut(), depth.slice_mut());
            rasterize_triangle(rgba, dep, &c, &ghost, &mut s);
        }
        // The pixel kept the blue clear colour; the depth test still ran/wrote.
        assert_eq!(px(&fb.into_rgba_bytes(), 10, 1, 1), [0, 0, 255, 255]);
        assert!(s.depth_written_pixels > 0);
    }

    #[test]
    fn equal_depth_keeps_the_earlier_fragment() {
        let pts = [[0.0, 0.0], [10.0, 0.0], [0.0, 10.0]];
        let first = tri(pts, 0.5, [1.0, 0.0, 0.0, 1.0], 1);
        let second = tri(pts, 0.5, [0.0, 1.0, 0.0, 1.0], 2);
        let c = ctx(10, 10);
        let mut fb = SoftwareFramebuffer::new(10, 10);
        let mut depth = DepthBuffer::new(10, 10);
        depth.clear_far();
        let mut s = PixelStats::default();
        {
            let (rgba, dep) = (fb.rgba_mut(), depth.slice_mut());
            rasterize_triangle(rgba, dep, &c, &first, &mut s);
            rasterize_triangle(rgba, dep, &c, &second, &mut s);
        }
        assert_eq!(px(&fb.into_rgba_bytes(), 10, 1, 1), [255, 0, 0, 255]);
    }

    #[test]
    fn enabling_fog_in_options_runs_the_post_pass() {
        // A ground at NDC z=0 with fog from -1..1 (f at z=0 is 0.5) → half-fogged.
        let cache = MeshCache::load(&[ground(7, [1.0, 0.0, 0.0, 1.0])]);
        let mut c = cues_off();
        c.fog.enabled = true;
        c.fog.near = -1.0;
        c.fog.far = 1.0;
        c.fog.strength = 1.0;
        c.fog.color = [0.0, 0.0, 1.0, 1.0];
        let result = SoftwareRasterizer::new(opts_cued(16, 16, c)).rasterize_packet(
            &packet(vec![draw(1, 7, [1.0; 4])], [0.0, 0.0, 1.0, 1.0]),
            &cache,
        );
        // Centre pixel: red mixed halfway toward blue fog → ~[128,0,128].
        let p = px(result.rgba_bytes(), 16, 8, 8);
        assert!(p[0] > 100, "red dimmed by fog");
        assert!(p[0] < 160, "red dimmed by fog");
        assert!(p[2] > 100, "blue raised by fog");
        assert!(p[2] < 160, "blue raised by fog");
        assert!(result.depth_fog_applied_pixels() > 0);
    }

    // ---- rasterize_packet (end-to-end) ----

    fn vertex(pos: [f32; 3], color: [f32; 4]) -> [f32; 12] {
        [
            pos[0], pos[1], pos[2], 0.0, 1.0, 0.0, 0.0, 0.0, color[0], color[1], color[2], color[3],
        ]
    }

    fn ground(id: u64, color: [f32; 4]) -> (u64, Vec<f32>, Vec<u32>) {
        let mut v = Vec::new();
        v.extend_from_slice(&vertex([-1.0, -1.0, 0.0], color));
        v.extend_from_slice(&vertex([1.0, -1.0, 0.0], color));
        v.extend_from_slice(&vertex([1.0, 1.0, 0.0], color));
        v.extend_from_slice(&vertex([-1.0, 1.0, 0.0], color));
        (id, v, vec![0, 1, 2, 0, 2, 3])
    }

    fn packet(draws: Vec<FrameDrawItem>, clear: [f32; 4]) -> FramePacket {
        FramePacket::new(
            2,
            120,
            FrameViewport::new(320, 180),
            clear,
            Some(FrameCamera::new(IDENTITY, IDENTITY, IDENTITY)),
            draws,
            Vec::new(),
            IDENTITY,
            FrameFeatureSet::new(false, false, 0, 0),
        )
    }

    fn draw(object_id: u64, mesh_id: u64, color: [f32; 4]) -> FrameDrawItem {
        FrameDrawItem::new(object_id, mesh_id, 9, IDENTITY, IDENTITY, color, false)
    }

    /// A mid-coverage gameplay object (emits a contact-shadow / outline anchor).
    fn gameplay_object(id: u64, color: [f32; 4]) -> (u64, Vec<f32>, Vec<u32>) {
        let s = 0.15;
        let mut v = Vec::new();
        v.extend_from_slice(&vertex([-s, -s, 0.0], color));
        v.extend_from_slice(&vertex([s, -s, 0.0], color));
        v.extend_from_slice(&vertex([s, s, 0.0], color));
        v.extend_from_slice(&vertex([-s, s, 0.0], color));
        (id, v, vec![0, 1, 2, 0, 2, 3])
    }

    fn cues_on() -> CanvasDepthCueProfile {
        CanvasDepthCueProfile::low_poly_framebuffer()
    }

    /// Every depth cue disabled — for asserting raw (un-shaded) raster output.
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

    fn opts(w: u32, h: u32) -> LowPolyRasterOptions {
        LowPolyRasterOptions::new(
            w,
            h,
            CanvasDebugOverlay::None,
            200_000,
            8_000_000,
            cues_off(),
        )
    }

    /// Options with one cue toggled on (others off), at `w×h`.
    fn opts_cued(w: u32, h: u32, cues: CanvasDepthCueProfile) -> LowPolyRasterOptions {
        LowPolyRasterOptions::new(w, h, CanvasDebugOverlay::None, 200_000, 8_000_000, cues)
    }

    #[test]
    fn packet_rasterizes_and_reports_full_stats() {
        let cache = MeshCache::load(&[ground(7, [1.0, 1.0, 1.0, 1.0])]);
        let result = SoftwareRasterizer::new(opts(64, 36)).rasterize_packet(
            &packet(
                vec![draw(42, 7, [0.2, 0.8, 0.3, 1.0])],
                [0.0, 0.0, 0.0, 1.0],
            ),
            &cache,
        );
        assert_eq!(result.conversion().projected_draws, 1);
        assert_eq!(result.conversion().skipped_draws, 0);
        assert_eq!(result.conversion().projected_triangles, 2);
        assert_eq!(result.rasterized_triangles(), 2);
        assert_eq!(result.conversion().rasterized_objects, 1);
        assert_eq!(result.width(), 64);
        assert_eq!(result.height(), 36);
        assert_eq!(result.rgba_bytes().len(), 64 * 36 * 4);
        assert!(result.candidate_pixels() > 0);
        assert!(result.depth_written_pixels() > 0);
        assert_eq!(
            result.depth_tested_pixels(),
            result.depth_written_pixels() + result.depth_rejected_pixels()
        );
    }

    #[test]
    fn overlapping_grounds_reject_occluded_fragments() {
        let cache = MeshCache::load(&[
            ground(7, [0.4, 0.4, 0.4, 1.0]),
            ground(8, [0.8, 0.1, 0.1, 1.0]),
        ]);
        let result = SoftwareRasterizer::new(opts(40, 40)).rasterize_packet(
            &packet(
                vec![draw(1, 7, [1.0; 4]), draw(2, 8, [1.0; 4])],
                [0.0, 0.0, 0.0, 1.0],
            ),
            &cache,
        );
        assert!(result.depth_rejected_pixels() > 0);
        assert_eq!(result.conversion().rasterized_objects, 2);
    }

    #[test]
    fn invalid_projection_does_not_create_nans() {
        let cache = MeshCache::load(&[ground(7, [1.0; 4])]);
        let d = FrameDrawItem::new(1, 7, 9, IDENTITY, [0.0; 16], [1.0; 4], false);
        let result = SoftwareRasterizer::new(opts(32, 32))
            .rasterize_packet(&packet(vec![d], [0.1, 0.1, 0.1, 1.0]), &cache);
        assert_eq!(result.conversion().skipped_invalid_projection_triangles, 2);
        assert_eq!(result.rasterized_triangles(), 0);
        let clear_byte = (0.1_f32 * 255.0 + 0.5) as u8;
        assert!(result
            .rgba_bytes()
            .chunks_exact(4)
            .all(|p| (p[0] == clear_byte) & (p[3] == 255)));
    }

    #[test]
    fn depth_buffer_overlay_is_grayscale_mid_for_centre_depth() {
        let cache = MeshCache::load(&[ground(7, [0.2, 0.6, 0.3, 1.0])]);
        let o = LowPolyRasterOptions::new(
            32,
            32,
            CanvasDebugOverlay::DepthBuffer,
            200_000,
            8_000_000,
            cues_off(),
        );
        let result = SoftwareRasterizer::new(o).rasterize_packet(
            &packet(vec![draw(1, 7, [1.0; 4])], [0.0, 0.0, 0.0, 1.0]),
            &cache,
        );
        let p = px(result.rgba_bytes(), 32, 16, 16);
        assert_eq!(p[0], p[1]);
        assert_eq!(p[1], p[2]);
        assert!(p[0] > 100, "covered depth pixel mid/bright");
        assert!(p[0] < 160, "covered depth pixel not full white");
    }

    #[test]
    fn triangle_edges_overlay_paints_only_edges() {
        let cache = MeshCache::load(&[ground(7, [0.9, 0.9, 0.9, 1.0])]);
        let o = LowPolyRasterOptions::new(
            40,
            40,
            CanvasDebugOverlay::TriangleEdges,
            200_000,
            8_000_000,
            cues_off(),
        );
        let result = SoftwareRasterizer::new(o).rasterize_packet(
            &packet(vec![draw(1, 7, [1.0; 4])], [0.0, 0.0, 0.0, 1.0]),
            &cache,
        );
        let painted = result
            .rgba_bytes()
            .chunks_exact(4)
            .filter(|p| p[0] > 0)
            .count();
        assert!(painted > 0);
        assert!(painted < 40 * 40 / 2, "interior not filled");
        assert!(result.depth_written_pixels() > painted as u64);
    }

    #[test]
    fn bounds_overlay_strokes_a_border() {
        let cache = MeshCache::load(&[ground(7, [0.2, 0.6, 0.3, 1.0])]);
        let o = LowPolyRasterOptions::new(
            40,
            40,
            CanvasDebugOverlay::Bounds,
            200_000,
            8_000_000,
            cues_off(),
        );
        let result = SoftwareRasterizer::new(o).rasterize_packet(
            &packet(vec![draw(1, 7, [1.0; 4])], [0.0, 0.0, 0.0, 1.0]),
            &cache,
        );
        let white = result
            .rgba_bytes()
            .chunks_exact(4)
            .filter(|p| p == &[255, 255, 255, 255])
            .count();
        assert!(white > 0);
    }

    #[test]
    fn same_packet_is_byte_identical_every_run() {
        let cache = MeshCache::load(&[ground(7, [0.3, 0.7, 0.2, 1.0])]);
        let p = packet(vec![draw(1, 7, [1.0; 4])], [0.0, 0.0, 0.0, 1.0]);
        let a = SoftwareRasterizer::new(opts(48, 48)).rasterize_packet(&p, &cache);
        let b = SoftwareRasterizer::new(opts(48, 48)).rasterize_packet(&p, &cache);
        assert_eq!(a, b);
    }

    // ---- depth-cue post-passes ----

    #[test]
    fn vertical_grade_darkens_lower_screen_more_than_top() {
        let cache = MeshCache::load(&[ground(7, [1.0, 1.0, 1.0, 1.0])]);
        let mut c = cues_off();
        c.enable_vertical_grade = true;
        let result = SoftwareRasterizer::new(opts_cued(8, 16, c)).rasterize_packet(
            &packet(vec![draw(1, 7, [1.0; 4])], [0.0, 0.0, 0.0, 1.0]),
            &cache,
        );
        let top = px(result.rgba_bytes(), 8, 4, 0)[0];
        let bottom = px(result.rgba_bytes(), 8, 4, 15)[0];
        assert!(
            bottom < top,
            "lower screen darker: top {top} bottom {bottom}"
        );
        assert!(result.vertical_grade_applied_pixels() > 0);
    }

    #[test]
    fn marked_caster_gets_a_planar_shadow_unmarked_draw_does_not() {
        use axiom_host::FrameLight;
        let mut c = cues_off();
        c.enable_contact_shadows = true;
        // A flat caster triangle at world y=0.5, a top-down camera (screen = world
        // x,z; depth = world y), and a straight-down directional light — so the
        // projected ground shadow has screen area and a finite depth.
        let topdown = [
            1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ];
        let mut verts = Vec::new();
        verts.extend_from_slice(&vertex([-0.3, 0.5, -0.3], [1.0; 4]));
        verts.extend_from_slice(&vertex([0.3, 0.5, -0.3], [1.0; 4]));
        verts.extend_from_slice(&vertex([0.0, 0.5, 0.3], [1.0; 4]));
        let cache = MeshCache::load(&[(8, verts, vec![0, 1, 2])]);
        let cam = Some(FrameCamera::new(IDENTITY, IDENTITY, topdown));
        let light = vec![FrameLight::new(0, [0.0, 1.0, 0.0], [1.0, 1.0, 1.0, 1.0])];
        let build = |casts: bool| {
            FramePacket::new(
                2,
                120,
                FrameViewport::new(64, 64),
                [0.3, 0.3, 0.3, 1.0],
                cam,
                vec![FrameDrawItem::new(
                    42, 8, 9, IDENTITY, IDENTITY, [1.0; 4], casts,
                )],
                light.clone(),
                IDENTITY,
                FrameFeatureSet::new(false, false, 1, 0),
            )
        };
        // A marked caster casts a planar ground shadow...
        let r =
            SoftwareRasterizer::new(opts_cued(64, 64, c)).rasterize_packet(&build(true), &cache);
        assert_eq!(r.contact_shadows_drawn(), 1);
        assert!(r.contact_shadow_pixels() > 0);
        // ...an unmarked draw (e.g. a wall) casts none.
        let r0 =
            SoftwareRasterizer::new(opts_cued(64, 64, c)).rasterize_packet(&build(false), &cache);
        assert_eq!(r0.contact_shadows_drawn(), 0);
        assert_eq!(r0.contact_shadow_pixels(), 0);
    }

    #[test]
    fn disabling_contact_shadows_draws_none() {
        let obj = MeshCache::load(&[gameplay_object(8, [0.8, 0.3, 0.2, 1.0])]);
        let r = SoftwareRasterizer::new(opts_cued(64, 64, cues_off())).rasterize_packet(
            &packet(vec![draw(42, 8, [1.0; 4])], [0.3, 0.3, 0.3, 1.0]),
            &obj,
        );
        assert_eq!(r.contact_shadows_drawn(), 0);
        assert_eq!(r.contact_shadow_pixels(), 0);
    }

    #[test]
    fn outline_drawn_for_gameplay_object_via_packet() {
        // (The near>far outline-alpha curve is unit-tested in `canvas_post_pass`.)
        let mut oc = cues_off();
        oc.enable_depth_outlines = true;
        let obj = MeshCache::load(&[gameplay_object(8, [0.8, 0.3, 0.2, 1.0])]);
        let r = SoftwareRasterizer::new(opts_cued(64, 64, oc)).rasterize_packet(
            &packet(vec![draw(42, 8, [1.0; 4])], [0.3, 0.3, 0.3, 1.0]),
            &obj,
        );
        assert_eq!(r.outlined_objects(), 1);
        assert!(r.outline_pixels() > 0);
    }

    #[test]
    fn all_cues_change_the_image_but_preserve_draw_and_object_counts() {
        let cache = MeshCache::load(&[ground(7, [0.5, 0.6, 0.4, 1.0])]);
        let p = packet(vec![draw(1, 7, [1.0; 4])], [0.4, 0.6, 0.9, 1.0]);
        let on = SoftwareRasterizer::new(opts_cued(48, 48, cues_on())).rasterize_packet(&p, &cache);
        let off = SoftwareRasterizer::new(opts(48, 48)).rasterize_packet(&p, &cache);
        assert_ne!(
            on.rgba_bytes(),
            off.rgba_bytes(),
            "depth cues change the image"
        );
        // Submitted draw/object counts are unchanged by the cue stage.
        assert_eq!(
            on.conversion().projected_draws,
            off.conversion().projected_draws
        );
        assert_eq!(
            on.conversion().rasterized_objects,
            off.conversion().rasterized_objects
        );
        assert_eq!(on.conversion().critical_coverage_skipped, 0);
        // Lighting/grade are baked/applied regardless of depth, so the image
        // changed even where (near) fog does not reach.
        assert_eq!(on.horizon_silhouette_drawn(), 0);
    }
}
