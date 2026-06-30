//! Unit tests for the software 2D raster ([`super`]).

use super::*;
use axiom_host::{
    Common2d, Fill2d, FontHandle, Glyph2d, GlyphRun, GradientStop, PaintId, Rgba, Stroke2d,
    TextAlign, TextDraw2d, TextureId,
};
use axiom_kernel::{Meters, Ratio};

fn ratio(v: f32) -> Ratio {
    Ratio::new(v).unwrap()
}

fn rgba(r: f32, g: f32, b: f32, a: f32) -> Rgba {
    Rgba::new(ratio(r), ratio(g), ratio(b), ratio(a))
}

fn header(submission: u32, layer: i32, alpha: f32) -> (u32, Mat3, Common2d) {
    (submission, Mat3::IDENTITY, Common2d::new(layer, ratio(alpha)))
}

/// One RGBA pixel out of a finished buffer's bytes.
fn px(bytes: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * w + x) * 4) as usize;
    [bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]
}

fn rect(min: Vec2, size: Vec2) -> Rect {
    Rect::new(min, size)
}

#[test]
fn rect_fills_its_pixels_over_transparent() {
    let mut list = Draw2dList::default();
    list.push_command(Draw2dCommand::rect(
        header(0, 0, 1.0),
        rect(Vec2::new(2.0, 2.0), Vec2::new(4.0, 4.0)),
        Fill2d::color(rgba(1.0, 0.0, 0.0, 1.0)),
    ));
    list.sort_commands();
    let (bytes, w, h) = render(&list, 8, 8, &Draw2dTextures::default());
    assert_eq!((w, h), (8, 8));
    // Inside the rect (cols/rows 2..6) is opaque red; outside is transparent.
    assert_eq!(px(&bytes, 8, 3, 3), [255, 0, 0, 255]);
    assert_eq!(px(&bytes, 8, 5, 5), [255, 0, 0, 255]);
    assert_eq!(px(&bytes, 8, 6, 6), [0, 0, 0, 0]);
    assert_eq!(px(&bytes, 8, 0, 0), [0, 0, 0, 0]);
}

#[test]
fn layer2_half_alpha_draw_composites_over_layer1_fill() {
    // THE core proof: a layer-2 draw with alpha < 1 over a layer-1 fill
    // COMPOSITES (not overwrites). Submit out of order to also prove the
    // host-sorted list is painted by (layer, submission).
    let mut list = Draw2dList::default();
    let full = rect(Vec2::ZERO, Vec2::new(8.0, 8.0));
    // Submit the translucent blue (layer 2) FIRST, opaque red (layer 1) second.
    list.push_command(Draw2dCommand::rect(
        header(0, 2, 0.5),
        full,
        Fill2d::color(rgba(0.0, 0.0, 1.0, 1.0)),
    ));
    list.push_command(Draw2dCommand::rect(
        header(1, 1, 1.0),
        full,
        Fill2d::color(rgba(1.0, 0.0, 0.0, 1.0)),
    ));
    list.sort_commands();
    let (bytes, _, _) = render(&list, 8, 8, &Draw2dTextures::default());
    // Red painted first (layer 1), then blue·0.5 over it: (0.5,0,0.5).
    assert_eq!(px(&bytes, 8, 4, 4), [128, 0, 128, 255]);
}

#[test]
fn camera_zoom_and_center_place_the_rect() {
    let mut list = Draw2dList::default();
    list.set_camera(axiom_host::Camera2d::new(Vec2::ZERO, ratio(2.0)));
    list.push_command(Draw2dCommand::rect(
        header(0, 0, 1.0),
        rect(Vec2::ZERO, Vec2::ONE),
        Fill2d::color(rgba(0.0, 1.0, 0.0, 1.0)),
    ));
    list.sort_commands();
    // 8×8 buffer, centre (4,4): world (0,0)->(4,4), (1,1)->(6,6) → pixels 4,5.
    let (bytes, _, _) = render(&list, 8, 8, &Draw2dTextures::default());
    assert_eq!(px(&bytes, 8, 4, 4), [0, 255, 0, 255]);
    assert_eq!(px(&bytes, 8, 5, 5), [0, 255, 0, 255]);
    assert_eq!(px(&bytes, 8, 0, 0), [0, 0, 0, 0]);
}

