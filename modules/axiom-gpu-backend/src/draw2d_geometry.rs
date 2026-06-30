//! The **covered core** of the GPU 2D raster arm: walk a layer-sorted
//! [`axiom_host::Draw2dList`] into backend-neutral quad geometry the platform
//! (wgpu) arm uploads and draws.
//!
//! This is the GPU peer of the canvas backend's `draw2d_raster` *list walk*: it
//! resolves each command's baked [`Mat3`] transform (composed with the frame's
//! optional [`axiom_host::Camera2d`]), folds the command `alpha` into the source
//! colour, and emits one axis-aligned quad per supported command — **in list
//! order**, which is already the host's `(layer, submission)` painter's order, so
//! a later quad composites over an earlier one on the GPU exactly as the software
//! `composite_pixel` paints later commands over earlier ones.
//!
//! ## Scope — parity with the software subset
//! The GPU 2D arm is scoped to the software rasterizer's **alpha-composited
//! subset that has no per-pixel sampling ambiguity**: filled **rect** (its
//! transformed axis-aligned bounding box, matching the software fill) and
//! **sprite** (an atlas source-rect blit with tint, per-axis flip, and alpha).
//! The deferred kinds — circle, ellipse, line, particle-quad, path, text, and
//! gradient/paint fills — are **not** emitted here (their `as_*` accessor is never
//! queried), exactly as the software backend defers path/text/paint. A command of
//! a non-emitted kind contributes zero quads, branchlessly.
//!
//! ## Coordinate model
//! Quad positions are **framebuffer pixels** (top-left origin), the same space the
//! software rasterizer reasons in; the platform arm's vertex shader converts them
//! to NDC. Colours are straight linear RGBA with the command alpha folded into the
//! alpha channel, so the GPU's `ALPHA_BLENDING` (`out = src·a + dst·(1−a)`)
//! reproduces the software `over()` blend.
//!
//! Pure Rust — no wgpu, no browser types — so it builds, is branchless, and is
//! fully covered on native exactly as on wasm.

use std::collections::HashMap;

use axiom_host::{Camera2d, Draw2dCommand, Draw2dList, Rect, Rgba, SpriteDraw2d, TextureId};
use axiom_math::{Mat3, Vec2};

/// The fully-transparent colour an absent / deferred-paint fill resolves to:
/// emitting a quad with alpha 0 composites nothing, so a "no fill" command draws
/// nothing without a branch — the same no-op-composite the software path relies on.
const TRANSPARENT: [f32; 4] = [0.0, 0.0, 0.0, 0.0];

/// Sprite/atlas texture **dimensions** keyed by the contract's
/// [`TextureId`] raw value — the GPU peer of the canvas backend's
/// `Draw2dTextures`, but holding only sizes (the pixels live in GPU textures the
/// platform arm uploads). Used to normalise a sprite's source sub-rect into
/// `0..1` UVs. A sprite naming an unknown id emits no quad (branchless via `get`
/// → `into_iter`), the same skip the software path makes for an unknown texture.
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct Draw2dTextureSizes {
    map: HashMap<u64, (f32, f32)>,
}

impl Draw2dTextureSizes {
    /// Build the size registry from `(texture_id, width, height, RGBA8 pixels)` —
    /// the same upload shape the 3D material path and the canvas sprite path use;
    /// only the dimensions are retained here.
    pub(crate) fn from_textures(textures: &[(u64, u32, u32, Vec<u8>)]) -> Self {
        Draw2dTextureSizes {
            map: textures
                .iter()
                .map(|(id, w, h, _)| (*id, (*w as f32, *h as f32)))
                .collect(),
        }
    }

    fn get(&self, id: u64) -> Option<(f32, f32)> {
        self.map.get(&id).copied()
    }
}

