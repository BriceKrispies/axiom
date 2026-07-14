//! Unit tests for the GPU 2D raster covered core ([`super`]).

use super::*;
use axiom_host::{
    Common2d, Fill2d, FontHandle, Glyph2d, GradientStop, PaintId, Rgba, Stroke2d, TextAlign,
};
use axiom_kernel::{Meters, Radians, Ratio};

fn ratio(v: f32) -> Ratio {
    Ratio::new(v).unwrap()
}

fn rgba(r: f32, g: f32, b: f32, a: f32) -> Rgba {
    Rgba::new(ratio(r), ratio(g), ratio(b), ratio(a))
}

fn header(submission: u32, layer: i32, alpha: f32) -> (u32, Mat3, Common2d) {
    (
        submission,
        Mat3::IDENTITY,
        Common2d::new(layer, ratio(alpha)),
    )
}

/// The `VERTEX_FLOATS` floats of vertex `v` (0..4) of quad `q`.
fn vert(geo: &Draw2dGeometry, q: usize, v: usize) -> [f32; VERTEX_FLOATS] {
    let base = (q * VERTS_PER_QUAD + v) * VERTEX_FLOATS;
    geo.vertices()[base..base + VERTEX_FLOATS]
        .try_into()
        .unwrap()
}

#[test]
fn rect_emits_a_fill_and_a_stroke_quad() {
    let mut list = Draw2dList::default();
    list.push_command(Draw2dCommand::rect(
        header(0, 0, 0.5),
        Rect::new(Vec2::new(2.0, 4.0), Vec2::new(6.0, 8.0)),
        Fill2d::color(rgba(1.0, 0.0, 0.0, 1.0)),
    ));
    list.sort_commands();
    let geo = build_geometry(&list, 64, 64, &Draw2dTextureSizes::default());
    // Fill quad then stroke quad (both Solid; the stroke is transparent here).
    assert_eq!(geo.quad_count(), 2);
    assert_eq!(geo.sources(), &[QuadSource::Solid, QuadSource::Solid]);
    let tl = vert(&geo, 0, 0);
    let br = vert(&geo, 0, 2);
    assert_eq!([tl[0], tl[1]], [2.0, 4.0]);
    assert_eq!([br[0], br[1]], [8.0, 12.0]);
    // Fill colour is straight red with alpha 1·0.5 folded; kind is plain capsule.
    assert_eq!([tl[4], tl[5], tl[6], tl[7]], [1.0, 0.0, 0.0, 0.5]);
    assert_eq!(tl[12], KIND_CAPSULE);
    // The (absent) stroke quad is the rect-stroke kind, transparent colour.
    let s = vert(&geo, 1, 0);
    assert_eq!(s[12], KIND_RECT_STROKE);
    assert_eq!([s[4], s[5], s[6], s[7]], [0.0, 0.0, 0.0, 0.0]);
}