#[test]
fn unknown_paint_fill_composites_nothing() {
    // A paint id naming no registered paint resolves to a transparent fill.
    let mut list = Draw2dList::default();
    list.push_command(Draw2dCommand::rect(
        header(0, 0, 1.0),
        rect(Vec2::ZERO, Vec2::new(8.0, 8.0)),
        Fill2d::paint(PaintId::from_raw(0)),
    ));
    list.sort_commands();
    let (bytes, _, _) = render(&list, 4, 4, &Draw2dTextures::default());
    assert!(bytes.iter().all(|&b| b == 0), "an unknown paint draws nothing");
}

#[test]
fn linear_gradient_fill_shades_across_the_axis() {
    // A black→white horizontal linear gradient over an 8×8 rect: the left edge
    // is near-black, the right near-white, proving per-pixel gradient eval.
    let mut list = Draw2dList::default();
    let lin = list.register_linear(
        Vec2::new(0.0, 0.0),
        Vec2::new(8.0, 0.0),
        vec![
            GradientStop::new(ratio(0.0), rgba(0.0, 0.0, 0.0, 1.0)),
            GradientStop::new(ratio(1.0), rgba(1.0, 1.0, 1.0, 1.0)),
        ],
    );
    list.push_command(Draw2dCommand::rect(
        header(0, 0, 1.0),
        rect(Vec2::ZERO, Vec2::new(8.0, 8.0)),
        Fill2d::paint(lin),
    ));
    list.sort_commands();
    let (bytes, _, _) = render(&list, 8, 8, &Draw2dTextures::default());
    let left = px(&bytes, 8, 0, 4);
    let right = px(&bytes, 8, 7, 4);
    assert!(left[0] < 60, "left edge near-black: {left:?}");
    assert!(right[0] > 200, "right edge near-white: {right:?}");
    assert_eq!(left[3], 255);
}

#[test]
fn radial_gradient_fill_brightens_toward_the_centre() {
    // A white→black radial gradient on a rect centred at (4,4): the centre is
    // near-white, a corner near-black.
    let mut list = Draw2dList::default();
    let rad = list.register_radial(
        Vec2::new(4.0, 4.0),
        Meters::new(4.0).unwrap(),
        vec![
            GradientStop::new(ratio(0.0), rgba(1.0, 1.0, 1.0, 1.0)),
            GradientStop::new(ratio(1.0), rgba(0.0, 0.0, 0.0, 1.0)),
        ],
    );
    list.push_command(Draw2dCommand::rect(
        header(0, 0, 1.0),
        rect(Vec2::ZERO, Vec2::new(8.0, 8.0)),
        Fill2d::paint(rad),
    ));
    list.sort_commands();
    let (bytes, _, _) = render(&list, 8, 8, &Draw2dTextures::default());
    let center = px(&bytes, 8, 4, 4);
    let corner = px(&bytes, 8, 0, 0);
    assert!(center[0] > 200, "centre near-white: {center:?}");
    assert!(corner[0] < 80, "corner near-black: {corner:?}");
}

#[test]
fn path_fills_a_polygon_and_strokes_its_edges() {
    // A closed convex quad (a diamond) filled blue with a white stroke: a point
    // inside fills blue; a point outside the polygon is clear.
    let mut list = Draw2dList::default();
    list.push_command(Draw2dCommand::path(
        header(0, 0, 1.0),
        vec![
            Vec2::new(8.0, 2.0),
            Vec2::new(14.0, 8.0),
            Vec2::new(8.0, 14.0),
            Vec2::new(2.0, 8.0),
        ],
        Fill2d::color(rgba(0.0, 0.0, 1.0, 1.0))
            .with_stroke(Stroke2d::new(rgba(1.0, 1.0, 1.0, 1.0), Meters::new(1.0).unwrap())),
        true,
    ));
    list.sort_commands();
    let (bytes, _, _) = render(&list, 16, 16, &Draw2dTextures::default());
    // Dead centre is inside the diamond → blue fill.
    assert_eq!(px(&bytes, 16, 8, 8), [0, 0, 255, 255]);
    // A far corner is outside the diamond → clear.
    assert_eq!(px(&bytes, 16, 0, 0), [0, 0, 0, 0]);
}

