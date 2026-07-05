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

// A monotonic test clock: each read returns the next integer millisecond. Stateless
// (a free `fn`, so it coerces to the rasterizer's `fn() -> f64` clock) via a
// thread-local counter — only this test touches it, so it starts at 0.
thread_local!(static CLOCK_TICKS: std::cell::Cell<f64> = const { std::cell::Cell::new(0.0) });
fn ticking_clock() -> f64 {
    CLOCK_TICKS.with(|t| {
        let v = t.get();
        t.set(v + 1.0);
        v
    })
}

// A recording phase sink: stashes the `(convert, rasterize, post)` split the
// rasterizer reports, so the test can assert the hooks fired in order.
thread_local!(static RECORDED_PHASES: std::cell::Cell<(f64, f64, f64)> = const { std::cell::Cell::new((0.0, 0.0, 0.0)) });
fn record_phases(convert_ms: f64, rasterize_ms: f64, post_ms: f64) {
    RECORDED_PHASES.with(|r| r.set((convert_ms, rasterize_ms, post_ms)));
}

#[test]
fn phase_clock_and_sink_report_the_convert_rasterize_post_split() {
    let cache = MeshCache::load(&[ground(7, [0.3, 0.7, 0.2, 1.0])]);
    let p = packet(vec![draw(1, 7, [1.0; 4])], [0.0, 0.0, 0.0, 1.0]);
    // Default clock (0.0) + discarding sink: the deterministic native path still
    // rasterizes and reports a zero split to the discard sink.
    let _ = SoftwareRasterizer::new(opts(32, 32)).rasterize_packet(&p, &cache);
    // An injected clock advancing 1 ms per boundary read (0,1,2,3) + a recording
    // sink: the sink receives a 1/1/1 split, proving both hooks feed the phases.
    let _ = SoftwareRasterizer::new(opts(32, 32))
        .with_clock(ticking_clock)
        .with_phase_sink(record_phases)
        .rasterize_packet(&p, &cache);
    assert_eq!(RECORDED_PHASES.with(std::cell::Cell::get), (1.0, 1.0, 1.0));
}

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

/// A column-major toy view_proj with `m[11] = 1`, so a `+z` to-light projects in
/// front of the camera (clip w > 0) — an on-screen sun for the volumetrics pass.
const FRONT_VP: [f32; 16] =
    [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 1.0];

#[test]
fn frame_volumetrics_reach_the_canvas_output() {
    use axiom_host::{FrameLight, FrameVolumetrics};
    // The backend applies the frame's neutral volumetrics to its RGBA: with a
    // directional sun in front, a frame carrying FrameVolumetrics renders
    // differently than the same frame without (god-rays brighten toward the sun).
    let cache = MeshCache::load(&[gameplay_object(42, [1.0, 1.0, 1.0, 1.0])]);
    let cam = Some(FrameCamera::new(IDENTITY, IDENTITY, FRONT_VP));
    let base = FramePacket::new(
        2,
        120,
        FrameViewport::new(48, 48),
        [0.01, 0.01, 0.03, 1.0],
        cam,
        vec![draw(42, 42, [1.0; 4])],
        vec![FrameLight::new(0, [0.0, 0.0, 1.0], [1.0, 1.0, 1.0, 1.0])],
        IDENTITY,
        FrameFeatureSet::new(false, true, 1, 0),
    );
    let lit = base.clone().with_volumetrics(FrameVolumetrics::low_poly());
    let off = SoftwareRasterizer::new(opts_cued(48, 48, cues_off())).rasterize_packet(&base, &cache);
    let on = SoftwareRasterizer::new(opts_cued(48, 48, cues_off())).rasterize_packet(&lit, &cache);
    assert_ne!(off.rgba_bytes(), on.rgba_bytes(), "frame volumetrics reach the canvas output");
}

