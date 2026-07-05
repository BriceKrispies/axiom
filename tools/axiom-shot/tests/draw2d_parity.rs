//! **SPEC-04 §7 — "Backend alpha-blend proof" (both backends).**
//!
//! Rasterizes the SAME layer-sorted [`Draw2dList`] through BOTH the software
//! Canvas 2D backend and the GPU off-screen backend, and asserts the resulting
//! pixels match within a tight tolerance. The scene exercises the **full SPEC-04
//! 2D command set**: overlapping semi-transparent rects + a semi-transparent
//! sprite (the alpha-blend core), the round/oriented kinds (circle, rotated
//! ellipse, thick line, particle), a **filled + stroked polygon path** (software
//! even-odd fill vs GPU barycentric-fan), **linear and radial gradient fills**
//! (both sampling the contract's identical baked gradient texture), and a **text
//! glyph run** (each glyph blitted through the sprite path against the font atlas).
//!
//! ## Why they can match byte-for-byte
//! * The GPU off-screen 2D arm renders into a **linear** (`Rgba8Unorm`, non-sRGB)
//!   target, so the GPU's `linear → byte` quantization and its `ALPHA_BLENDING`
//!   (`out = src·a + dst·(1−a)`) match the software backend's linear `over()`
//!   composite and `to_byte` write. (The 3D off-screen path uses sRGB for
//!   display; the 2D path deliberately uses linear for byte parity — see
//!   `draw2d_offscreen`.)
//! * The scene is **integer-pixel-aligned**, so the GPU's pixel-centre coverage
//!   equals the software's `floor..ceil` fill (no half-covered edge pixels), and
//!   the 1:1 axis-aligned sprite samples the same nearest texel on both.
//!
//! ## Tolerance
//! `max channel diff ≤ 2`, `mean channel diff ≤ 1`. The residual is purely a
//! **rounding-convention** difference: the software `to_byte` rounds half **up**
//! (`x·255 + 0.5` truncated) while the GPU's `Rgba8Unorm` quantization rounds half
//! to **even**, so a channel landing exactly on an `n.5` product (e.g. the
//! background's `0.3·255 = 76.5` → software 77, GPU 76) differs by 1 across every
//! pixel it covers. Stacked over four blended layers that compounds to a mean
//! under 1 and a max of 2 — no pixel diverges by more than the two-unit budget,
//! which is the signature of identical math under different tie-breaking, not a
//! real difference. (Choosing only non-`.5` colours would shrink the mean toward
//! 0, but representative colours + an honest tolerance is the better proof.)
//!
//! The path/gradient/text additions hold the **same** ±2 budget, not a loosened
//! one: the path is integer-aligned so software even-odd and GPU barycentric agree
//! at pixel centres (and the stroke's outer band is scanned on both); the gradient
//! fills sample the *same* host-baked ramp/disc with the same nearest rule, fine
//! enough (a 512-texel ramp) that a sub-texel rounding disagreement stays inside
//! ±2; the text run is rendered 1:1 with the integer-aligned atlas cells, so each
//! glyph pixel samples exactly one atlas texel on both backends.
//!
//! Requires the native GPU adapter the sandbox provides (the off-screen arm),
//! so the whole file is compiled only behind the `offscreen` feature.
#![cfg(feature = "offscreen")]

mod common;

use axiom_canvas2d_backend::Canvas2dBackendApi;
use axiom_gpu_backend::GpuBackendApi;
use axiom_host::{
    Common2d, Draw2dCommand, Draw2dList, Fill2d, FontHandle, Glyph2d, GlyphRun, GradientStop, Rect,
    Rgba, SpriteDraw2d, Stroke2d, TextAlign, TextDraw2d, TextureId,
};
use axiom_kernel::{Meters, Radians, Ratio};
use axiom_math::{Mat3, Vec2};

const W: u32 = 64;
const H: u32 = 64;

fn ratio(v: f32) -> Ratio {
    Ratio::new(v).expect("finite")
}

