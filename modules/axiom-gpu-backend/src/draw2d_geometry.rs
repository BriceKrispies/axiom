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
//! ## Scope — shape parity with the software subset
//! The GPU 2D arm is at **shape parity** with the software rasterizer's filled,
//! alpha-composited subset: **rect** (its transformed axis-aligned bounding box),
//! **sprite** (an atlas source-rect blit with tint, per-axis flip, and alpha),
//! **circle** and **ellipse** (rotation-exact via conjugate semi-diameters),
//! **line** (a rounded capsule with stroke width), and **particle-quad** (a
//! centred square — the same `fill_rect` the software path emits). The deferred
//! kinds — **path**, **text**, gradient/**paint** fills, and per-shape **strokes**
//! (the software backend draws strokes; the GPU subset, like its rect path, fills
//! only for now) — are **not** emitted here (their `as_*` accessor is never
//! queried). A command of a non-emitted kind contributes zero quads, branchlessly.
//!
//! ## How the round shapes stay byte-exact (analytic coverage, not tessellation)
//! A circle/ellipse/line cannot be a hard-edged polygon fan and still match the
//! software backend's **exact per-pixel** coverage (`s²+t²≤1` for a conic, capsule
//! distance for a line) within the ±2 quantization budget — a polygon edge
//! disagrees with the true curve at boundary pixels by the shape's full colour. So
//! each round shape emits **one quad over its bounding region** carrying, per
//! vertex, an *analytic coverage field*: the conic basis coordinate `(s, t)` for a
//! circle/ellipse, or the line-local `(along, perp, length, half_width)` for a
//! line. The platform (wgpu) arm's fragment shader interpolates that field (affine,
//! so exact at every pixel centre) and `discard`s the fragment when the *same* test
//! the software path runs fails. The covered core therefore owns all the coverage
//! math — the shader only evaluates it — keeping this file the single parity source
//! of truth, branchless, and fully covered on native.
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

/// Smallest line length (screen px) treated as non-degenerate; a shorter segment
/// has no direction, so its capsule collapses to a round dot of `half_width`
/// radius — matching the software `raster_line`'s `EPS`-floored `len²`.
const EPS: f32 = 1e-6;

/// The coverage `field` for a shape that is always fully inside its quad (rect,
/// sprite, particle): the platform shader's capsule test reads
/// `(along, perp, length, half_width)` = `(0, 0, 0, HUGE)`, so the distance `0`
/// is always `≤ HUGE²` and nothing is discarded. `HUGE` is finite (its square does
/// not overflow `f32`) so the comparison is a plain number, never a `NaN`.
const PLAIN_FIELD: [f32; 4] = [0.0, 0.0, 0.0, 1.0e18];

/// Coverage `kind` selecting the platform shader's **capsule** distance test —
/// used by the always-inside plain quads and by the line (a real rounded capsule).
const KIND_CAPSULE: f32 = 0.0;
/// Coverage `kind` selecting the platform shader's **conic** test (`s²+t²≤1`) —
/// used by circle and ellipse.
const KIND_CONIC: f32 = 1.0;

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