#[test]
fn capability_profile_gates_the_volumetric_pass() {
    use axiom_host::{BackendCapabilityProfile, FrameLight, FrameVolumetrics, RenderCapability};
    let cache = MeshCache::load(&[gameplay_object(42, [1.0, 1.0, 1.0, 1.0])]);
    let cam = Some(FrameCamera::new(IDENTITY, IDENTITY, FRONT_VP));
    let lit = FramePacket::new(
        2,
        120,
        FrameViewport::new(48, 48),
        [0.01, 0.01, 0.03, 1.0],
        cam,
        vec![draw(42, 42, [1.0; 4])],
        vec![FrameLight::new(0, [0.0, 0.0, 1.0], [1.0, 1.0, 1.0, 1.0])],
        IDENTITY,
        FrameFeatureSet::new(false, true, 1, 0),
    )
    .with_volumetrics(FrameVolumetrics::low_poly());
    // Default profile (all) applies the god-ray pass...
    let with = SoftwareRasterizer::new(opts_cued(48, 48, cues_off())).rasterize_packet(&lit, &cache);
    // ...but a profile WITHOUT Volumetrics skips it (the Canvas 2D fps lever) — even
    // though the frame carries volumetrics, the gated backend never runs the pass.
    let restricted = opts_cued(48, 48, cues_off())
        .with_capability_profile(BackendCapabilityProfile::all().without(RenderCapability::Volumetrics));
    let without = SoftwareRasterizer::new(restricted).rasterize_packet(&lit, &cache);
    assert_ne!(with.rgba_bytes(), without.rgba_bytes(), "capability gate skips the volumetric pass");
}

#[test]
fn capability_profile_gates_the_postprocess_pass() {
    use axiom_host::{BackendCapabilityProfile, FrameLight, FramePostProcess, RenderCapability};
    let cache = MeshCache::load(&[gameplay_object(42, [1.0, 1.0, 1.0, 1.0])]);
    let cam = Some(FrameCamera::new(IDENTITY, IDENTITY, FRONT_VP));
    // A mid-grey frame so the tonemap (exposure + ACES) visibly shifts every pixel.
    let graded = FramePacket::new(
        2,
        120,
        FrameViewport::new(48, 48),
        [0.2, 0.2, 0.2, 1.0],
        cam,
        vec![draw(42, 42, [1.0; 4])],
        vec![FrameLight::new(0, [0.0, 0.0, 1.0], [1.0, 1.0, 1.0, 1.0])],
        IDENTITY,
        FrameFeatureSet::new(false, true, 1, 0),
    )
    .with_postprocess(FramePostProcess::cinematic());
    // Default profile (all) applies the filmic tonemap...
    let with = SoftwareRasterizer::new(opts_cued(48, 48, cues_off())).rasterize_packet(&graded, &cache);
    // ...but a profile WITHOUT PostProcess skips it, so the finished frame is ungraded.
    let restricted = opts_cued(48, 48, cues_off())
        .with_capability_profile(BackendCapabilityProfile::all().without(RenderCapability::PostProcess));
    let without = SoftwareRasterizer::new(restricted).rasterize_packet(&graded, &cache);
    assert_ne!(with.rgba_bytes(), without.rgba_bytes(), "capability gate skips the tonemap pass");
}

#[test]
fn capability_profile_gates_the_retro_32bit_pass() {
    use axiom_host::{BackendCapabilityProfile, FrameLight, FrameRetro32BitProfile, RenderCapability};
    let cache = MeshCache::load(&[gameplay_object(42, [1.0, 1.0, 1.0, 1.0])]);
    let cam = Some(FrameCamera::new(IDENTITY, IDENTITY, FRONT_VP));
    // A smoothly-shaded frame so the retro colour-depth quantize + ordered dither
    // visibly reshapes the finished pixels.
    let retro = FramePacket::new(
        2,
        120,
        FrameViewport::new(48, 48),
        [0.35, 0.22, 0.11, 1.0],
        cam,
        vec![draw(42, 42, [1.0; 4])],
        vec![FrameLight::new(0, [0.0, 0.0, 1.0], [1.0, 1.0, 1.0, 1.0])],
        IDENTITY,
        FrameFeatureSet::new(false, true, 1, 0),
    )
    .with_retro_32bit_profile(FrameRetro32BitProfile::retro_32bit());
    // Default profile (all) applies the retro quantize + dither...
    let with = SoftwareRasterizer::new(opts_cued(48, 48, cues_off())).rasterize_packet(&retro, &cache);
    // ...but a profile WITHOUT Retro32Bit skips it — the same neutral post the GPU
    // backend gates, now gated (and applied) on Canvas 2D too.
    let restricted = opts_cued(48, 48, cues_off())
        .with_capability_profile(BackendCapabilityProfile::all().without(RenderCapability::Retro32Bit));
    let without = SoftwareRasterizer::new(restricted).rasterize_packet(&retro, &cache);
    assert_ne!(with.rgba_bytes(), without.rgba_bytes(), "capability gate skips the retro pass");
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