#[test]
fn rect_stroke_field_is_edge_distance_over_width() {
    // A 2px stroke on a (0,0)..(8,8) rect: at the TL corner the left/top edge
    // distances are 0, the right/bottom are 8; divided by width 2 → 0 and 4.
    let mut list = Draw2dList::default();
    list.push_command(Draw2dCommand::rect(
        header(0, 0, 1.0),
        Rect::new(Vec2::ZERO, Vec2::new(8.0, 8.0)),
        Fill2d::color(rgba(0.0, 0.0, 1.0, 1.0)).with_stroke(Stroke2d::new(
            rgba(1.0, 0.0, 0.0, 1.0),
            Meters::new(2.0).unwrap(),
        )),
    ));
    list.sort_commands();
    let geo = build_geometry(&list, 16, 16, &Draw2dTextureSizes::default());
    let s = vert(&geo, 1, 0);
    // field = (left/w, right/w, top/w, bottom/w) = (0, 4, 0, 4); red stroke.
    assert_eq!([s[8], s[9], s[10], s[11]], [0.0, 4.0, 0.0, 4.0]);
    assert_eq!([s[4], s[5], s[6], s[7]], [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(s[12], KIND_RECT_STROKE);
}

#[test]
fn absent_and_unknown_paint_fills_resolve_transparent() {
    // A paint id naming no registered paint → transparent fill (no gradient).
    let mut list = Draw2dList::default();
    list.push_command(Draw2dCommand::rect(
        header(0, 0, 1.0),
        Rect::new(Vec2::ZERO, Vec2::ONE),
        Fill2d::paint(PaintId::from_raw(0)),
    ));
    list.sort_commands();
    let geo = build_geometry(&list, 8, 8, &Draw2dTextureSizes::default());
    let tl = vert(&geo, 0, 0);
    assert_eq!([tl[4], tl[5], tl[6], tl[7]], [0.0, 0.0, 0.0, 0.0]);
    assert_eq!(geo.sources()[0], QuadSource::Solid);
    assert!(geo.gradient_textures().is_empty());
}

#[test]
fn linear_gradient_fill_binds_a_ramp_with_affine_uvs() {
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
        Rect::new(Vec2::ZERO, Vec2::new(8.0, 8.0)),
        Fill2d::paint(lin),
    ));
    list.sort_commands();
    let geo = build_geometry(&list, 16, 16, &Draw2dTextureSizes::default());
    // The fill quad binds the baked ramp texture; the ramp is registered n×1.
    let ramp_id = GRADIENT_TEXTURE_BASE + u64::from(lin.raw());
    assert_eq!(geo.sources()[0], QuadSource::Sprite(ramp_id));
    let textures = geo.gradient_textures();
    assert_eq!(textures.len(), 1);
    assert_eq!(
        (textures[0].0, textures[0].1, textures[0].2),
        (ramp_id, RAMP_N, 1)
    );
    // Colour is white (the ramp carries the colour); UV.x spans 0→1 across the
    // gradient axis (left corner 0, right corner 1).
    let tl = vert(&geo, 0, 0);
    let br = vert(&geo, 0, 2);
    assert_eq!([tl[4], tl[5], tl[6], tl[7]], [1.0, 1.0, 1.0, 1.0]);
    assert!((tl[2] - 0.0).abs() < 1e-5);
    assert!((br[2] - 1.0).abs() < 1e-5);
}

#[test]
fn radial_gradient_fill_bakes_a_disc_and_centres_uv() {
    let mut list = Draw2dList::default();
    let rad = list.register_radial(
        Vec2::new(4.0, 4.0),
        Meters::new(4.0).unwrap(),
        vec![
            GradientStop::new(ratio(0.0), rgba(1.0, 1.0, 1.0, 1.0)),
            GradientStop::new(ratio(1.0), rgba(0.0, 0.0, 0.0, 1.0)),
        ],
    );
    list.push_command(Draw2dCommand::circle(
        header(0, 0, 1.0),
        Vec2::new(4.0, 4.0),
        Meters::new(4.0).unwrap(),
        Fill2d::paint(rad),
    ));
    list.sort_commands();
    let geo = build_geometry(&list, 16, 16, &Draw2dTextureSizes::default());
    let ramp_id = GRADIENT_TEXTURE_BASE + u64::from(rad.raw());
    // Circle fill quad binds the radial disc (n×n); the conic coverage stays.
    assert_eq!(geo.sources()[0], QuadSource::Sprite(ramp_id));
    let textures = geo.gradient_textures();
    assert_eq!((textures[0].1, textures[0].2), (RAMP_N, RAMP_N));
    let tl = vert(&geo, 0, 0);
    assert_eq!(tl[12], KIND_CONIC);
    // The bbox corner is at the gradient-radius edge → UV ≈ (0,0) (the
    // ((p-c)/r)*0.5+0.5 of (-1,-1)). Conic basis still present in the field.
    assert!((tl[2] - 0.0).abs() < 1e-5);
    assert!((tl[3] - 0.0).abs() < 1e-5);
}