/// Floats per emitted vertex: position `x,y` (pixels) + UV `u,v` + colour
/// `r,g,b,a` + analytic coverage `field` (`vec4`) + coverage `kind` (`f32`). The
/// `field`/`kind` feed the platform shader's per-pixel discard test (conic or
/// capsule); a plain rect/sprite/particle carries the always-inside [`PLAIN_FIELD`]
/// with [`KIND_CAPSULE`]. A layout constant the platform-arm renderer (vertex
/// stride) and the tests (vertex indexing) read; it has no consumer in the default
/// no-GPU build, so it is compiled only where one exists — the same `any(test, …)`
/// gating `surface_recovery` uses.
#[cfg(any(test, all(not(target_arch = "wasm32"), feature = "offscreen")))]
pub(crate) const VERTEX_FLOATS: usize = 13;
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
    /// `0,1,2,0,2,3` index order), their `uvs`, per-corner analytic coverage
    /// `fields` (interpolated by the platform shader for its discard test), the
    /// coverage `kind` (capsule or conic — flat across the quad), a flat
    /// straight-linear `color`, and the `source` the platform arm binds.
    fn push_quad(
        &mut self,
        corners: [[f32; 2]; 4],
        uvs: [[f32; 2]; 4],
        fields: [[f32; 4]; 4],
        kind: f32,
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
                fields[i][0],
                fields[i][1],
                fields[i][2],
                fields[i][3],
                kind,
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
    cmd.as_circle().into_iter().for_each(|(center, radius)| {
        let ax = m.transform_vector(Vec2::new(radius.get(), 0.0));
        let ay = m.transform_vector(Vec2::new(0.0, radius.get()));
        append_conic(geo, m.transform_point(center), ax, ay, fill, alpha);
    });
    cmd.as_ellipse()
        .into_iter()
        .for_each(|(center, rx, ry, rotation)| {
            let local = Mat3::rotation(rotation);
            let ax = m.transform_vector(local.transform_vector(Vec2::new(rx.get(), 0.0)));
            let ay = m.transform_vector(local.transform_vector(Vec2::new(0.0, ry.get())));
            append_conic(geo, m.transform_point(center), ax, ay, fill, alpha);
        });
    cmd.as_line().into_iter().for_each(|(a, b, color, width)| {
        append_line(geo, &m, a, b, color.channels(), width.get(), alpha);
    });
    cmd.as_particle().into_iter().for_each(|(center, size, color)| {
        let h = size.get();
        let quad = Rect::new(center.subtract(Vec2::new(h, h)), Vec2::new(2.0 * h, 2.0 * h));
        append_rect(geo, &m, quad, color.channels(), alpha);
    });
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
    geo.push_quad(
        corners,
        [[0.0, 0.0]; 4],
        [PLAIN_FIELD; 4],
        KIND_CAPSULE,
        color,
        QuadSource::Solid,
    );
}

/// Emit a filled circle/ellipse as the quad covering its conic **bounding box**,
/// carrying per-corner conic-basis coordinates `(s, t)` so the platform shader can
/// run the software backend's exact `s²+t²≤1` test per pixel and discard the
/// outside. `center`, `ax`, `ay` are the screen-space centre and the two conjugate
/// semi-diameter vectors — identical to the software `fill_conic` inputs, so the
/// two emit the same disc. A degenerate (zero-area) basis makes `det` zero, so
/// every corner `(s, t)` is non-finite and the shader's `s²+t²` test is `false`
/// everywhere — nothing draws, matching the software path, with no branch here.
fn append_conic(geo: &mut Draw2dGeometry, center: Vec2, ax: Vec2, ay: Vec2, fill: [f32; 4], alpha: f32) {
    let hx = (ax.x * ax.x + ay.x * ay.x).sqrt();
    let hy = (ax.y * ax.y + ay.y * ay.y).sqrt();
    let corners = [
        [center.x - hx, center.y - hy],
        [center.x + hx, center.y - hy],
        [center.x + hx, center.y + hy],
        [center.x - hx, center.y + hy],
    ];
    let det = ax.x * ay.y - ay.x * ax.y;
    let fields = corners.map(|c| {
        let dx = c[0] - center.x;
        let dy = c[1] - center.y;
        [
            (ay.y * dx - ay.x * dy) / det,
            (ax.x * dy - ax.y * dx) / det,
            0.0,
            0.0,
        ]
    });
    let color = [fill[0], fill[1], fill[2], fill[3] * alpha];
    geo.push_quad(corners, [[0.0, 0.0]; 4], fields, KIND_CONIC, color, QuadSource::Solid);
}

