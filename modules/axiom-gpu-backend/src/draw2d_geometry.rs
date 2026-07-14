//! The **covered core** of the GPU 2D raster arm: walk a layer-sorted
//! [`axiom_host::Draw2dList`] into backend-neutral quad geometry the platform
//! (wgpu) arm uploads and draws.
//! This is the GPU peer of the canvas backend's `draw2d_raster` *list walk*: it
//! resolves each command's baked [`Mat3`] transform (composed with the frame's
//! optional [`axiom_host::Camera2d`]), folds the command `alpha` into the source
//! colour, and emits one or more axis-aligned quads per command — **in list
//! order**, which is already the host's `(layer, submission)` painter's order, so
//! a later quad composites over an earlier one on the GPU exactly as the software
//! `composite_pixel` paints later commands over earlier ones.
//! ## Scope — full SPEC-04 2D parity with the software rasterizer
//! Every [`axiom_host::Draw2dCommand`] kind the software backend rasterizes is
//! emitted here, at byte-parity (within the established ±2 tolerance):
//! **rect** / **circle** / **ellipse** / **line** / **particle-quad** / **sprite**
//! (the original subset), plus **path** (an arbitrary polygon, fan-triangulated
//! into per-triangle barycentric-coverage quads), **text glyph runs** (each glyph
//! laid out and emitted through the sprite path against the font atlas), per-shape
//! **strokes** (rect border, circle/ellipse annulus, polyline path edges — the
//! GPU peer of the software stroke arm), and **gradient/paint fills** (linear and
//! radial: the contract's canonical gradient texture, sampled with a per-vertex
//! affine UV — see "How gradients stay parity-exact" below).
//! ## How the round shapes / strokes / polygons stay byte-exact (analytic coverage)
//! A circle/ellipse/line/polygon cannot be a hard-edged polygon fan and still
//! match the software backend's **exact per-pixel** coverage within the ±2
//! quantization budget. So each shape emits **one quad over its bounding region**
//! carrying, per vertex, an *analytic coverage field*, and the platform (wgpu)
//! arm's fragment shader interpolates that field (affine, so exact at every pixel
//! centre) and `discard`s the fragment when the *same* test the software path runs
//! fails. The coverage `kind` selects the test:
//! * `CAPSULE` — line capsule distance (and the always-inside plain rect/sprite/
//!   particle, which carry [`PLAIN_FIELD`]).
//! * `CONIC` — circle/ellipse fill (`s²+t² ≤ 1`).
//! * `TRI` — a path fan triangle, via barycentric coordinates (`min(b₀,b₁,b₂) ≥ 0`).
//! * `RECT_STROKE` — a rect's inset border band (`min(edge distances)/width < 1`).
//! * `CONIC_STROKE` — a circle/ellipse annulus (`inner² ≤ s²+t² ≤ 1`).
//! The covered core owns all the coverage math — the shader only evaluates it —
//! keeping this file the single parity source of truth, branchless, and fully
//! covered on native.
//! ## How gradients stay parity-exact (canonical texture + affine UV)
//! A gradient fill resolves to the contract's **canonical gradient texture**
//! ([`axiom_host::Draw2dList::paint_texture`]) — an `n×1` colour ramp for a linear
//! gradient, an `n×n` disc for a radial one — registered for the platform arm to
//! upload, and the filled shape's quad carries a **per-vertex UV** that is affine
//! in screen position (the linear projection parameter, or `((p−centre)/radius)`),
//! so the interpolated UV at every pixel centre is exact. The software backend
//! samples the *same* host-baked texture with the *same* nearest rule, so a
//! gradient fill is byte-identical across backends. The shape's own coverage
//! (`CONIC`/`TRI`/plain) is untouched — gradient is purely the colour source.
//! ## Coordinate model
//! Quad positions are **framebuffer pixels** (top-left origin), the same space the
//! software rasterizer reasons in; the platform arm's vertex shader converts them
//! to NDC. Colours are straight linear RGBA with the command alpha folded into the
//! alpha channel, so the GPU's `ALPHA_BLENDING` reproduces the software `over()`
//! blend.
//! Pure Rust — no wgpu, no browser types — so it builds, is branchless, and is
//! fully covered on native exactly as on wasm.

use std::collections::HashMap;