#[test]
fn path_fan_triangulates_into_barycentric_quads_plus_stroke() {
    // A closed convex triangle path: one fan triangle (fill) + 3 stroke edges.
    let mut list = Draw2dList::default();
    list.push_command(Draw2dCommand::path(
        header(0, 0, 1.0),
        vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(8.0, 0.0),
            Vec2::new(8.0, 8.0),
        ],
        Fill2d::color(rgba(0.2, 0.4, 0.6, 1.0)).with_stroke(Stroke2d::new(
            rgba(1.0, 1.0, 1.0, 1.0),
            Meters::new(1.0).unwrap(),
        )),
        true,
    ));
    list.sort_commands();
    let geo = build_geometry(&list, 16, 16, &Draw2dTextureSizes::default());
    // 1 fan triangle + 3 edge capsules = 4 quads.
    assert_eq!(geo.quad_count(), 4);
    // Triangle 0 is TRI kind; the first vertex's barycentric is (1,0,0) at a=(0,0).
    let t = vert(&geo, 0, 0);
    assert_eq!(t[12], KIND_TRI);
    // The bbox corner (0,0) coincides with vertex a → barycentric l0≈1.
    assert!((t[8] - 1.0).abs() < 1e-4);
    // Edge quads are capsule strokes coloured white.
    let e = vert(&geo, 1, 0);
    assert_eq!(e[12], KIND_CAPSULE);
    assert_eq!([e[4], e[5], e[6], e[7]], [1.0, 1.0, 1.0, 1.0]);
}

#[test]
fn open_path_emits_no_fill_and_omits_the_closing_edge() {
    // An open 3-point polyline: fill alpha 0 (no fill drawn) and only 2 edge
    // strokes (the closing edge has zero width → a no-op capsule).
    let mut list = Draw2dList::default();
    list.push_command(Draw2dCommand::path(
        header(0, 0, 1.0),
        vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(8.0, 0.0),
            Vec2::new(8.0, 8.0),
        ],
        Fill2d::color(rgba(0.2, 0.4, 0.6, 1.0)).with_stroke(Stroke2d::new(
            rgba(1.0, 1.0, 1.0, 1.0),
            Meters::new(2.0).unwrap(),
        )),
        false,
    ));
    list.sort_commands();
    let geo = build_geometry(&list, 16, 16, &Draw2dTextureSizes::default());
    // 1 fan triangle (transparent) + 3 edge quads (the closing one zero-width).
    assert_eq!(geo.quad_count(), 4);
    // The fill triangle's colour alpha folded with closed=false → 0.
    assert_eq!(vert(&geo, 0, 0)[7], 0.0);
    // The closing edge (quad 3) has half_width 0 (field[3]).
    assert_eq!(vert(&geo, 3, 0)[11], 0.0);
    // A real edge (quad 1) has a non-zero half_width.
    assert!(vert(&geo, 1, 0)[11] > 0.0);
}

#[test]
fn degenerate_path_under_three_points_emits_only_strokes() {
    // Two points (a line): no fan triangle, just edge strokes (no panic on the
    // saturating fan range / modulo).
    let mut list = Draw2dList::default();
    list.push_command(Draw2dCommand::path(
        header(0, 0, 1.0),
        vec![Vec2::new(1.0, 1.0), Vec2::new(5.0, 1.0)],
        Fill2d::stroked(Stroke2d::new(
            rgba(1.0, 0.0, 0.0, 1.0),
            Meters::new(1.0).unwrap(),
        )),
        true,
    ));
    list.sort_commands();
    let geo = build_geometry(&list, 8, 8, &Draw2dTextureSizes::default());
    // No fan triangle (n<3); 2 edge capsules (forward + closing wrap).
    assert_eq!(geo.quad_count(), 2);
    assert_eq!(vert(&geo, 0, 0)[12], KIND_CAPSULE);
}