#[test]
fn open_path_strokes_without_filling() {
    // An open L-polyline: no fill (closed=false) anywhere, but the stroke marks
    // the segments. A point "inside" the implied closure stays clear.
    let mut list = Draw2dList::default();
    list.push_command(Draw2dCommand::path(
        header(0, 0, 1.0),
        vec![Vec2::new(2.0, 2.0), Vec2::new(2.0, 12.0), Vec2::new(12.0, 12.0)],
        Fill2d::color(rgba(1.0, 0.0, 0.0, 1.0))
            .with_stroke(Stroke2d::new(rgba(0.0, 1.0, 0.0, 1.0), Meters::new(2.0).unwrap())),
        false,
    ));
    list.sort_commands();
    let (bytes, _, _) = render(&list, 16, 16, &Draw2dTextures::default());
    // On the vertical segment near (2,7): green stroke.
    assert_eq!(px(&bytes, 16, 2, 7), [0, 255, 0, 255]);
    // Interior of the implied triangle (not on a segment): no fill, clear.
    assert_eq!(px(&bytes, 16, 8, 6), [0, 0, 0, 0]);
}

#[test]
fn empty_path_draws_nothing() {
    let mut list = Draw2dList::default();
    list.push_command(Draw2dCommand::path(
        header(0, 0, 1.0),
        vec![],
        Fill2d::color(rgba(1.0, 1.0, 1.0, 1.0)),
        true,
    ));
    list.sort_commands();
    let (bytes, _, _) = render(&list, 4, 4, &Draw2dTextures::default());
    assert!(bytes.iter().all(|&b| b == 0));
}

#[test]
fn text_blits_glyph_cells_from_the_atlas() {
    // A 2-glyph run sampling a 2-cell white atlas (the font atlas), tinted red.
    // Each glyph cell is 8×16; the run lays them out at x=0 and x=8, drawn at
    // font_size 16 (so a cell renders 8×16 px). A pixel inside glyph 0 reads
    // the red tint; an untouched pixel beyond the run is clear.
    let atlas = FontHandle::from_raw(1).atlas_texture();
    // A 16×16 atlas: two 8×16 cells, all opaque white.
    let pixels = vec![255u8; 16 * 16 * 4];
    let textures = Draw2dTextures::load(&[(atlas.raw(), 16, 16, pixels)]);
    let mut list = Draw2dList::default();
    list.push_command(Draw2dCommand::text(
        header(0, 0, 1.0),
        GlyphRun::new(
            vec![
                Glyph2d::new(rect(Vec2::ZERO, Vec2::new(8.0, 16.0)), Meters::new(8.0).unwrap()),
                Glyph2d::new(rect(Vec2::new(8.0, 0.0), Vec2::new(8.0, 16.0)), Meters::new(8.0).unwrap()),
            ],
            Meters::new(16.0).unwrap(),
        ),
        TextDraw2d::new(FontHandle::from_raw(1), rgba(1.0, 0.0, 0.0, 1.0), TextAlign::LEFT),
    ));
    list.sort_commands();
    let (bytes, _, _) = render(&list, 32, 16, &textures);
    // Inside glyph 0 (x≈2, y≈8): white atlas texel × red tint = red.
    assert_eq!(px(&bytes, 32, 2, 8), [255, 0, 0, 255]);
    // Inside glyph 1 (x≈10): also red.
    assert_eq!(px(&bytes, 32, 10, 8), [255, 0, 0, 255]);
    // Beyond the run (x=20): clear.
    assert_eq!(px(&bytes, 32, 20, 8), [0, 0, 0, 0]);
}

#[test]
fn text_with_unloaded_atlas_draws_nothing() {
    let mut list = Draw2dList::default();
    list.push_command(Draw2dCommand::text(
        header(0, 0, 1.0),
        GlyphRun::new(
            vec![Glyph2d::new(rect(Vec2::ZERO, Vec2::new(8.0, 16.0)), Meters::new(8.0).unwrap())],
            Meters::new(16.0).unwrap(),
        ),
        TextDraw2d::new(FontHandle::from_raw(1), rgba(1.0, 1.0, 1.0, 1.0), TextAlign::LEFT),
    ));
    list.sort_commands();
    let (bytes, _, _) = render(&list, 16, 16, &Draw2dTextures::default());
    assert!(bytes.iter().all(|&b| b == 0));
}