use axiom_host::{
    Camera2d, Draw2dCommand, Draw2dList, GlyphRun, Rect, SpriteDraw2d, TextDraw2d, TextureId,
};
use axiom_math::{Mat3, Vec2};

/// The fully-transparent colour an absent / unresolvable-paint fill resolves to:
/// emitting a quad with alpha 0 composites nothing, so a "no fill" command draws
/// nothing without a branch — the same no-op-composite the software path relies on.
const TRANSPARENT: [f32; 4] = [0.0, 0.0, 0.0, 0.0];

/// Smallest non-degenerate magnitude (screen px / length²) used to floor a divide
/// so a degenerate segment / gradient axis / glyph cell stays finite — the GPU
/// peer of the software backend's `EPS`.
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
/// used by circle and ellipse fills.
const KIND_CONIC: f32 = 1.0;
/// Coverage `kind` selecting the platform shader's **barycentric** triangle test
/// (`min(b₀,b₁,b₂) ≥ 0`) — used by path fan triangles.
const KIND_TRI: f32 = 2.0;
/// Coverage `kind` selecting the platform shader's **rect border** test
/// (`min(edge distances)/width < 1`) — used by a rect's inset stroke band.
const KIND_RECT_STROKE: f32 = 3.0;
/// Coverage `kind` selecting the platform shader's **conic annulus** test
/// (`inner² ≤ s²+t² ≤ 1`) — used by a circle/ellipse stroke.
const KIND_CONIC_STROKE: f32 = 4.0;

/// Resolution of a baked gradient texture: a linear ramp is `RAMP_N×1`, a radial
/// disc is `RAMP_N×RAMP_N`. Parity does not depend on this value (both backends
/// sample the identical host-baked texture); it only sets the gradient's banding
/// fidelity.
const RAMP_N: u32 = 512;

/// Reserved [`TextureId`] base for baked gradient ramp textures: a paint's ramp is
/// uploaded under `GRADIENT_TEXTURE_BASE + paint_id`. Held below the font atlas
/// (`0x00F0_0000`) and above normal sprite / render-target ids so it collides with
/// neither.
const GRADIENT_TEXTURE_BASE: u64 = 0x00E0_0000;

/// Floats per emitted vertex: position `x,y` (pixels) + UV `u,v` + colour
/// `r,g,b,a` + analytic coverage `field` (`vec4`) + coverage `kind` (`f32`). A
/// layout constant the platform-arm renderer (vertex stride) and the tests (vertex
/// indexing) read; it has no consumer in the default no-GPU build, so it is
/// compiled only where the wgpu renderer that reads it exists — the live wasm32
/// arm or the native `offscreen` feature (plus `test`), matching `draw2d_renderer`'s
/// own `cfg`.
#[cfg(any(test, target_arch = "wasm32", feature = "offscreen"))]
pub(crate) const VERTEX_FLOATS: usize = 13;
/// Vertices per quad (two triangles share the four corners via the index buffer).
pub(crate) const VERTS_PER_QUAD: usize = 4;

/// Sprite/atlas texture **dimensions** keyed by the contract's [`TextureId`] raw
/// value — the GPU peer of the canvas backend's `Draw2dTextures`, but holding only
/// sizes (the pixels live in GPU textures the platform arm uploads). Used to
/// normalise a sprite's (and a glyph's) source sub-rect into `0..1` UVs. A sprite
/// naming an unknown id emits no quad (branchless via `get` → `into_iter`), the
/// same skip the software path makes for an unknown texture.
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
    /// A textured sprite / glyph atlas / gradient ramp — the platform arm binds
    /// the texture for this id.
    Sprite(u64),
}

/// A resolved fill's colour **source**: a solid colour, or a gradient sampling a
/// baked ramp texture by an affine per-vertex UV. Carried as one `Copy` value so
/// the shape emitters stay within a small argument count and a fill is computed
/// once per command, not per shape arm.
#[derive(Debug, Clone, Copy, PartialEq)]
struct FillPaint {
    /// The texture the platform arm binds (white for solid, the ramp for gradient).
    source: QuadSource,
    /// Straight RGBA the texel is modulated by — the solid colour, or white
    /// `(1,1,1,1)` for a gradient (the ramp carries the colour).
    color: [f32; 4],
    /// UV mode: `0` solid (constant `(0,0)`), `1` linear, `2` radial.
    mode: usize,
    /// Linear: gradient start (screen space) and direction; `inv_len2 = 1/|dir|²`.
    from: Vec2,
    dir: Vec2,
    inv_len2: f32,
    /// Radial: gradient centre (screen space) and `1/radius` (screen scale).
    center: Vec2,
    inv_radius: f32,
}