#[test]
fn empty_path_emits_nothing() {
    let mut list = Draw2dList::default();
    list.push_command(Draw2dCommand::path(
        header(0, 0, 1.0),
        vec![],
        Fill2d::color(rgba(1.0, 1.0, 1.0, 1.0)),
        true,
    ));
    list.sort_commands();
    let geo = build_geometry(&list, 8, 8, &Draw2dTextureSizes::default());
    assert_eq!(geo.quad_count(), 0);
}

#[test]
fn text_lays_out_glyphs_as_sprite_quads_against_the_atlas() {
    let atlas = FontHandle::from_raw(1).atlas_texture();
    let mut list = Draw2dList::default();
    list.push_command(Draw2dCommand::text(
        header(0, 0, 1.0),
        GlyphRun::new(
            vec![
                Glyph2d::new(
                    Rect::new(Vec2::ZERO, Vec2::new(8.0, 16.0)),
                    Meters::new(8.0).unwrap(),
                ),
                Glyph2d::new(
                    Rect::new(Vec2::new(8.0, 0.0), Vec2::new(8.0, 16.0)),
                    Meters::new(8.0).unwrap(),
                ),
            ],
            Meters::new(16.0).unwrap(),
        ),
        TextDraw2d::new(
            FontHandle::from_raw(1),
            rgba(0.0, 1.0, 0.0, 1.0),
            TextAlign::LEFT,
        ),
    ));
    list.sort_commands();
    let sizes = Draw2dTextureSizes::from_textures(&[(atlas.raw(), 128, 96, vec![0; 128 * 96 * 4])]);
    let geo = build_geometry(&list, 64, 64, &sizes);
    // Two glyphs → two sprite quads against the atlas, tinted green.
    assert_eq!(geo.quad_count(), 2);
    assert_eq!(
        geo.sources(),
        &[
            QuadSource::Sprite(atlas.raw()),
            QuadSource::Sprite(atlas.raw())
        ]
    );
    // Glyph 0 at pen x=0 (left aligned): its dest TL is the origin.
    assert_eq!([vert(&geo, 0, 0)[0], vert(&geo, 0, 0)[1]], [0.0, 0.0]);
    // Glyph 1 advanced by 8 → its dest TL is x=8.
    assert!((vert(&geo, 1, 0)[0] - 8.0).abs() < 1e-4);
    assert_eq!(
        [
            vert(&geo, 0, 0)[4],
            vert(&geo, 0, 0)[5],
            vert(&geo, 0, 0)[6]
        ],
        [0.0, 1.0, 0.0]
    );
}

#[test]
fn text_alignment_shifts_the_pen_start() {
    let atlas = FontHandle::from_raw(1).atlas_texture();
    let run = || {
        GlyphRun::new(
            vec![Glyph2d::new(
                Rect::new(Vec2::ZERO, Vec2::new(8.0, 16.0)),
                Meters::new(8.0).unwrap(),
            )],
            Meters::new(16.0).unwrap(),
        )
    };
    let sizes = Draw2dTextureSizes::from_textures(&[(atlas.raw(), 128, 96, vec![0; 128 * 96 * 4])]);
    let build = |align: TextAlign| {
        let mut list = Draw2dList::default();
        list.push_command(Draw2dCommand::text(
            header(0, 0, 1.0),
            run(),
            TextDraw2d::new(FontHandle::from_raw(1), rgba(1.0, 1.0, 1.0, 1.0), align),
        ));
        list.sort_commands();
        let geo = build_geometry(&list, 64, 64, &sizes);
        vert(&geo, 0, 0)[0]
    };
    // Left starts at 0; centre at -advance/2 = -4; right at -advance = -8.
    assert!((build(TextAlign::LEFT) - 0.0).abs() < 1e-4);
    assert!((build(TextAlign::CENTER) + 4.0).abs() < 1e-4);
    assert!((build(TextAlign::RIGHT) + 8.0).abs() < 1e-4);
}