/// Emit a stroked line `a`–`b` (both through `m`) as **one** quad covering its
/// rounded-capsule bounding rectangle (the oriented segment extended by the
/// screen-space half-width `0.5·|m·(width,0)|` on every side). Each corner carries
/// the line-local coverage field `(along, perp, length, half_width)` so the
/// platform shader reproduces the software `raster_line` capsule distance
/// (`perp² + (along−clamp(along,0,length))² ≤ half_width²`) per pixel — including
/// the round caps — and discards the rectangle's four outside corners. A
/// zero-length segment has no direction, so the basis falls back to the axis-aligned
/// unit frame (branchless via `usize::from`) and the capsule collapses to a round
/// dot of `half_width` radius — the same shape the software `EPS`-floored `len²`
/// yields.
fn append_line(geo: &mut Draw2dGeometry, m: &Mat3, a: Vec2, b: Vec2, color: [f32; 4], width: f32, alpha: f32) {
    let pa = m.transform_point(a);
    let pb = m.transform_point(b);
    let half_w = 0.5 * m.transform_vector(Vec2::new(width, 0.0)).length();
    let ab = pb.subtract(pa);
    let len = ab.length();
    let inv = 1.0 / len.max(EPS);
    // When `len < EPS` the direction is undefined; add the unit-x basis so the
    // frame falls back to axis-aligned (`u = (1,0)`, `n = (0,1)`) without a branch.
    let degenerate = usize::from(len < EPS) as f32;
    let u = Vec2::new(ab.x * inv + degenerate, ab.y * inv);
    let n = Vec2::new(-u.y, u.x);
    let ext_a = pa.subtract(u.mul_scalar(half_w));
    let ext_b = pb.add(u.mul_scalar(half_w));
    let off = n.mul_scalar(half_w);
    let corners = [
        point(ext_a.subtract(off)),
        point(ext_b.subtract(off)),
        point(ext_b.add(off)),
        point(ext_a.add(off)),
    ];
    let fields = corners.map(|c| {
        let d = Vec2::new(c[0] - pa.x, c[1] - pa.y);
        [d.dot(u), d.dot(n), len, half_w]
    });
    let col = [color[0], color[1], color[2], color[3] * alpha];
    geo.push_quad(corners, [[0.0, 0.0]; 4], fields, KIND_CAPSULE, col, QuadSource::Solid);
}