impl FillPaint {
    /// A solid-colour fill (white texture, constant UV).
    fn solid(color: [f32; 4]) -> Self {
        FillPaint {
            source: QuadSource::Solid,
            color,
            mode: 0,
            from: Vec2::ZERO,
            dir: Vec2::ZERO,
            inv_len2: 0.0,
            center: Vec2::ZERO,
            inv_radius: 0.0,
        }
    }

    /// A linear-gradient fill sampling ramp `ramp_id` along the screen-space axis
    /// `from`→`from+dir`.
    fn linear(ramp_id: u64, from: Vec2, dir: Vec2, inv_len2: f32) -> Self {
        FillPaint {
            source: QuadSource::Sprite(ramp_id),
            color: [1.0, 1.0, 1.0, 1.0],
            mode: 1,
            from,
            dir,
            inv_len2,
            center: Vec2::ZERO,
            inv_radius: 0.0,
        }
    }

    /// A radial-gradient fill sampling the `n×n` ramp `ramp_id` around the
    /// screen-space `center` with `1/radius` scale.
    fn radial(ramp_id: u64, center: Vec2, inv_radius: f32) -> Self {
        FillPaint {
            source: QuadSource::Sprite(ramp_id),
            color: [1.0, 1.0, 1.0, 1.0],
            mode: 2,
            from: Vec2::ZERO,
            dir: Vec2::ZERO,
            inv_len2: 0.0,
            center,
            inv_radius,
        }
    }

    /// The sampling UV for a corner at screen position `p`. Branchless: linear and
    /// radial parameters are both computed and the `mode` table-indexes the
    /// result, so the unused arm's value (possibly non-finite) is never selected.
    /// The linear param and the radial `((p−c)/r)` are affine in `p`, so the
    /// platform arm's interpolation of these per-vertex UVs is exact per pixel.
    fn uv(&self, p: Vec2) -> [f32; 2] {
        let lin = p.subtract(self.from).dot(self.dir) * self.inv_len2;
        let d = p.subtract(self.center).mul_scalar(self.inv_radius);
        [[0.0, 0.0], [lin, 0.5], [d.x * 0.5 + 0.5, d.y * 0.5 + 0.5]][self.mode]
    }
}

/// A resolved stroke: its straight colour and world-space width (the screen width
/// is derived per shape from the transform). Absent stroke resolves to a
/// transparent colour and zero width, so the stroke quad composites nothing
/// without a branch.
#[derive(Debug, Clone, Copy, PartialEq)]
struct StrokeStyle {
    color: [f32; 4],
    width: f32,
}

impl StrokeStyle {
    fn resolve(cmd: &Draw2dCommand) -> StrokeStyle {
        cmd.fill()
            .and_then(|f| f.stroke)
            .map(|s| StrokeStyle {
                color: s.color.channels(),
                width: s.width.get(),
            })
            .unwrap_or(StrokeStyle {
                color: TRANSPARENT,
                width: 0.0,
            })
    }
}