#[test]
fn text_with_unloaded_atlas_emits_no_quads() {
    let mut list = Draw2dList::default();
    list.push_command(Draw2dCommand::text(
        header(0, 0, 1.0),
        GlyphRun::new(
            vec![Glyph2d::new(
                Rect::new(Vec2::ZERO, Vec2::new(8.0, 16.0)),
                Meters::new(8.0).unwrap(),
            )],
            Meters::new(16.0).unwrap(),
        ),
        TextDraw2d::new(
            FontHandle::from_raw(1),
            rgba(1.0, 1.0, 1.0, 1.0),
            TextAlign::LEFT,
        ),
    ));
    list.sort_commands();
    // No atlas loaded → the sprite path skips every glyph.
    let geo = build_geometry(&list, 64, 64, &Draw2dTextureSizes::default());
    assert_eq!(geo.quad_count(), 0);
}

#[test]
fn empty_text_run_emits_nothing() {
    let atlas = FontHandle::from_raw(1).atlas_texture();
    let mut list = Draw2dList::default();
    list.push_command(Draw2dCommand::text(
        header(0, 0, 1.0),
        GlyphRun::new(vec![], Meters::new(16.0).unwrap()),
        TextDraw2d::new(
            FontHandle::from_raw(1),
            rgba(1.0, 1.0, 1.0, 1.0),
            TextAlign::LEFT,
        ),
    ));
    list.sort_commands();
    let sizes = Draw2dTextureSizes::from_textures(&[(atlas.raw(), 128, 96, vec![0; 128 * 96 * 4])]);
    let geo = build_geometry(&list, 64, 64, &sizes);
    assert_eq!(geo.quad_count(), 0);
}

#[test]
fn circle_emits_a_conic_fill_and_conic_stroke() {
    let mut list = Draw2dList::default();
    list.push_command(Draw2dCommand::circle(
        header(0, 0, 1.0),
        Vec2::new(4.0, 4.0),
        Meters::new(2.0).unwrap(),
        Fill2d::color(rgba(1.0, 0.0, 0.0, 1.0)).with_stroke(Stroke2d::new(
            rgba(0.0, 1.0, 0.0, 1.0),
            Meters::new(1.0).unwrap(),
        )),
    ));
    list.sort_commands();
    let geo = build_geometry(&list, 8, 8, &Draw2dTextureSizes::default());
    assert_eq!(geo.quad_count(), 2);
    // Fill quad: conic kind, red, basis (s,t) at the corners is the unit square.
    let f = vert(&geo, 0, 0);
    assert_eq!(f[12], KIND_CONIC);
    assert_eq!([f[8], f[9]], [-1.0, -1.0]);
    // Stroke quad: conic-stroke kind, green, inner² in field.z. inner =
    // 1 - 1/2 = 0.5 → inner² = 0.25.
    let s = vert(&geo, 1, 0);
    assert_eq!(s[12], KIND_CONIC_STROKE);
    assert_eq!([s[4], s[5], s[6], s[7]], [0.0, 1.0, 0.0, 1.0]);
    assert!((s[10] - 0.25).abs() < 1e-5);
}

#[test]
fn ellipse_rotation_orients_the_bounding_box() {
    let mut list = Draw2dList::default();
    list.push_command(Draw2dCommand::ellipse(
        header(0, 0, 1.0),
        Vec2::new(8.0, 8.0),
        Meters::new(4.0).unwrap(),
        Meters::new(2.0).unwrap(),
        Radians::new(std::f32::consts::FRAC_PI_2).unwrap(),
        Fill2d::color(rgba(0.0, 0.0, 1.0, 1.0)),
    ));
    list.sort_commands();
    let geo = build_geometry(&list, 16, 16, &Draw2dTextureSizes::default());
    let tl = vert(&geo, 0, 0);
    let br = vert(&geo, 0, 2);
    assert!((tl[0] - 6.0).abs() < 1e-4);
    assert!((tl[1] - 4.0).abs() < 1e-4);
    assert!((br[0] - 10.0).abs() < 1e-4);
    assert!((br[1] - 12.0).abs() < 1e-4);
    assert_eq!(tl[12], KIND_CONIC);
}