#[test]
fn empty_text_run_draws_nothing() {
    let atlas = FontHandle::from_raw(1).atlas_texture();
    let textures = Draw2dTextures::load(&[(atlas.raw(), 16, 16, vec![255u8; 16 * 16 * 4])]);
    let mut list = Draw2dList::default();
    list.push_command(Draw2dCommand::text(
        header(0, 0, 1.0),
        GlyphRun::new(vec![], Meters::new(16.0).unwrap()),
        TextDraw2d::new(FontHandle::from_raw(1), rgba(1.0, 1.0, 1.0, 1.0), TextAlign::RIGHT),
    ));
    list.sort_commands();
    let (bytes, _, _) = render(&list, 16, 16, &textures);
    assert!(bytes.iter().all(|&b| b == 0));
}

#[test]
fn circle_fills_a_round_disc() {
    // A radius-3 red circle centred at (4,4) on an 8×8 buffer: the centre is
    // filled; a far corner pixel (distance > 3) is outside the disc.
    let mut list = Draw2dList::default();
    list.push_command(Draw2dCommand::circle(
        header(0, 0, 1.0),
        Vec2::new(4.0, 4.0),
        Meters::new(3.0).unwrap(),
        Fill2d::color(rgba(1.0, 0.0, 0.0, 1.0)),
    ));
    list.sort_commands();
    let (bytes, _, _) = render(&list, 8, 8, &Draw2dTextures::default());
    // Centre is inside the disc.
    assert_eq!(px(&bytes, 8, 4, 4), [255, 0, 0, 255]);
    // The (0,0) corner is ~5.6 px from the centre — outside radius 3.
    assert_eq!(px(&bytes, 8, 0, 0), [0, 0, 0, 0]);
}

#[test]
fn circle_stroke_draws_an_annulus_over_the_fill() {
    // A green circle with a 2-px red stroke: the centre keeps the green fill,
    // a near-edge pixel inside the stroke band reads red.
    let mut list = Draw2dList::default();
    list.push_command(Draw2dCommand::circle(
        header(0, 0, 1.0),
        Vec2::new(8.0, 8.0),
        Meters::new(6.0).unwrap(),
        Fill2d::color(rgba(0.0, 1.0, 0.0, 1.0))
            .with_stroke(Stroke2d::new(rgba(1.0, 0.0, 0.0, 1.0), Meters::new(2.0).unwrap())),
    ));
    list.sort_commands();
    let (bytes, _, _) = render(&list, 16, 16, &Draw2dTextures::default());
    // Dead centre: pure green fill (well inside the stroke's inner radius).
    assert_eq!(px(&bytes, 16, 8, 8), [0, 255, 0, 255]);
    // Near the right edge (x≈13, ~5 px out of radius 6): inside the 2-px stroke.
    assert_eq!(px(&bytes, 16, 13, 8), [255, 0, 0, 255]);
}

#[test]
fn ellipse_honours_radii_and_rotation() {
    // An axis-aligned ellipse rx=6, ry=2 at (8,8): a point 5 px along x is in,
    // the same distance along y is out (the short axis).
    let mut list = Draw2dList::default();
    list.push_command(Draw2dCommand::ellipse(
        header(0, 0, 1.0),
        Vec2::new(8.0, 8.0),
        Meters::new(6.0).unwrap(),
        Meters::new(2.0).unwrap(),
        axiom_kernel::Radians::new(0.0).unwrap(),
        Fill2d::color(rgba(0.0, 0.0, 1.0, 1.0)),
    ));
    list.sort_commands();
    let (bytes, _, _) = render(&list, 16, 16, &Draw2dTextures::default());
    // Along the long (x) axis at 5 px: inside.
    assert_eq!(px(&bytes, 16, 13, 8), [0, 0, 255, 255]);
    // Along the short (y) axis at 5 px: outside (ry is only 2).
    assert_eq!(px(&bytes, 16, 8, 13), [0, 0, 0, 0]);
}

#[test]
fn ellipse_rotation_swaps_the_long_axis() {
    // The same rx=6, ry=2 ellipse rotated 90°: now the long axis is vertical,
    // so the point 5 px along y is inside and 5 px along x is outside.
    let mut list = Draw2dList::default();
    list.push_command(Draw2dCommand::ellipse(
        header(0, 0, 1.0),
        Vec2::new(8.0, 8.0),
        Meters::new(6.0).unwrap(),
        Meters::new(2.0).unwrap(),
        axiom_kernel::Radians::new(std::f32::consts::FRAC_PI_2).unwrap(),
        Fill2d::color(rgba(0.0, 0.0, 1.0, 1.0)),
    ));
    list.sort_commands();
    let (bytes, _, _) = render(&list, 16, 16, &Draw2dTextures::default());
    assert_eq!(px(&bytes, 16, 8, 13), [0, 0, 255, 255]);
    assert_eq!(px(&bytes, 16, 13, 8), [0, 0, 0, 0]);
}