/// The backend-neutral 2D geometry for one frame: interleaved quad vertices, one
/// [`QuadSource`] per quad (painter's order), and the baked gradient ramp textures
/// the platform arm must upload before drawing. The platform arm uploads
/// [`Self::vertices`] as a vertex buffer, draws six indices per quad
/// (`0,1,2,0,2,3` offset by `4·q`), selects each quad's texture from
/// [`Self::sources`], and uploads [`Self::gradient_textures`] alongside the app's
/// sprite atlases.
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct Draw2dGeometry {
    vertices: Vec<f32>,
    sources: Vec<QuadSource>,
    gradient_textures: HashMap<u64, (u32, u32, Vec<u8>)>,
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

    /// The baked gradient ramp textures `(id, width, height, RGBA8)` the platform
    /// arm uploads (in id order, so the set is deterministic) before drawing the
    /// gradient-filled quads that bind them. Consumed only by the off-screen /
    /// browser platform arm (and the tests), so it is compiled only where one
    /// exists — the same `any(test, …)` gating `VERTEX_FLOATS` uses.
    #[cfg(any(test, all(not(target_arch = "wasm32"), feature = "offscreen")))]
    pub(crate) fn gradient_textures(&self) -> Vec<(u64, u32, u32, Vec<u8>)> {
        let mut out: Vec<(u64, u32, u32, Vec<u8>)> = self
            .gradient_textures
            .iter()
            .map(|(id, (w, h, bytes))| (*id, *w, *h, bytes.clone()))
            .collect();
        out.sort_by_key(|(id, _, _, _)| *id);
        out
    }

    /// Register a baked gradient ramp texture for the platform arm to upload
    /// (idempotent: re-registering the same id overwrites identical bytes).
    fn add_gradient_texture(&mut self, id: u64, width: u32, height: u32, bytes: Vec<u8>) {
        self.gradient_textures.insert(id, (width, height, bytes));
    }

    /// Append one quad: its four screen-pixel `corners` (TL, TR, BR, BL — the
    /// `0,1,2,0,2,3` index order), their `uvs`, per-corner analytic coverage
    /// `fields`, the coverage `kind`, a flat straight-linear `color`, and the
    /// `source` the platform arm binds.
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

    /// Push a fill quad: the filled shape's `corners`, its coverage `fields`/`kind`,
    /// the resolved [`FillPaint`] (which supplies the per-corner UV, the quad's
    /// colour source, and the modulating colour), and the command `alpha` folded
    /// once into the colour's alpha channel.
    fn push_fill(
        &mut self,
        fill: &FillPaint,
        corners: [Vec2; 4],
        fields: [[f32; 4]; 4],
        kind: f32,
        alpha: f32,
    ) {
        let positions = corners.map(|c| [c.x, c.y]);
        let uvs = corners.map(|c| fill.uv(c));
        let color = [
            fill.color[0],
            fill.color[1],
            fill.color[2],
            fill.color[3] * alpha,
        ];
        self.push_quad(positions, uvs, fields, kind, color, fill.source);
    }

    /// Push a solid-colour stroke quad (no gradient): `corners`, coverage
    /// `fields`/`kind`, the straight stroke `color`, and the command `alpha`.
    fn push_solid(
        &mut self,
        corners: [Vec2; 4],
        fields: [[f32; 4]; 4],
        kind: f32,
        color: [f32; 4],
        alpha: f32,
    ) {
        let positions = corners.map(|c| [c.x, c.y]);
        let folded = [color[0], color[1], color[2], color[3] * alpha];
        self.push_quad(
            positions,
            [[0.0, 0.0]; 4],
            fields,
            kind,
            folded,
            QuadSource::Solid,
        );
    }
}

/// Walk the layer-sorted `list` into quad geometry for a `width`×`height`
/// framebuffer, normalising sprite/glyph UVs against `sizes` and resolving
/// gradient fills against the list's paint table. The list arrives
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
        .for_each(|cmd| append_command(&mut geo, &camera, cmd, sizes, list));
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