fn rgba(r: f32, g: f32, b: f32, a: f32) -> Rgba {
    Rgba::new(ratio(r), ratio(g), ratio(b), ratio(a))
}

/// A 4×4 opaque RGBA8 atlas with a recognizable per-texel pattern.
fn atlas() -> Vec<(u64, u32, u32, Vec<u8>)> {
    let mut rgba = Vec::with_capacity(4 * 4 * 4);
    (0..4).for_each(|y| {
        (0..4).for_each(|x| {
            let r = (x * 80).min(255) as u8;
            let g = (y * 80).min(255) as u8;
            rgba.extend_from_slice(&[r, g, 200, 255]);
        })
    });
    vec![(7, 4, 4, rgba)]
}

/// A deterministic native font atlas under the reserved `FONT_ATLAS_TEXTURE` id:
/// a `16×16` grid of two `8×16` glyph cells (the layout the runtime's `font.rs`
/// resolver + the browser harness bake), each cell a recognizable per-texel
/// pattern so a glyph is distinguishable and a covered pixel is opaque. The live
/// path bakes glyph bitmaps browser-side; this is the equivalent deterministic
/// atlas so the headless proof can render text — both backends sample these exact
/// bytes, so text is byte-identical across backends.
fn font_atlas() -> Vec<(u64, u32, u32, Vec<u8>)> {
    let id = FontHandle::from_raw(1).atlas_texture().raw();
    let (w, h) = (16u32, 16u32);
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    (0..h).for_each(|y| {
        (0..w).for_each(|x| {
            // Cell 0 = x∈[0,8), cell 1 = x∈[8,16); a per-cell diagonal pattern,
            // fully opaque so a glyph pixel reads its tint.
            let cell = x / 8;
            let lx = x % 8;
            let v = (((lx + y + cell * 3) * 24) % 256) as u8;
            rgba.extend_from_slice(&[v, 255 - v, 128, 255]);
        })
    });
    vec![(id, w, h, rgba)]
}