/// Where one generated quad samples its colour from: a sprite's atlas texture, or
/// a solid fill (sampled from the platform arm's 1×1 white texture, so the single
/// `tex * color` fragment shader serves both without a per-fragment branch).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QuadSource {
    /// A solid fill — the platform arm binds its 1×1 white texture.
    Solid,
    /// A textured sprite — the platform arm binds the atlas for this texture id.
    Sprite(u64),
}

/// Floats per emitted vertex: position `x,y` (pixels) + UV `u,v` + colour `r,g,b,a`.
/// A layout constant the platform-arm renderer (vertex stride) and the tests
/// (vertex indexing) read; it has no consumer in the default no-GPU build, so it
/// is compiled only where one exists — the same `any(test, …)` gating
/// `surface_recovery` uses.
#[cfg(any(test, all(not(target_arch = "wasm32"), feature = "offscreen")))]
pub(crate) const VERTEX_FLOATS: usize = 8;
/// Vertices per quad (two triangles share the four corners via the index buffer).
pub(crate) const VERTS_PER_QUAD: usize = 4;

/// The backend-neutral 2D geometry for one frame: interleaved quad vertices and
/// one [`QuadSource`] per quad, in painter's order. The platform arm uploads
/// [`Self::vertices`] as a vertex buffer, draws six indices per quad
/// (`0,1,2,0,2,3` offset by `4·q`), and selects each quad's texture from
/// [`Self::sources`].
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct Draw2dGeometry {
    vertices: Vec<f32>,
    sources: Vec<QuadSource>,
}

impl Draw2dGeometry {
    /// The interleaved vertex floats (`VERTEX_FLOATS` per vertex, `VERTS_PER_QUAD`
    /// per quad), in painter's order.
    pub(crate) fn vertices(&self) -> &[f32] {
        &self.vertices
    }

    /// The per-quad colour sources, in painter's order — one per emitted quad.
    pub(crate) fn sources(&self) -> &[QuadSource] {
        &self.sources
    }

    /// The number of emitted quads.
    pub(crate) fn quad_count(&self) -> usize {
        self.sources.len()
    }

    /// Append one quad: its four screen-pixel `corners` (TL, TR, BR, BL — the
    /// `0,1,2,0,2,3` index order), their `uvs`, a flat straight-linear `color`,
    /// and the `source` the platform arm binds.
    fn push_quad(
        &mut self,
        corners: [[f32; 2]; 4],
        uvs: [[f32; 2]; 4],
        color: [f32; 4],
        source: QuadSource,
    ) {
        (0..VERTS_PER_QUAD).for_each(|i| {
            self.vertices.extend_from_slice(&[
                corners[i][0],
                corners[i][1],
                uvs[i][0],
                uvs[i][1],
                color[0],
                color[1],
                color[2],
                color[3],
            ]);
        });
        self.sources.push(source);
    }
}

/// Walk the layer-sorted `list` into quad geometry for a `width`×`height`
/// framebuffer, normalising sprite UVs against `sizes`. The list arrives
/// `(layer, submission)`-sorted, so iterating `commands` in order is the correct
/// painter's order.
pub(crate) fn build_geometry(
    list: &Draw2dList,
    width: u32,
    height: u32,
    sizes: &Draw2dTextureSizes,
) -> Draw2dGeometry {
    let mut geo = Draw2dGeometry::default();
    let camera = camera_matrix(list, width, height);
    list.commands()
        .iter()
        .for_each(|cmd| append_command(&mut geo, &camera, cmd, sizes));
    geo
}

/// The screen transform for the frame's optional [`Camera2d`] — identical to the
/// software backend's: `screen = (world − center)·zoom + viewport_centre`. With no
/// camera the author gets the identity framing (world == framebuffer pixels).
fn camera_matrix(list: &Draw2dList, width: u32, height: u32) -> Mat3 {
    list.camera()
        .map(|c| camera_to_screen(c, width, height))
        .unwrap_or(Mat3::IDENTITY)
}