/// Emit the quads for one command's kind. Dispatch is branchless: each kind's
/// `as_*` accessor is `Some` only for its own kind, so a non-matching command runs
/// zero iterations.
fn append_command(
    geo: &mut Draw2dGeometry,
    camera: &Mat3,
    cmd: &Draw2dCommand,
    sizes: &Draw2dTextureSizes,
    list: &Draw2dList,
) {
    let m = camera.multiply(cmd.transform());
    let alpha = cmd.alpha().get();
    let fill = resolve_fill(geo, &m, cmd, list);
    let stroke = StrokeStyle::resolve(cmd);
    cmd.as_rect().into_iter().for_each(|r| {
        append_rect(geo, &m, r, &fill, alpha);
        append_rect_stroke(geo, &m, r, &stroke, alpha);
    });
    cmd.as_circle().into_iter().for_each(|(center, radius)| {
        let ax = m.transform_vector(Vec2::new(radius.get(), 0.0));
        let ay = m.transform_vector(Vec2::new(0.0, radius.get()));
        append_conic(
            geo,
            m.transform_point(center),
            ax,
            ay,
            &fill,
            &stroke,
            alpha,
        );
    });
    cmd.as_ellipse()
        .into_iter()
        .for_each(|(center, rx, ry, rotation)| {
            let local = Mat3::rotation(rotation);
            let ax = m.transform_vector(local.transform_vector(Vec2::new(rx.get(), 0.0)));
            let ay = m.transform_vector(local.transform_vector(Vec2::new(0.0, ry.get())));
            append_conic(
                geo,
                m.transform_point(center),
                ax,
                ay,
                &fill,
                &stroke,
                alpha,
            );
        });
    cmd.as_line().into_iter().for_each(|(a, b, color, width)| {
        append_line(geo, &m, a, b, color.channels(), width.get(), alpha);
    });
    cmd.as_path().into_iter().for_each(|(points, closed)| {
        append_path(geo, &m, &points, closed, &fill, &stroke, alpha);
    });
    cmd.as_particle()
        .into_iter()
        .for_each(|(center, size, color)| {
            let h = size.get();
            let quad = Rect::new(
                center.subtract(Vec2::new(h, h)),
                Vec2::new(2.0 * h, 2.0 * h),
            );
            append_rect(geo, &m, quad, &FillPaint::solid(color.channels()), alpha);
        });
    cmd.as_sprite()
        .into_iter()
        .for_each(|(texture, opts)| append_sprite(geo, &m, texture, opts, alpha, sizes));
    cmd.as_text()
        .into_iter()
        .for_each(|(run, opts)| append_text(geo, &m, &run, opts, alpha, sizes));
}

/// Resolve a command's fill into a [`FillPaint`]: a solid colour, a gradient
/// sampling a baked ramp (registered on `geo` for upload), or — for an absent or
/// unresolvable-paint fill — a transparent solid (a no-op composite). Branchless:
/// the solid and the (mutually exclusive) gradient arms are each an `Option`, and
/// `Option::or` picks the present one with no control flow.
fn resolve_fill(
    geo: &mut Draw2dGeometry,
    m: &Mat3,
    cmd: &Draw2dCommand,
    list: &Draw2dList,
) -> FillPaint {
    let style = cmd.fill();
    let solid = style
        .and_then(|f| f.fill_color)
        .map(|c| FillPaint::solid(c.channels()));
    let gradient = style
        .and_then(|f| f.fill_paint)
        .and_then(|id| build_gradient_fill(geo, m, id, list));
    solid.or(gradient).unwrap_or(FillPaint::solid(TRANSPARENT))
}

/// Build the gradient [`FillPaint`] for paint `id`, registering its baked ramp
/// texture on `geo` for the platform arm to upload. `None` if the id names no
/// paint (so the caller falls back to a transparent fill, matching the software
/// path). The linear/radial geometry is transformed into screen space by `m`, so
/// the gradient moves with the shape it fills.
fn build_gradient_fill(
    geo: &mut Draw2dGeometry,
    m: &Mat3,
    id: axiom_host::PaintId,
    list: &Draw2dList,
) -> Option<FillPaint> {
    list.paint_texture(id, RAMP_N).and_then(|(w, h, bytes)| {
        let ramp_id = GRADIENT_TEXTURE_BASE + u64::from(id.raw());
        geo.add_gradient_texture(ramp_id, w, h, bytes);
        let linear = list.paint_linear(id).map(|(from, to)| {
            let from_s = m.transform_point(from);
            let dir = m.transform_point(to).subtract(from_s);
            FillPaint::linear(ramp_id, from_s, dir, 1.0 / dir.length_squared().max(EPS))
        });
        let radial = list.paint_radial(id).map(|(center, radius)| {
            let center_s = m.transform_point(center);
            let radius_s = m.transform_vector(Vec2::new(radius.get(), 0.0)).length();
            FillPaint::radial(ramp_id, center_s, 1.0 / radius_s.max(EPS))
        });
        linear.or(radial)
    })
}

/// Emit a filled rect as the quad covering its **transformed axis-aligned bounding
/// box** — matching the software `fill_rect`. The `fill` supplies the colour
/// source (solid or gradient) and the per-corner UV; the command alpha is folded
/// in once.
fn append_rect(geo: &mut Draw2dGeometry, m: &Mat3, r: Rect, fill: &FillPaint, alpha: f32) {
    geo.push_fill(
        fill,
        bbox_corners(m, r),
        [PLAIN_FIELD; 4],
        KIND_CAPSULE,
        alpha,
    );
}