#[test]
fn line_strokes_a_thick_segment() {
    // A 2-px-wide horizontal line from (1,8) to (14,8): a pixel on the line is
    // coloured; one several rows away is not.
    let mut list = Draw2dList::default();
    list.push_command(Draw2dCommand::line(
        header(0, 0, 1.0),
        Vec2::new(1.0, 8.0),
        Vec2::new(14.0, 8.0),
        rgba(1.0, 1.0, 0.0, 1.0),
        Meters::new(2.0).unwrap(),
    ));
    list.sort_commands();
    let (bytes, _, _) = render(&list, 16, 16, &Draw2dTextures::default());
    // On the segment.
    assert_eq!(px(&bytes, 16, 7, 8), [255, 255, 0, 255]);
    // Far from the segment (row 0).
    assert_eq!(px(&bytes, 16, 7, 0), [0, 0, 0, 0]);
}

#[test]
fn particle_quad_fills_a_centred_square() {
    // A KIND_PARTICLE_QUAD of half-extent 2 at (8,8): the centre fills; a
    // corner well outside the 4×4 quad is clear.
    let mut list = Draw2dList::default();
    list.push_command(Draw2dCommand::particle_quad(
        header(0, 0, 1.0),
        Vec2::new(8.0, 8.0),
        Meters::new(2.0).unwrap(),
        rgba(1.0, 1.0, 1.0, 1.0),
    ));
    list.sort_commands();
    let (bytes, _, _) = render(&list, 16, 16, &Draw2dTextures::default());
    assert_eq!(px(&bytes, 16, 8, 8), [255, 255, 255, 255]);
    assert_eq!(px(&bytes, 16, 0, 0), [0, 0, 0, 0]);
}

#[test]
fn rect_stroke_borders_the_fill() {
    // A blue rect (2,2)..(14,14) with a 2-px red border: an edge pixel reads
    // the red stroke, a centre pixel keeps the blue fill.
    let mut list = Draw2dList::default();
    list.push_command(Draw2dCommand::rect(
        header(0, 0, 1.0),
        rect(Vec2::new(2.0, 2.0), Vec2::new(12.0, 12.0)),
        Fill2d::color(rgba(0.0, 0.0, 1.0, 1.0))
            .with_stroke(Stroke2d::new(rgba(1.0, 0.0, 0.0, 1.0), Meters::new(2.0).unwrap())),
    ));
    list.sort_commands();
    let (bytes, _, _) = render(&list, 16, 16, &Draw2dTextures::default());
    // Top-left inside the border band: red stroke over the blue fill.
    assert_eq!(px(&bytes, 16, 2, 2), [255, 0, 0, 255]);
    // Dead centre: blue fill, untouched by the border.
    assert_eq!(px(&bytes, 16, 8, 8), [0, 0, 255, 255]);
}

#[test]
fn zero_length_line_with_degenerate_transform_draws_nothing() {
    // A degenerate segment (a==b) under a zero-scale transform exercises the
    // EPS-floored length²: the projection stays finite and nothing is drawn.
    let mut list = Draw2dList::default();
    list.push_command(Draw2dCommand::line(
        (0, Mat3::scale(Vec2::ZERO), Common2d::new(0, ratio(1.0))),
        Vec2::new(4.0, 4.0),
        Vec2::new(4.0, 4.0),
        rgba(1.0, 1.0, 1.0, 1.0),
        Meters::new(1.0).unwrap(),
    ));
    list.sort_commands();
    let (bytes, _, _) = render(&list, 8, 8, &Draw2dTextures::default());
    assert!(bytes.iter().all(|&b| b == 0));
}

/// A 2×2 atlas: TL red, TR green, BL blue, BR white — all opaque.
fn atlas() -> Draw2dTextures {
    let rgba = vec![
        255, 0, 0, 255, // (0,0) red
        0, 255, 0, 255, // (1,0) green
        0, 0, 255, 255, // (0,1) blue
        255, 255, 255, 255, // (1,1) white
    ];
    Draw2dTextures::load(&[(7, 2, 2, rgba)])
}