/// Compose one resolved [`Camera2d`] into its screen matrix (split out so the
/// `Some` arm is a named, testable function).
fn camera_to_screen(c: Camera2d, width: u32, height: u32) -> Mat3 {
    let z = c.zoom.get();
    let half = Vec2::new(width as f32 * 0.5, height as f32 * 0.5);
    Mat3::translation(half)
        .multiply(Mat3::scale(Vec2::new(z, z)))
        .multiply(Mat3::translation(c.center.mul_scalar(-1.0)))
}

/// Emit the quads for one command's supported kinds. Dispatch is branchless: each
/// kind's `as_*` accessor is `Some` only for its own kind, so a non-matching (or
/// deferred) command runs zero iterations.
fn append_command(
    geo: &mut Draw2dGeometry,
    camera: &Mat3,
    cmd: &Draw2dCommand,
    sizes: &Draw2dTextureSizes,
) {
    let m = camera.multiply(cmd.transform());
    let alpha = cmd.alpha().get();
    let fill = resolve_fill(cmd);
    cmd.as_rect()
        .into_iter()
        .for_each(|r| append_rect(geo, &m, r, fill, alpha));
    cmd.as_sprite()
        .into_iter()
        .for_each(|(texture, opts)| append_sprite(geo, &m, texture, opts, alpha, sizes));
}

/// A command's solid fill colour (straight, not yet alpha-folded), or
/// [`TRANSPARENT`] when the fill is absent or a deferred paint/gradient — so an
/// absent/deferred fill emits a no-op-composite quad without a branch.
fn resolve_fill(cmd: &Draw2dCommand) -> [f32; 4] {
    cmd.fill()
        .and_then(|f| f.fill_color)
        .map(Rgba::channels)
        .unwrap_or(TRANSPARENT)
}

/// Emit a filled rect as the quad covering its **transformed axis-aligned
/// bounding box** — matching the software `fill_rect` (which fills the transformed
/// AABB, treating a rotated rect as its AABB approximation). The command alpha is
/// folded into the colour's alpha channel once, here.
fn append_rect(geo: &mut Draw2dGeometry, m: &Mat3, r: Rect, fill: [f32; 4], alpha: f32) {
    let (minx, miny, maxx, maxy) = transformed_bbox(m, r);
    let corners = [[minx, miny], [maxx, miny], [maxx, maxy], [minx, maxy]];
    let color = [fill[0], fill[1], fill[2], fill[3] * alpha];
    geo.push_quad(corners, [[0.0, 0.0]; 4], color, QuadSource::Solid);
}

/// Emit a sprite as the quad covering its transformed dest AABB, with UVs that map
/// the AABB linearly across the source sub-rect (normalised by the atlas size),
/// honouring per-axis flip. The tint folds the command alpha into its alpha
/// channel; the platform arm multiplies the sampled texel by this colour, so
/// `texel.rgb·tint.rgb` and `texel.a·tint.a·alpha` match the software blit. A
/// sprite whose texture size is unknown emits no quad (branchless via `into_iter`).
fn append_sprite(
    geo: &mut Draw2dGeometry,
    m: &Mat3,
    texture: TextureId,
    opts: SpriteDraw2d,
    alpha: f32,
    sizes: &Draw2dTextureSizes,
) {
    sizes.get(texture.raw()).into_iter().for_each(|(tw, th)| {
        let ssize = opts.source.size;
        let origin = Vec2::new(-opts.anchor.x * ssize.x, -opts.anchor.y * ssize.y);
        let dest = Rect::new(origin, ssize);
        let (minx, miny, maxx, maxy) = transformed_bbox(m, dest);
        let corners = [[minx, miny], [maxx, miny], [maxx, maxy], [minx, maxy]];
        let u0 = opts.source.min.x / tw;
        let u1 = (opts.source.min.x + opts.source.size.x) / tw;
        let v0 = opts.source.min.y / th;
        let v1 = (opts.source.min.y + opts.source.size.y) / th;
        let (ua, ub) = [(u0, u1), (u1, u0)][usize::from(opts.flip_x)];
        let (va, vb) = [(v0, v1), (v1, v0)][usize::from(opts.flip_y)];
        let uvs = [[ua, va], [ub, va], [ub, vb], [ua, vb]];
        let tint = opts.tint.channels();
        let color = [tint[0], tint[1], tint[2], tint[3] * alpha];
        geo.push_quad(corners, uvs, color, QuadSource::Sprite(texture.raw()));
    });
}