/// Emit a rect's **stroke**: a quad over the same transformed AABB carrying, per
/// corner, the four edge distances divided by the screen-space stroke width, so
/// the shader keeps a fragment when `min(edge distances)/width < 1` — the inset
/// border band the software `stroke_rect` composites. A zero / transparent stroke
/// makes the band empty (huge field) or the colour a no-op, so it draws nothing.
fn append_rect_stroke(
    geo: &mut Draw2dGeometry,
    m: &Mat3,
    r: Rect,
    stroke: &StrokeStyle,
    alpha: f32,
) {
    let (minx, miny, maxx, maxy) = transformed_bbox(m, r);
    let inv_w = 1.0
        / m.transform_vector(Vec2::new(stroke.width, 0.0))
            .length()
            .max(EPS);
    let corners = [
        Vec2::new(minx, miny),
        Vec2::new(maxx, miny),
        Vec2::new(maxx, maxy),
        Vec2::new(minx, maxy),
    ];
    let fields = corners.map(|c| {
        [
            (c.x - minx) * inv_w,
            (maxx - c.x) * inv_w,
            (c.y - miny) * inv_w,
            (maxy - c.y) * inv_w,
        ]
    });
    geo.push_solid(corners, fields, KIND_RECT_STROKE, stroke.color, alpha);
}

/// Emit a filled circle/ellipse as the quad covering its conic **bounding box**,
/// carrying per-corner conic-basis coordinates `(s, t)` so the shader runs the
/// software backend's exact `s²+t²≤1` test per pixel. Also emits the **stroke**
/// annulus quad (after the fill, so it composites over it like the software path).
fn append_conic(
    geo: &mut Draw2dGeometry,
    center: Vec2,
    ax: Vec2,
    ay: Vec2,
    fill: &FillPaint,
    stroke: &StrokeStyle,
    alpha: f32,
) {
    let hx = (ax.x * ax.x + ay.x * ay.x).sqrt();
    let hy = (ax.y * ax.y + ay.y * ay.y).sqrt();
    let corners = [
        Vec2::new(center.x - hx, center.y - hy),
        Vec2::new(center.x + hx, center.y - hy),
        Vec2::new(center.x + hx, center.y + hy),
        Vec2::new(center.x - hx, center.y + hy),
    ];
    let det = ax.x * ay.y - ay.x * ax.y;
    let st = |c: Vec2| {
        let dx = c.x - center.x;
        let dy = c.y - center.y;
        [(ay.y * dx - ay.x * dy) / det, (ax.x * dy - ax.y * dx) / det]
    };
    let fill_fields = corners.map(|c| {
        let s = st(c);
        [s[0], s[1], 0.0, 0.0]
    });
    geo.push_fill(fill, corners, fill_fields, KIND_CONIC, alpha);
    // Stroke annulus: inner radius matches the software `fill_conic` exactly
    // (world stroke width over the screen-space mean semi-diameter).
    let mean_radius = (ax.length() + ay.length()) * 0.5;
    let inner = (1.0 - stroke.width / mean_radius).max(0.0);
    let inner2 = inner * inner;
    let stroke_fields = corners.map(|c| {
        let s = st(c);
        [s[0], s[1], inner2, 0.0]
    });
    geo.push_solid(
        corners,
        stroke_fields,
        KIND_CONIC_STROKE,
        stroke.color,
        alpha,
    );
}

