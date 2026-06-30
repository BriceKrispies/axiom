//! **SPEC-04 §7 — "Backend alpha-blend proof" (both backends).**
//!
//! Rasterizes the SAME layer-sorted [`Draw2dList`] — a scene of overlapping
//! semi-transparent rects plus a semi-transparent sprite — through BOTH the
//! software Canvas 2D backend and the GPU off-screen backend, and asserts the
//! resulting pixels match within a tight tolerance.
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
//! Requires the native GPU adapter the sandbox provides (the off-screen arm).

mod common;

use axiom_canvas2d_backend::Canvas2dBackendApi;
use axiom_gpu_backend::GpuBackendApi;
use axiom_host::{Common2d, Draw2dCommand, Draw2dList, Fill2d, Rect, Rgba, SpriteDraw2d, TextureId};
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
    let textures = atlas();

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