/// A [`Vec2`] as a two-element corner array.
fn point(v: Vec2) -> [f32; 2] {
    [v.x, v.y]
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
        geo.push_quad(
            corners,
            uvs,
            [PLAIN_FIELD; 4],
            KIND_CAPSULE,
            color,
            QuadSource::Sprite(texture.raw()),
        );
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
    use axiom_kernel::{Meters, Radians, Ratio};

    fn ratio(v: f32) -> Ratio {
        Ratio::new(v).unwrap()
    }

    fn rgba(r: f32, g: f32, b: f32, a: f32) -> Rgba {
        Rgba::new(ratio(r), ratio(g), ratio(b), ratio(a))
    }

    fn header(submission: u32, layer: i32, alpha: f32) -> (u32, Mat3, Common2d) {
        (submission, Mat3::IDENTITY, Common2d::new(layer, ratio(alpha)))
    }

    /// The `VERTEX_FLOATS` floats of vertex `v` (0..4) of quad `q`: position `x,y`
    /// (0,1), UV `u,v` (2,3), colour `r,g,b,a` (4..8), coverage field (8..12),
    /// coverage kind (12).
    fn vert(geo: &Draw2dGeometry, q: usize, v: usize) -> [f32; VERTEX_FLOATS] {
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
    fn path_kind_is_deferred_and_emits_no_quads() {
        // Path / text / paint fills are still deferred → zero quads (their `as_*`
        // accessor is never queried).
        let mut list = Draw2dList::default();
        list.push_command(Draw2dCommand::path(
            header(0, 0, 1.0),
            vec![Vec2::ZERO, Vec2::new(4.0, 0.0), Vec2::new(4.0, 4.0)],
            Fill2d::color(rgba(1.0, 1.0, 1.0, 1.0)),
            true,
        ));
        list.sort_commands();
        let geo = build_geometry(&list, 8, 8, &Draw2dTextureSizes::default());
        assert_eq!(geo.quad_count(), 0);
        assert!(geo.vertices().is_empty());
    }

    #[test]
    fn circle_emits_one_conic_quad_with_basis_coords() {
        // A radius-2 circle at (4,4), identity transform: ax=(2,0), ay=(0,2),
        // det=4. The bounding quad is (2,2)..(6,6); each corner's (s,t) is the
        // unit-square corner the shader tests against `s²+t²≤1`.
        let mut list = Draw2dList::default();
        list.push_command(Draw2dCommand::circle(
            header(0, 0, 0.5),
            Vec2::new(4.0, 4.0),
            Meters::new(2.0).unwrap(),
            Fill2d::color(rgba(1.0, 0.0, 0.0, 1.0)),
        ));
        list.sort_commands();
        let geo = build_geometry(&list, 8, 8, &Draw2dTextureSizes::default());
        assert_eq!(geo.quad_count(), 1);
        assert_eq!(geo.sources(), &[QuadSource::Solid]);
        let tl = vert(&geo, 0, 0);
        let br = vert(&geo, 0, 2);
        // Bounding-box corners.
        assert_eq!([tl[0], tl[1]], [2.0, 4.0 - 2.0]);
        assert_eq!([br[0], br[1]], [6.0, 6.0]);
        // Colour: straight red, alpha 1·0.5 folded.
        assert_eq!([tl[4], tl[5], tl[6], tl[7]], [1.0, 0.0, 0.0, 0.5]);
        // Conic basis at the corners is the unit square; kind selects the conic test.
        assert_eq!([tl[8], tl[9]], [-1.0, -1.0]);
        assert_eq!([br[8], br[9]], [1.0, 1.0]);
        assert_eq!(tl[12], KIND_CONIC);
    }

    #[test]
    fn ellipse_rotation_orients_the_bounding_box() {
        // rx=4, ry=2 rotated 90°: the rotated long axis is vertical, so the
        // bounding box is 2 wide and 4 tall (half_x=2, half_y=4) around (8,8).
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
        assert_eq!(geo.quad_count(), 1);
        let tl = vert(&geo, 0, 0);
        let br = vert(&geo, 0, 2);
        // half_x≈2, half_y≈4 → corners (6,4)..(10,12).
        assert!((tl[0] - 6.0).abs() < 1e-4);
        assert!((tl[1] - 4.0).abs() < 1e-4);
        assert!((br[0] - 10.0).abs() < 1e-4);
        assert!((br[1] - 12.0).abs() < 1e-4);
        assert_eq!(tl[12], KIND_CONIC);
    }

    #[test]
    fn line_emits_one_capsule_quad_with_local_coords() {
        // A width-2 horizontal line (1,8)->(14,8), identity: half_w=1, len=13,
        // u=(1,0), n=(0,1). The bounding rect extends ±1 each way: (0,7)..(15,9).
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
        assert_eq!(geo.sources(), &[QuadSource::Solid]);
        let tl = vert(&geo, 0, 0);
        let br = vert(&geo, 0, 2);
        assert_eq!([tl[0], tl[1]], [0.0, 7.0]);
        assert_eq!([br[0], br[1]], [15.0, 9.0]);
        // Field at the back-left corner: along=-1 (a half-width behind pa),
        // perp=-1, length=13, half_width=1; kind is the capsule test.
        assert_eq!([tl[8], tl[9], tl[10], tl[11]], [-1.0, -1.0, 13.0, 1.0]);
        assert_eq!(tl[12], KIND_CAPSULE);
        // Colour folded with alpha 1.
        assert_eq!([tl[4], tl[5], tl[6], tl[7]], [1.0, 1.0, 0.0, 1.0]);
    }

    #[test]
    fn zero_length_line_falls_back_to_an_axis_aligned_dot() {
        // a==b under a zero-scale transform: len<EPS triggers the axis-aligned
        // fallback; half_w=0 collapses the quad to the origin and the capsule to a
        // zero dot — a finite, no-op quad (the degenerate-length branch).
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
        // Every corner collapsed to the origin; length 0, half_width 0.
        assert_eq!([tl[0], tl[1]], [0.0, 0.0]);
        assert_eq!([tl[10], tl[11]], [0.0, 0.0]);
        assert_eq!(tl[12], KIND_CAPSULE);
    }

    #[test]
    fn particle_emits_a_centred_plain_quad() {
        // A particle of half-extent 2 at (8,8) is the (6,6)..(10,10) square — the
        // same `fill_rect` the software path emits, so it is a plain capsule quad.
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
        // Plain coverage: kind capsule, field is the always-inside PLAIN_FIELD.
        assert_eq!(tl[12], KIND_CAPSULE);
        assert_eq!([tl[8], tl[9], tl[10], tl[11]], PLAIN_FIELD);
        assert_eq!([tl[4], tl[5], tl[6], tl[7]], [0.0, 1.0, 1.0, 1.0]);
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