/// Emit a stroked line `a`–`b` (both through `m`) as **one** quad covering its
/// rounded-capsule bounding rectangle. Each corner carries the line-local coverage
/// field `(along, perp, length, half_width)` so the shader reproduces the software
/// `raster_line` capsule distance per pixel — including the round caps. A
/// zero-length segment falls back to the axis-aligned unit frame (branchless via
/// `usize::from`) and collapses to a round dot, the same shape the software
/// `EPS`-floored `len²` yields.
fn append_line(
    geo: &mut Draw2dGeometry,
    m: &Mat3,
    a: Vec2,
    b: Vec2,
    color: [f32; 4],
    width: f32,
    alpha: f32,
) {
    let pa = m.transform_point(a);
    let pb = m.transform_point(b);
    let half_w = 0.5 * m.transform_vector(Vec2::new(width, 0.0)).length();
    let ab = pb.subtract(pa);
    let len = ab.length();
    let inv = 1.0 / len.max(EPS);
    let degenerate = usize::from(len < EPS) as f32;
    let u = Vec2::new(ab.x * inv + degenerate, ab.y * inv);
    let n = Vec2::new(-u.y, u.x);
    let ext_a = pa.subtract(u.mul_scalar(half_w));
    let ext_b = pb.add(u.mul_scalar(half_w));
    let off = n.mul_scalar(half_w);
    let corners = [
        ext_a.subtract(off),
        ext_b.subtract(off),
        ext_b.add(off),
        ext_a.add(off),
    ];
    let fields = corners.map(|c| {
        let d = c.subtract(pa);
        [d.dot(u), d.dot(n), len, half_w]
    });
    geo.push_solid(corners, fields, KIND_CAPSULE, color, alpha);
}

/// Emit a polygon **path**: fill (when `closed`) as a fan of barycentric-coverage
/// triangles from the first vertex, and stroke as a capsule per edge (the closing
/// edge included only when `closed`). The fill alpha is folded with `closed`, so
/// an open polyline emits no fill (transparent triangles) — branchless. Each fan
/// triangle emits one quad over the triangle's bounding box carrying per-corner
/// barycentric coordinates, so the shader keeps a fragment when all three are
/// `≥ 0` — the exact point-in-triangle test, matching the software fill on a
/// convex polygon. A degenerate (collinear) triangle's zero `det` makes its
/// barycentrics non-finite, so it draws nothing.
fn append_path(
    geo: &mut Draw2dGeometry,
    m: &Mat3,
    points: &[Vec2],
    closed: bool,
    fill: &FillPaint,
    stroke: &StrokeStyle,
    alpha: f32,
) {
    let pts: Vec<Vec2> = points.iter().map(|p| m.transform_point(*p)).collect();
    let n = pts.len();
    let fill_alpha = alpha * f32::from(u8::from(closed));
    // Fan triangles (p0, p_i, p_{i+1}); empty for n < 3 via saturating range.
    (1..n.saturating_sub(1)).for_each(|i| {
        append_fan_triangle(geo, pts[0], pts[i], pts[i + 1], fill, fill_alpha);
    });
    // Stroke each edge; the closing edge (i == n-1) contributes only when closed.
    (0..n).for_each(|i| {
        let keep = ((i + 1) < n) | closed;
        let edge_width = stroke.width * f32::from(u8::from(keep));
        append_line(
            geo,
            &Mat3::IDENTITY,
            pts[i],
            pts[(i + 1) % n.max(1)],
            stroke.color,
            edge_width,
            alpha,
        );
    });
}

/// Emit one path fan triangle `(a, b, c)` (screen space) as a quad over its
/// bounding box carrying per-corner barycentric coordinates and `KIND_TRI`. The
/// `fill` supplies the colour source + per-corner UV (a gradient samples the same
/// affine UV as any other fill).
fn append_fan_triangle(
    geo: &mut Draw2dGeometry,
    a: Vec2,
    b: Vec2,
    c: Vec2,
    fill: &FillPaint,
    alpha: f32,
) {
    let minx = a.x.min(b.x).min(c.x);
    let miny = a.y.min(b.y).min(c.y);
    let maxx = a.x.max(b.x).max(c.x);
    let maxy = a.y.max(b.y).max(c.y);
    let corners = [
        Vec2::new(minx, miny),
        Vec2::new(maxx, miny),
        Vec2::new(maxx, maxy),
        Vec2::new(minx, maxy),
    ];
    let det = (b.y - c.y) * (a.x - c.x) + (c.x - b.x) * (a.y - c.y);
    let fields = corners.map(|p| barycentric(a, b, c, p, det));
    geo.push_fill(fill, corners, fields, KIND_TRI, alpha);
}

/// The barycentric coordinates `(l0, l1, l2, 0)` of point `p` with respect to
/// triangle `(a, b, c)` and its precomputed `det`. A zero `det` (collinear
/// triangle) makes them non-finite, so the shader's `min ≥ 0` test fails and the
/// triangle draws nothing — branchless, no divide-by-zero panic.
fn barycentric(a: Vec2, b: Vec2, c: Vec2, p: Vec2, det: f32) -> [f32; 4] {
    let l0 = ((b.y - c.y) * (p.x - c.x) + (c.x - b.x) * (p.y - c.y)) / det;
    let l1 = ((c.y - a.y) * (p.x - c.x) + (a.x - c.x) * (p.y - c.y)) / det;
    [l0, l1, 1.0 - l0 - l1, 0.0]
}