#[test]
fn line_emits_one_capsule_quad_with_local_coords() {
    let mut list = Draw2dList::default();
    list.push_command(Draw2dCommand::line(
        header(0, 0, 1.0),
        Vec2::new(1.0, 8.0),
        Vec2::new(14.0, 8.0),
        rgba(1.0, 1.0, 0.0, 1.0),
        Meters::new(2.0).unwrap(),
    ));
    list.sort_commands();
    let geo = build_geometry(&list, 16, 16, &Draw2dTextureSizes::default());
    assert_eq!(geo.quad_count(), 1);
    let tl = vert(&geo, 0, 0);
    let br = vert(&geo, 0, 2);
    assert_eq!([tl[0], tl[1]], [0.0, 7.0]);
    assert_eq!([br[0], br[1]], [15.0, 9.0]);
    assert_eq!([tl[8], tl[9], tl[10], tl[11]], [-1.0, -1.0, 13.0, 1.0]);
    assert_eq!(tl[12], KIND_CAPSULE);
}

#[test]
fn zero_length_line_falls_back_to_an_axis_aligned_dot() {
    let mut list = Draw2dList::default();
    list.push_command(Draw2dCommand::line(
        (0, Mat3::scale(Vec2::ZERO), Common2d::new(0, ratio(1.0))),
        Vec2::new(4.0, 4.0),
        Vec2::new(4.0, 4.0),
        rgba(1.0, 1.0, 1.0, 1.0),
        Meters::new(1.0).unwrap(),
    ));
    list.sort_commands();
    let geo = build_geometry(&list, 8, 8, &Draw2dTextureSizes::default());
    assert_eq!(geo.quad_count(), 1);
    let tl = vert(&geo, 0, 0);
    assert_eq!([tl[0], tl[1]], [0.0, 0.0]);
    assert_eq!([tl[10], tl[11]], [0.0, 0.0]);
}

#[test]
fn particle_emits_a_centred_plain_quad() {
    let mut list = Draw2dList::default();
    list.push_command(Draw2dCommand::particle_quad(
        header(0, 0, 1.0),
        Vec2::new(8.0, 8.0),
        Meters::new(2.0).unwrap(),
        rgba(0.0, 1.0, 1.0, 1.0),
    ));
    list.sort_commands();
    let geo = build_geometry(&list, 16, 16, &Draw2dTextureSizes::default());
    assert_eq!(geo.quad_count(), 1);
    let tl = vert(&geo, 0, 0);
    let br = vert(&geo, 0, 2);
    assert_eq!([tl[0], tl[1]], [6.0, 6.0]);
    assert_eq!([br[0], br[1]], [10.0, 10.0]);
    assert_eq!(tl[12], KIND_CAPSULE);
    assert_eq!([tl[8], tl[9], tl[10], tl[11]], PLAIN_FIELD);
    assert_eq!([tl[4], tl[5], tl[6], tl[7]], [0.0, 1.0, 1.0, 1.0]);
}