/// The transformed rect's continuous screen-space bounding box
/// `(minx, miny, maxx, maxy)` — its four corners through `m`, folded to extents.
/// Identical to the software backend's `transformed_bbox`, so the two emit the
/// same screen region.
fn transformed_bbox(m: &Mat3, r: Rect) -> (f32, f32, f32, f32) {
    let mx = r.max();
    [r.min, Vec2::new(mx.x, r.min.y), mx, Vec2::new(r.min.x, mx.y)]
        .iter()
        .map(|c| m.transform_point(*c))
        .fold(
            (
                f32::INFINITY,
                f32::INFINITY,
                f32::NEG_INFINITY,
                f32::NEG_INFINITY,
            ),
            |(mnx, mny, mxx, mxy), p| (mnx.min(p.x), mny.min(p.y), mxx.max(p.x), mxy.max(p.y)),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_host::{Common2d, Fill2d, PaintId};
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

    /// The 8 floats of vertex `v` (0..4) of quad `q` from a geometry's stream.
    fn vert(geo: &Draw2dGeometry, q: usize, v: usize) -> [f32; 8] {
        let base = (q * VERTS_PER_QUAD + v) * VERTEX_FLOATS;
        geo.vertices()[base..base + VERTEX_FLOATS].try_into().unwrap()
    }

    #[test]
    fn rect_emits_one_solid_quad_with_alpha_folded() {
        let mut list = Draw2dList::default();
        list.push_command(Draw2dCommand::rect(
            header(0, 0, 0.5),
            Rect::new(Vec2::new(2.0, 4.0), Vec2::new(6.0, 8.0)),
            Fill2d::color(rgba(1.0, 0.0, 0.0, 1.0)),
        ));
        list.sort_commands();
        let geo = build_geometry(&list, 64, 64, &Draw2dTextureSizes::default());
        assert_eq!(geo.quad_count(), 1);
        assert_eq!(geo.sources(), &[QuadSource::Solid]);
        // Identity transform → AABB corners are the rect itself: (2,4)..(8,12).
        let tl = vert(&geo, 0, 0);
        let br = vert(&geo, 0, 2);
        assert_eq!([tl[0], tl[1]], [2.0, 4.0]);
        assert_eq!([br[0], br[1]], [8.0, 12.0]);
        // Colour is straight red with alpha 1·0.5 folded in.
        assert_eq!([tl[4], tl[5], tl[6], tl[7]], [1.0, 0.0, 0.0, 0.5]);
    }

    #[test]
    fn absent_and_paint_fills_resolve_transparent() {
        // A paint (gradient) fill is deferred → resolves to a transparent quad.
        let mut list = Draw2dList::default();
        list.push_command(Draw2dCommand::rect(
            header(0, 0, 1.0),
            Rect::new(Vec2::ZERO, Vec2::ONE),
            Fill2d::paint(PaintId::from_raw(0)),
        ));
        list.sort_commands();
        let geo = build_geometry(&list, 8, 8, &Draw2dTextureSizes::default());
        assert_eq!(geo.quad_count(), 1);
        let tl = vert(&geo, 0, 0);
        assert_eq!([tl[4], tl[5], tl[6], tl[7]], [0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn deferred_kinds_emit_no_quads() {
        // Circle / line / particle are not in the GPU subset → zero quads.
        let mut list = Draw2dList::default();
        list.push_command(Draw2dCommand::circle(
            header(0, 0, 1.0),
            Vec2::new(4.0, 4.0),
            Meters::new(2.0).unwrap(),
            Fill2d::color(rgba(1.0, 1.0, 1.0, 1.0)),
        ));
        list.push_command(Draw2dCommand::line(
            header(1, 0, 1.0),
            Vec2::ZERO,
            Vec2::new(4.0, 0.0),
            rgba(1.0, 1.0, 1.0, 1.0),
            Meters::new(1.0).unwrap(),
        ));
        list.push_command(Draw2dCommand::particle_quad(
            header(2, 0, 1.0),
            Vec2::new(4.0, 4.0),
            Meters::new(1.0).unwrap(),
            rgba(1.0, 1.0, 1.0, 1.0),
        ));
        list.sort_commands();
        let geo = build_geometry(&list, 8, 8, &Draw2dTextureSizes::default());
        assert_eq!(geo.quad_count(), 0);
        assert!(geo.vertices().is_empty());
    }

    #[test]
    fn list_order_is_painter_order_across_layers() {
        // Submit a layer-2 quad before a layer-1 quad; after the host sort the
        // geometry is emitted layer-1-then-layer-2 (painter's order).
        let mut list = Draw2dList::default();
        list.push_command(Draw2dCommand::rect(
            header(0, 2, 1.0),
            Rect::new(Vec2::ZERO, Vec2::ONE),
            Fill2d::color(rgba(0.0, 0.0, 1.0, 1.0)),
        ));
        list.push_command(Draw2dCommand::rect(
            header(1, 1, 1.0),
            Rect::new(Vec2::ZERO, Vec2::ONE),
            Fill2d::color(rgba(1.0, 0.0, 0.0, 1.0)),
        ));
        list.sort_commands();
        let geo = build_geometry(&list, 8, 8, &Draw2dTextureSizes::default());
        assert_eq!(geo.quad_count(), 2);
        // First quad (painted first) is the layer-1 red; second is the layer-2 blue.
        assert_eq!(vert(&geo, 0, 0)[4..8], [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(vert(&geo, 1, 0)[4..8], [0.0, 0.0, 1.0, 1.0]);
    }

    #[test]
    fn camera_zoom_and_center_place_the_quad() {
        let mut list = Draw2dList::default();
        list.set_camera(Camera2d::new(Vec2::ZERO, ratio(2.0)));
        list.push_command(Draw2dCommand::rect(
            header(0, 0, 1.0),
            Rect::new(Vec2::ZERO, Vec2::ONE),
            Fill2d::color(rgba(0.0, 1.0, 0.0, 1.0)),
        ));
        list.sort_commands();
        // 8×8 → centre (4,4); zoom 2 maps world (0,0)→(4,4), (1,1)→(6,6).
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
        // Dest AABB is the 2×2 source at the origin; UVs span the full atlas 0..1.
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
        // Both axes flipped: TL corner now samples atlas (1,1), BR samples (0,0).
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
        // tint green, command alpha 0.5 → colour (0,1,0,0.5).
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
        // Debug + Clone + PartialEq are exercised (derive coverage).
        assert_eq!(sizes.clone(), sizes);
        assert!(format!("{sizes:?}").contains("Draw2dTextureSizes"));
    }

    #[test]
    fn empty_list_yields_empty_geometry() {
        let geo = build_geometry(&Draw2dList::default(), 8, 8, &Draw2dTextureSizes::default());
        assert_eq!(geo, Draw2dGeometry::default());
        assert_eq!(geo.quad_count(), 0);
        assert!(format!("{geo:?}").contains("Draw2dGeometry"));
        // QuadSource derives are exercised.
        assert_eq!(QuadSource::Solid, QuadSource::Solid);
        assert_ne!(QuadSource::Solid, QuadSource::Sprite(1));
        assert!(format!("{:?}", QuadSource::Sprite(1)).contains("Sprite"));
    }
}