/// Emit a sprite as the quad covering its transformed dest AABB, with UVs mapping
/// the AABB linearly across the source sub-rect (normalised by the atlas size),
/// honouring per-axis flip. A sprite whose texture size is unknown emits no quad
/// (branchless via `into_iter`).
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
        let corners = bbox_corners(m, dest);
        let u0 = opts.source.min.x / tw;
        let u1 = (opts.source.min.x + opts.source.size.x) / tw;
        let v0 = opts.source.min.y / th;
        let v1 = (opts.source.min.y + opts.source.size.y) / th;
        let (ua, ub) = [(u0, u1), (u1, u0)][usize::from(opts.flip_x)];
        let (va, vb) = [(v0, v1), (v1, v0)][usize::from(opts.flip_y)];
        let uvs = [[ua, va], [ub, va], [ub, vb], [ua, vb]];
        let tint = opts.tint.channels();
        let color = [tint[0], tint[1], tint[2], tint[3] * alpha];
        let positions = corners.map(|c| [c.x, c.y]);
        geo.push_quad(
            positions,
            uvs,
            [PLAIN_FIELD; 4],
            KIND_CAPSULE,
            color,
            QuadSource::Sprite(texture.raw()),
        );
    });
}

/// Emit a **text glyph run**: lay out each glyph along the pen (honouring the run
/// advances + alignment) and draw it through the sprite path against the font's
/// atlas — a glyph quad is a textured quad, exactly like a sprite. The pen offset
/// + cell→`(advance, line_height)` scale ride on a per-glyph transform composed
/// onto `m`. An empty run emits nothing; an unloaded atlas no-ops per glyph
/// (the sprite path skips an unknown texture).
fn append_text(
    geo: &mut Draw2dGeometry,
    m: &Mat3,
    run: &GlyphRun,
    opts: TextDraw2d,
    alpha: f32,
    sizes: &Draw2dTextureSizes,
) {
    let atlas = opts.font.atlas_texture();
    let total: f32 = run.glyphs.iter().map(|g| g.advance.get()).sum();
    let start_x = -total * [0.0, 0.5, 1.0][usize::from(opts.align.raw())];
    let line_h = run.line_height.get();
    run.glyphs
        .iter()
        .scan(start_x, |pen, g| {
            let x = *pen;
            *pen += g.advance.get();
            Some((x, g))
        })
        .for_each(|(x, g)| {
            let cell = g.source;
            let sx = g.advance.get() / cell.size.x.max(EPS);
            let sy = line_h / cell.size.y.max(EPS);
            let gm = m
                .multiply(Mat3::translation(Vec2::new(x, 0.0)))
                .multiply(Mat3::scale(Vec2::new(sx, sy)));
            let glyph = SpriteDraw2d::new(cell, Vec2::ZERO, opts.color, false, false);
            append_sprite(geo, &gm, atlas, glyph, alpha, sizes);
        });
}

/// The four corners of `r` transformed by `m`, reduced to the axis-aligned
/// bounding box (`TL, TR, BR, BL`).
fn bbox_corners(m: &Mat3, r: Rect) -> [Vec2; 4] {
    let (minx, miny, maxx, maxy) = transformed_bbox(m, r);
    [
        Vec2::new(minx, miny),
        Vec2::new(maxx, miny),
        Vec2::new(maxx, maxy),
        Vec2::new(minx, maxy),
    ]
}

/// The transformed rect's continuous screen-space bounding box
/// `(minx, miny, maxx, maxy)` — its four corners through `m`, folded to extents.
/// Identical to the software backend's `transformed_bbox`, so the two emit the
/// same screen region.
fn transformed_bbox(m: &Mat3, r: Rect) -> (f32, f32, f32, f32) {
    let mx = r.max();
    [
        r.min,
        Vec2::new(mx.x, r.min.y),
        mx,
        Vec2::new(r.min.x, mx.y),
    ]
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
mod tests;