#[test]
fn list_order_is_painter_order_across_layers() {
    let mut list = Draw2dList::default();
    list.push_command(Draw2dCommand::particle_quad(
        header(0, 2, 1.0),
        Vec2::ZERO,
        Meters::new(1.0).unwrap(),
        rgba(0.0, 0.0, 1.0, 1.0),
    ));
    list.push_command(Draw2dCommand::particle_quad(
        header(1, 1, 1.0),
        Vec2::ZERO,
        Meters::new(1.0).unwrap(),
        rgba(1.0, 0.0, 0.0, 1.0),
    ));
    list.sort_commands();
    let geo = build_geometry(&list, 8, 8, &Draw2dTextureSizes::default());
    assert_eq!(geo.quad_count(), 2);
    assert_eq!(vert(&geo, 0, 0)[4..8], [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(vert(&geo, 1, 0)[4..8], [0.0, 0.0, 1.0, 1.0]);
}

#[test]
fn camera_zoom_and_center_place_the_quad() {
    let mut list = Draw2dList::default();
    list.set_camera(Camera2d::new(Vec2::ZERO, ratio(2.0)));
    list.push_command(Draw2dCommand::particle_quad(
        header(0, 0, 1.0),
        Vec2::new(0.5, 0.5),
        Meters::new(0.5).unwrap(),
        rgba(0.0, 1.0, 0.0, 1.0),
    ));
    list.sort_commands();
    // particle at (0.5,0.5) half 0.5 → quad (0,0)..(1,1) world; zoom 2, centre
    // (4,4) → (4,4)..(6,6).
    let geo = build_geometry(&list, 8, 8, &Draw2dTextureSizes::default());
    assert_eq!([vert(&geo, 0, 0)[0], vert(&geo, 0, 0)[1]], [4.0, 4.0]);
    assert_eq!([vert(&geo, 0, 2)[0], vert(&geo, 0, 2)[1]], [6.0, 6.0]);
}

#[test]
fn sprite_emits_a_textured_quad_with_normalised_uvs() {
    let mut list = Draw2dList::default();
    let opts = SpriteDraw2d::new(
        Rect::new(Vec2::ZERO, Vec2::new(2.0, 2.0)),
        Vec2::ZERO,
        rgba(1.0, 1.0, 1.0, 1.0),
        false,
        false,
    );
    list.push_command(Draw2dCommand::sprite(
        header(0, 0, 1.0),
        TextureId::from_raw(7),
        opts,
    ));
    list.sort_commands();
    let sizes = Draw2dTextureSizes::from_textures(&[(7, 2, 2, vec![0; 16])]);
    let geo = build_geometry(&list, 8, 8, &sizes);
    assert_eq!(geo.quad_count(), 1);
    assert_eq!(geo.sources(), &[QuadSource::Sprite(7)]);
    let tl = vert(&geo, 0, 0);
    let br = vert(&geo, 0, 2);
    assert_eq!([tl[2], tl[3]], [0.0, 0.0]);
    assert_eq!([br[2], br[3]], [1.0, 1.0]);
}

#[test]
fn sprite_flip_x_and_y_swap_the_uv_corners() {
    let mut list = Draw2dList::default();
    let opts = SpriteDraw2d::new(
        Rect::new(Vec2::ZERO, Vec2::new(2.0, 2.0)),
        Vec2::ZERO,
        rgba(1.0, 1.0, 1.0, 1.0),
        true,
        true,
    );
    list.push_command(Draw2dCommand::sprite(
        header(0, 0, 1.0),
        TextureId::from_raw(7),
        opts,
    ));
    list.sort_commands();
    let sizes = Draw2dTextureSizes::from_textures(&[(7, 2, 2, vec![0; 16])]);
    let geo = build_geometry(&list, 8, 8, &sizes);
    assert_eq!([vert(&geo, 0, 0)[2], vert(&geo, 0, 0)[3]], [1.0, 1.0]);
    assert_eq!([vert(&geo, 0, 2)[2], vert(&geo, 0, 2)[3]], [0.0, 0.0]);
}

#[test]
fn sprite_tint_and_alpha_fold_into_the_colour() {
    let mut list = Draw2dList::default();
    let opts = SpriteDraw2d::new(
        Rect::new(Vec2::ZERO, Vec2::new(1.0, 1.0)),
        Vec2::ZERO,
        rgba(0.0, 1.0, 0.0, 1.0),
        false,
        false,
    );
    list.push_command(Draw2dCommand::sprite(
        header(0, 0, 0.5),
        TextureId::from_raw(9),
        opts,
    ));
    list.sort_commands();
    let sizes = Draw2dTextureSizes::from_textures(&[(9, 1, 1, vec![255; 4])]);
    let geo = build_geometry(&list, 4, 4, &sizes);
    assert_eq!(vert(&geo, 0, 0)[4..8], [0.0, 1.0, 0.0, 0.5]);
}

#[test]
fn sprite_with_unknown_texture_emits_no_quad() {
    let mut list = Draw2dList::default();
    let opts = SpriteDraw2d::new(
        Rect::new(Vec2::ZERO, Vec2::new(2.0, 2.0)),
        Vec2::ZERO,
        rgba(1.0, 1.0, 1.0, 1.0),
        false,
        false,
    );
    list.push_command(Draw2dCommand::sprite(
        header(0, 0, 1.0),
        TextureId::from_raw(404),
        opts,
    ));
    list.sort_commands();
    let geo = build_geometry(&list, 8, 8, &Draw2dTextureSizes::default());
    assert_eq!(geo.quad_count(), 0);
}

#[test]
fn texture_sizes_round_trip_and_miss() {
    let sizes = Draw2dTextureSizes::from_textures(&[(1, 4, 8, vec![0; 128])]);
    assert_eq!(sizes.get(1), Some((4.0, 8.0)));
    assert_eq!(sizes.get(2), None);
    assert_eq!(sizes.clone(), sizes);
    assert!(format!("{sizes:?}").contains("Draw2dTextureSizes"));
}

#[test]
fn empty_list_yields_empty_geometry() {
    let geo = build_geometry(&Draw2dList::default(), 8, 8, &Draw2dTextureSizes::default());
    assert_eq!(geo, Draw2dGeometry::default());
    assert_eq!(geo.quad_count(), 0);
    assert!(geo.gradient_textures().is_empty());
    assert!(format!("{geo:?}").contains("Draw2dGeometry"));
    assert_eq!(QuadSource::Solid, QuadSource::Solid);
    assert_ne!(QuadSource::Solid, QuadSource::Sprite(1));
    assert!(format!("{:?}", QuadSource::Sprite(1)).contains("Sprite"));
}

#[test]
fn two_gradient_fills_register_distinct_sorted_ramps() {
    // Cover the gradient_textures sort + the dedup-by-id map with two paints.
    let mut list = Draw2dList::default();
    let a = list.register_linear(
        Vec2::ZERO,
        Vec2::new(4.0, 0.0),
        vec![GradientStop::new(ratio(0.0), rgba(1.0, 0.0, 0.0, 1.0))],
    );
    let b = list.register_radial(
        Vec2::ZERO,
        Meters::new(4.0).unwrap(),
        vec![GradientStop::new(ratio(0.0), rgba(0.0, 1.0, 0.0, 1.0))],
    );
    list.push_command(Draw2dCommand::rect(
        header(0, 0, 1.0),
        Rect::new(Vec2::ZERO, Vec2::new(4.0, 4.0)),
        Fill2d::paint(b),
    ));
    list.push_command(Draw2dCommand::rect(
        header(1, 1, 1.0),
        Rect::new(Vec2::ZERO, Vec2::new(4.0, 4.0)),
        Fill2d::paint(a),
    ));
    list.sort_commands();
    let geo = build_geometry(&list, 8, 8, &Draw2dTextureSizes::default());
    let textures = geo.gradient_textures();
    assert_eq!(textures.len(), 2);
    // Sorted by id: paint a (linear) before paint b (radial).
    assert_eq!(textures[0].0, GRADIENT_TEXTURE_BASE + u64::from(a.raw()));
    assert_eq!(textures[1].0, GRADIENT_TEXTURE_BASE + u64::from(b.raw()));
}