/// The shared parity scene: an opaque background, an opaque rect, a
/// semi-transparent rect overlapping it, and a semi-transparent sprite — every
/// shape integer-pixel-aligned, submitted out of layer order to also exercise the
/// host `(layer, submission)` sort.
fn scene() -> Draw2dList {
    let mut list = Draw2dList::default();
    let header = |sub: u32, layer: i32, alpha: f32| (sub, Mat3::IDENTITY, Common2d::new(layer, ratio(alpha)));

    // Submit the translucent sprite (layer 3) FIRST to prove the sort.
    list.push_command(Draw2dCommand::sprite(
        (0, Mat3::translation(Vec2::new(16.0, 40.0)), Common2d::new(3, ratio(0.6))),
        TextureId::from_raw(7),
        SpriteDraw2d::new(
            Rect::new(Vec2::ZERO, Vec2::new(4.0, 4.0)),
            Vec2::ZERO,
            rgba(1.0, 1.0, 1.0, 1.0),
            false,
            false,
        ),
    ));
    // Layer 0: opaque background over the whole buffer.
    list.push_command(Draw2dCommand::rect(
        header(1, 0, 1.0),
        Rect::new(Vec2::ZERO, Vec2::new(64.0, 64.0)),
        Fill2d::color(rgba(0.1, 0.1, 0.3, 1.0)),
    ));
    // Layer 1: opaque red block.
    list.push_command(Draw2dCommand::rect(
        header(2, 1, 1.0),
        Rect::new(Vec2::new(8.0, 8.0), Vec2::new(32.0, 32.0)),
        Fill2d::color(rgba(0.9, 0.1, 0.1, 1.0)),
    ));
    // Layer 2: half-alpha green block overlapping the red (the composite proof).
    list.push_command(Draw2dCommand::rect(
        header(3, 2, 0.5),
        Rect::new(Vec2::new(24.0, 24.0), Vec2::new(32.0, 32.0)),
        Fill2d::color(rgba(0.1, 0.9, 0.1, 1.0)),
    ));

    // --- Shape-parity additions (SPEC-04 §7): the round/oriented kinds the GPU
    // arm now rasterizes via analytic per-pixel coverage. Each is OPAQUE on a top
    // layer, so a covered pixel is the shape colour on BOTH backends (an exact
    // overwrite) and only the shared analytic coverage boundary is under test —
    // proving the GPU's conic/capsule discard matches the software per-pixel test.

    // Layer 4: a filled circle (conic coverage; integer centre + radius).
    list.push_command(Draw2dCommand::circle(
        header(4, 4, 1.0),
        Vec2::new(16.0, 18.0),
        Meters::new(6.0).expect("finite"),
        Fill2d::color(rgba(0.9, 0.9, 0.1, 1.0)),
    ));
    // Layer 5: a rotated ellipse (conic coverage with a 45° local rotation).
    list.push_command(Draw2dCommand::ellipse(
        header(5, 5, 1.0),
        Vec2::new(46.0, 18.0),
        Meters::new(9.0).expect("finite"),
        Meters::new(3.0).expect("finite"),
        Radians::new(std::f32::consts::FRAC_PI_4).expect("finite"),
        Fill2d::color(rgba(0.1, 0.85, 0.9, 1.0)),
    ));
    // Layer 6: a thick diagonal line (capsule coverage with round caps).
    list.push_command(Draw2dCommand::line(
        header(6, 6, 1.0),
        Vec2::new(6.0, 54.0),
        Vec2::new(30.0, 60.0),
        rgba(0.95, 0.95, 0.95, 1.0),
        Meters::new(3.0).expect("finite"),
    ));
    // Layer 7: a particle quad (the centred-square `fill_rect` peer).
    list.push_command(Draw2dCommand::particle_quad(
        header(7, 7, 1.0),
        Vec2::new(46.0, 50.0),
        Meters::new(4.0).expect("finite"),
        rgba(0.9, 0.5, 0.1, 1.0),
    ));

    // --- SPEC-04 §7 full-command-set additions: path, gradients, text. Each is
    // OPAQUE on a top layer (an exact overwrite where it lands), so only the
    // shared coverage / sampling is under test. Integer-pixel-aligned so the
    // software even-odd / GPU barycentric polygon coverage agree at pixel centres,
    // and the gradient/glyph nearest sampling picks the same texel on both.

    // Layer 8: a filled + stroked convex polygon (a diamond). Software fills it by
    // even-odd scanline, the GPU by a barycentric fan; both stroke the edges.
    list.push_command(Draw2dCommand::path(
        header(8, 8, 1.0),
        vec![
            Vec2::new(14.0, 38.0),
            Vec2::new(22.0, 46.0),
            Vec2::new(14.0, 54.0),
            Vec2::new(6.0, 46.0),
        ],
        Fill2d::color(rgba(0.9, 0.2, 0.8, 1.0))
            .with_stroke(Stroke2d::new(rgba(1.0, 1.0, 1.0, 1.0), Meters::new(2.0).expect("finite"))),
        true,
    ));

    // Layer 9: a linear-gradient-filled rect (black → white left→right). Both
    // backends sample the same baked ramp at the same affine projection parameter.
    let linear = list.register_linear(
        Vec2::new(34.0, 2.0),
        Vec2::new(58.0, 2.0),
        vec![
            GradientStop::new(ratio(0.0), rgba(0.0, 0.0, 0.0, 1.0)),
            GradientStop::new(ratio(1.0), rgba(1.0, 1.0, 1.0, 1.0)),
        ],
    );
    list.push_command(Draw2dCommand::rect(
        header(9, 9, 1.0),
        Rect::new(Vec2::new(34.0, 2.0), Vec2::new(24.0, 8.0)),
        Fill2d::paint(linear),
    ));

    // Layer 10: a radial-gradient-filled rect (white centre → black edge). Both
    // backends sample the same baked radial disc at the same affine UV.
    let radial = list.register_radial(
        Vec2::new(10.0, 8.0),
        Meters::new(7.0).expect("finite"),
        vec![
            GradientStop::new(ratio(0.0), rgba(1.0, 1.0, 1.0, 1.0)),
            GradientStop::new(ratio(1.0), rgba(0.1, 0.1, 0.1, 1.0)),
        ],
    );
    list.push_command(Draw2dCommand::rect(
        header(10, 10, 1.0),
        Rect::new(Vec2::new(2.0, 0.0), Vec2::new(16.0, 16.0)),
        Fill2d::paint(radial),
    ));

    // Layer 11: a 2-glyph text run against the font atlas, rendered 1:1 with the
    // 8×16 cells and integer-aligned, so each dest pixel samples exactly one atlas
    // texel on both backends. Tinted red; placed at (46, 30) — clear of the other
    // top-layer shapes' sample points.
    list.push_command(Draw2dCommand::text(
        (11, Mat3::translation(Vec2::new(46.0, 30.0)), Common2d::new(11, ratio(1.0))),
        GlyphRun::new(
            vec![
                Glyph2d::new(Rect::new(Vec2::ZERO, Vec2::new(8.0, 16.0)), Meters::new(8.0).expect("finite")),
                Glyph2d::new(Rect::new(Vec2::new(8.0, 0.0), Vec2::new(8.0, 16.0)), Meters::new(8.0).expect("finite")),
            ],
            Meters::new(16.0).expect("finite"),
        ),
        TextDraw2d::new(FontHandle::from_raw(1), rgba(1.0, 0.2, 0.2, 1.0), TextAlign::LEFT),
    ));

    list.sort_commands();
    list
}