fn sprite_opts(flip_x: bool, flip_y: bool, tint: Rgba) -> SpriteDraw2d {
    SpriteDraw2d::new(
        rect(Vec2::ZERO, Vec2::new(2.0, 2.0)),
        Vec2::ZERO,
        tint,
        flip_x,
        flip_y,
    )
}

#[test]
fn sprite_blits_atlas_pixels() {
    let mut list = Draw2dList::default();
    list.push_command(Draw2dCommand::sprite(
        header(0, 0, 1.0),
        TextureId::from_raw(7),
        sprite_opts(false, false, rgba(1.0, 1.0, 1.0, 1.0)),
    ));
    list.sort_commands();
    // Dest is the 2×2 source at the origin → pixels (0,0)..(2,2).
    let (bytes, _, _) = render(&list, 2, 2, &atlas());
    assert_eq!(px(&bytes, 2, 0, 0), [255, 0, 0, 255]); // TL red
    assert_eq!(px(&bytes, 2, 1, 0), [0, 255, 0, 255]); // TR green
    assert_eq!(px(&bytes, 2, 0, 1), [0, 0, 255, 255]); // BL blue
    assert_eq!(px(&bytes, 2, 1, 1), [255, 255, 255, 255]); // BR white
}

#[test]
fn sprite_flip_x_and_y_mirror_the_sample() {
    let mut list = Draw2dList::default();
    list.push_command(Draw2dCommand::sprite(
        header(0, 0, 1.0),
        TextureId::from_raw(7),
        sprite_opts(true, true, rgba(1.0, 1.0, 1.0, 1.0)),
    ));
    list.sort_commands();
    let (bytes, _, _) = render(&list, 2, 2, &atlas());
    // Both axes mirrored: TL now samples the atlas BR (white), TR samples BL.
    assert_eq!(px(&bytes, 2, 0, 0), [255, 255, 255, 255]); // was BR white
    assert_eq!(px(&bytes, 2, 1, 1), [255, 0, 0, 255]); // was TL red
}

#[test]
fn sprite_tint_and_alpha_modulate_the_blit() {
    let mut list = Draw2dList::default();
    // Half-alpha command, green tint → white texel becomes half-alpha green.
    list.push_command(Draw2dCommand::sprite(
        header(0, 0, 0.5),
        TextureId::from_raw(9),
        sprite_opts(false, false, rgba(0.0, 1.0, 0.0, 1.0)),
    ));
    list.sort_commands();
    // A 1×1 white opaque texture id 9.
    let tex = Draw2dTextures::load(&[(9, 1, 1, vec![255, 255, 255, 255])]);
    let (bytes, _, _) = render(&list, 2, 2, &tex);
    // src = white·green-tint·0.5 alpha = (0,1,0,0.5) over transparent →
    // out_rgb = green·0.5 = (0,128,0), out_a = 0.5.
    assert_eq!(px(&bytes, 2, 0, 0), [0, 128, 0, 128]);
}

#[test]
fn sprite_with_unknown_texture_is_a_no_op() {
    let mut list = Draw2dList::default();
    list.push_command(Draw2dCommand::sprite(
        header(0, 0, 1.0),
        TextureId::from_raw(404),
        sprite_opts(false, false, rgba(1.0, 1.0, 1.0, 1.0)),
    ));
    list.sort_commands();
    let (bytes, _, _) = render(&list, 2, 2, &atlas());
    assert!(bytes.iter().all(|&b| b == 0));
}

#[test]
fn far_offscreen_rect_draws_nothing() {
    let mut list = Draw2dList::default();
    list.push_command(Draw2dCommand::rect(
        header(0, 0, 1.0),
        rect(Vec2::new(100.0, 100.0), Vec2::new(10.0, 10.0)),
        Fill2d::color(rgba(1.0, 1.0, 1.0, 1.0)),
    ));
    list.sort_commands();
    let (bytes, _, _) = render(&list, 4, 4, &Draw2dTextures::default());
    assert!(bytes.iter().all(|&b| b == 0));
}

#[test]
fn sprite_texture_sample_in_and_out_of_range() {
    let t = SpriteTexture {
        width: 2,
        height: 2,
        // Only one pixel of bytes — sampling (1,1) reads past the buffer.
        rgba: vec![255, 0, 0, 255],
    };
    assert_eq!(t.sample(0.0, 0.0), [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(t.sample(1.0, 1.0), [0.0, 0.0, 0.0, 0.0]);
}
