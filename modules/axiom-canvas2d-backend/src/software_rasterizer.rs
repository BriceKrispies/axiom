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
// Re-exported here (defined in its own file) so the long-standing
// `software_rasterizer::SoftwareRasterResult` path stays valid for callers.
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

        // Depth-cue post-passes run in a fixed order: fog (6) → vertical grade
        // (7) → contact shadows + outlines (8). Per-triangle cues (lighting,
        // height tint, falloff) are already baked into each flat triangle colour.
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

        // 9. volumetric light scatter (god-rays): the backend-neutral frame effect —
        // `host` applies the frame's `FrameVolumetrics` to the finished RGBA. Gated on
        // the backend's capability profile: skipped when this backend is configured to
        // not attempt `Volumetrics` (the Canvas 2D fps lever), so the god-ray pass runs
        // only when the profile allows it. A no-op anyway when the frame carries none.
        self.options
            .capability_profile()
            .contains(axiom_host::RenderCapability::Volumetrics)
            .then(|| axiom_host::apply_frame_volumetrics(self.framebuffer.rgba_mut(), fb_w, fb_h, packet));

        // The far-horizon silhouette needs neutral far-band data the FramePacket
        // does not carry (see ARCHITECTURE.md); its knobs are read but unused.
        let _ = cues.horizon_alpha;
        let _ = cues.enable_horizon_silhouette;
        let horizon: u32 = 0;

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
    // Straight (non-premultiplied) src-over alpha per SPEC-11 §3.4: a covered
    // fragment composites `out = src·a + dst·(1-a)`. `base·a` and `1-a` are
    // precomputed per triangle; only `dst·(1-a)` is per-pixel.
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
        // The x-span this row actually covers, not the whole bounding box.
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
mod tests;