/// One RGBA pixel from a buffer.
fn px(b: &[u8], x: u32, y: u32) -> [u8; 4] {
    let i = ((y * W + x) * 4) as usize;
    [b[i], b[i + 1], b[i + 2], b[i + 3]]
}

#[test]
fn gpu_and_software_2d_alpha_blend_match() {
    let list = scene();
    // Sprite atlas (id 7) + the native font atlas (reserved id) the text run samples.
    let textures: Vec<(u64, u32, u32, Vec<u8>)> = atlas().into_iter().chain(font_atlas()).collect();

    // Software backend at the canvas display size.
    let mut sw = Canvas2dBackendApi::new(&common::present_request(W, H));
    sw.load_textures(&textures);
    let (sw_px, sw_w, sw_h) = sw.render_draw2d_rgba(&list);
    assert_eq!((sw_w, sw_h), (W, H));

    // GPU off-screen backend (linear target) at the same size.
    let gpu_px = GpuBackendApi::render_draw2d_offscreen_rgba(W, H, &list, &textures)
        .expect("a native GPU adapter is required for the GPU off-screen 2D arm");
    assert_eq!(gpu_px.len(), sw_px.len());

    // Sanity: the blend actually happened — the green-over-red overlap is a
    // distinct purple-ish mix on BOTH backends (not pure red, not pure green),
    // proving src-over compositing rather than overwrite.
    let overlap_sw = px(&sw_px, 30, 30);
    assert!(overlap_sw[0] > 60 && overlap_sw[1] > 60, "software overlap blended: {overlap_sw:?}");
    assert!(overlap_sw[0] < 220 && overlap_sw[1] < 220, "software overlap is a mix: {overlap_sw:?}");

    // Sanity: each new shape actually rasterized (its interior centre reads the
    // shape colour) on BOTH backends — proving the analytic coverage filled the
    // shape rather than discarding everything.
    let circle_sw = px(&sw_px, 16, 18);
    let circle_gpu = px(&gpu_px, 16, 18);
    assert!(circle_sw[0] > 200 && circle_sw[1] > 200 && circle_sw[2] < 60, "software circle: {circle_sw:?}");
    assert!(circle_gpu[0] > 200 && circle_gpu[1] > 200 && circle_gpu[2] < 60, "gpu circle: {circle_gpu:?}");
    let ellipse_sw = px(&sw_px, 46, 18);
    let ellipse_gpu = px(&gpu_px, 46, 18);
    assert!(ellipse_sw[2] > 200 && ellipse_sw[0] < 60, "software ellipse: {ellipse_sw:?}");
    assert!(ellipse_gpu[2] > 200 && ellipse_gpu[0] < 60, "gpu ellipse: {ellipse_gpu:?}");
    let particle_sw = px(&sw_px, 46, 50);
    let particle_gpu = px(&gpu_px, 46, 50);
    assert!(particle_sw[0] > 200 && particle_sw[2] < 60, "software particle: {particle_sw:?}");
    assert!(particle_gpu[0] > 200 && particle_gpu[2] < 60, "gpu particle: {particle_gpu:?}");
    // The line is thin; sample a point on the segment midline (round(18,57)).
    let line_sw = px(&sw_px, 18, 57);
    let line_gpu = px(&gpu_px, 18, 57);
    assert!(line_sw[0] > 200 && line_sw[1] > 200 && line_sw[2] > 200, "software line: {line_sw:?}");
    assert!(line_gpu[0] > 200 && line_gpu[1] > 200 && line_gpu[2] > 200, "gpu line: {line_gpu:?}");

    // Path fill: the diamond's centre (14,46) is interior → its magenta fill on
    // BOTH backends (proving the even-odd fill and the barycentric fan agree).
    let path_sw = px(&sw_px, 14, 46);
    let path_gpu = px(&gpu_px, 14, 46);
    assert!(path_sw[0] > 200 && path_sw[2] > 180 && path_sw[1] < 80, "software path fill: {path_sw:?}");
    assert!(path_gpu[0] > 200 && path_gpu[2] > 180 && path_gpu[1] < 80, "gpu path fill: {path_gpu:?}");

    // Linear gradient: dark on the left edge of the rect, bright on the right.
    let grad_l_sw = px(&sw_px, 35, 5);
    let grad_r_sw = px(&sw_px, 56, 5);
    assert!(grad_l_sw[0] < 70, "software linear-gradient left is dark: {grad_l_sw:?}");
    assert!(grad_r_sw[0] > 190, "software linear-gradient right is bright: {grad_r_sw:?}");
    assert!(px(&gpu_px, 35, 5)[0] < 70, "gpu linear-gradient left is dark");
    assert!(px(&gpu_px, 56, 5)[0] > 190, "gpu linear-gradient right is bright");

    // Radial gradient: bright near the centre (10,8), darker toward the edge.
    let rad_c_sw = px(&sw_px, 10, 8);
    let rad_e_sw = px(&sw_px, 2, 1);
    assert!(rad_c_sw[0] > 190, "software radial centre is bright: {rad_c_sw:?}");
    assert!(rad_e_sw[0] < 130, "software radial edge is darker: {rad_e_sw:?}");
    assert!(px(&gpu_px, 10, 8)[0] > 190, "gpu radial centre is bright");

    // Text: a glyph pixel inside the first cell (≈(48,38)) reads the red tint on
    // BOTH backends (proving the glyph-run sampled the atlas and placed the glyph).
    let text_sw = px(&sw_px, 48, 38);
    let text_gpu = px(&gpu_px, 48, 38);
    assert!(text_sw[0] > 60 && text_sw[3] == 255, "software text glyph is drawn: {text_sw:?}");
    assert!(text_gpu[3] == 255, "gpu text glyph is drawn: {text_gpu:?}");

    // Tight pixel parity across the whole frame.
    let maxd = common::max_channel_diff(&gpu_px, &sw_px);
    let meand = common::mean_channel_diff(&gpu_px, &sw_px);
    assert!(
        maxd <= 2,
        "max channel diff {maxd} exceeds the ±2 linear-target tolerance (mean {meand:.4})"
    );
    assert!(
        meand <= 1.0,
        "mean channel diff {meand:.4} exceeds 1.0 — more than rounding-convention drift"
    );
}
