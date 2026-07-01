//! Software-raster consumer of the host-neutral [`axiom_host::Draw2dList`].
//! Composites a frame's ordered 2D draw commands onto an RGBA framebuffer with
//! **src-over alpha blending** (via [`SoftwareFramebuffer::composite_pixel`]) —
//! the verified-missing "no alpha blending" fix on the software backend. The
//! list arrives already `(layer, submission)`-sorted by the host, so iterating
//! `Draw2dList::commands` in order is correct painter's order; each command's
//! resolved `alpha`, baked `Mat3` transform, and `layer` are honoured.
//! ## Coverage of the SPEC-04 2D command set
//! This backend rasterizes every [`axiom_host::Draw2dCommand`] kind: **rect**,
//! **circle**, **ellipse** (rotation-exact via conjugate semi-diameters), **line**,
//! **particle-quad**, **sprite** (atlas source-rect blit with tint, per-axis flip,
//! and alpha), **path** (an arbitrary polygon, branchless even-odd scanline fill),
//! and **text glyph runs** (each glyph laid out and blitted through the sprite
//! path against the font atlas). Filled shapes honour their resolved **stroke**
//! (rect: an inset screen border; circle/ellipse: a radial annulus; path: a
//! per-edge polyline) and **gradient/paint fills** (linear and radial: the
//! contract's canonical gradient texture, sampled per pixel along the projection
//! parameter / radius). Everything is alpha-composited and honours the command's
//! resolved `alpha`.
//! ## Coordinate model
//! Draw coordinates are framebuffer pixels. Each command's baked `Mat3` (composed
//! with the list's optional `Camera2d`) places the shape. **Rect**, **line**, and
//! **circle/ellipse** honour the transform's translation + scale; circle/ellipse
//! additionally honour rotation/shear **exactly** (their per-pixel test inverts
//! the transformed conjugate semi-diameters), while a rect still fills the
//! transformed axis-aligned bounding box (its rotated form is an approximation).
//! Pure Rust — no browser types — so it builds and is fully covered on native.

use std::collections::HashMap;

use axiom_host::{Draw2dCommand, Draw2dList, GlyphRun, PaintId, Rect, SpriteDraw2d, TextDraw2d};
use axiom_math::{Mat3, Vec2};

/// The fully-transparent colour every "no fill" / "no stroke" / unresolvable-paint
/// path resolves to — compositing it is a no-op, so an absent fill or stroke
/// draws nothing without a branch.
const TRANSPARENT: [f32; 4] = [0.0, 0.0, 0.0, 0.0];

use crate::software_framebuffer::SoftwareFramebuffer;

/// Smallest span used to normalize sprite UVs / floor a degenerate divisor, so a
/// zero-extent destination, gradient axis, or polygon edge never divides by zero.
const EPS: f32 = 1e-6;

/// Resolution of a baked gradient texture — a linear ramp is `RAMP_N×1`, a radial
/// disc `RAMP_N×RAMP_N`. Must equal the GPU backend's `RAMP_N` so both backends
/// sample the *identical* host-baked texture (byte-parity, not just visual match).
const RAMP_N: u32 = 512;

/// One CPU-side sprite/atlas texture the [`Draw2dList`] sprite path samples. The
/// pixels are resolved **in the app** (fetch/decode) and uploaded here by id —
/// the same fetch-in-the-app rule the 3D mesh/material path already follows.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SpriteTexture {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

impl SpriteTexture {
    /// Nearest-neighbour sample at source-pixel `(x, y)`, clamped to the texture;
    /// a sample beyond the byte buffer is transparent (branchless via `get`).
    fn sample(&self, x: f32, y: f32) -> [f32; 4] {
        let xi = (x.floor() as i64).clamp(0, (self.width as i64 - 1).max(0)) as usize;
        let yi = (y.floor() as i64).clamp(0, (self.height as i64 - 1).max(0)) as usize;
        let off = (yi * self.width as usize + xi) * 4;
        self.rgba
            .get(off..off + 4)
            .map(|p| {
                [
                    p[0] as f32 / 255.0,
                    p[1] as f32 / 255.0,
                    p[2] as f32 / 255.0,
                    p[3] as f32 / 255.0,
                ]
            })
            .unwrap_or([0.0, 0.0, 0.0, 0.0])
    }
}

/// The backend's CPU sprite-texture registry, keyed by the contract's
/// [`axiom_host::TextureId`] raw value. A sprite command naming an unknown id
/// composites nothing (branchless via `get` → `into_iter`).
#[derive(Debug, Default)]
pub(crate) struct Draw2dTextures {
    map: HashMap<u64, SpriteTexture>,
}

impl Draw2dTextures {
    /// Build the registry from `(texture_id, width, height, RGBA8 pixels)` — the
    /// same upload shape the 3D material path uses.
    pub(crate) fn load(textures: &[(u64, u32, u32, Vec<u8>)]) -> Self {
        Draw2dTextures {
            map: textures
                .iter()
                .map(|(id, w, h, rgba)| {
                    (
                        *id,
                        SpriteTexture {
                            width: *w,
                            height: *h,
                            rgba: rgba.clone(),
                        },
                    )
                })
                .collect(),
        }
    }

    fn get(&self, id: u64) -> Option<&SpriteTexture> {
        self.map.get(&id)
    }
}

/// Composite the layer-sorted `list` onto a fresh transparent `width`×`height`
/// framebuffer and return the finished RGBA8 image — the 2D analogue of
/// [`crate::software_rasterizer::SoftwareRasterResult::rgba_bytes`].
pub(crate) fn render(
    list: &Draw2dList,
    width: u32,
    height: u32,
    textures: &Draw2dTextures,
) -> (Vec<u8>, u32, u32) {
    let mut fb = SoftwareFramebuffer::new(width, height);
    let camera = camera_matrix(list, width, height);
    list.commands()
        .iter()
        .for_each(|cmd| composite_command(&mut fb, &camera, cmd, textures, list));
    (fb.into_rgba_bytes(), width, height)
}

/// The screen transform for the frame's optional [`axiom_host::Camera2d`]:
/// `screen = (world - center)·zoom + viewport_centre`. With no camera the author
/// gets the backend's identity framing (world == framebuffer pixels).
fn camera_matrix(list: &Draw2dList, width: u32, height: u32) -> Mat3 {
    list.camera()
        .map(|c| {
            let z = c.zoom.get();
            let half = Vec2::new(width as f32 * 0.5, height as f32 * 0.5);
            Mat3::translation(half)
                .multiply(Mat3::scale(Vec2::new(z, z)))
                .multiply(Mat3::translation(c.center.mul_scalar(-1.0)))
        })
        .unwrap_or(Mat3::IDENTITY)
}

/// Rasterize one command onto `fb`. Dispatch is branchless: each kind's `as_*`
/// accessor is `Some` only for its own `KIND_*`, so a non-matching command runs
/// zero iterations.
fn composite_command(
    fb: &mut SoftwareFramebuffer,
    camera: &Mat3,
    cmd: &Draw2dCommand,
    textures: &Draw2dTextures,
    list: &Draw2dList,
) {
    let m = camera.multiply(cmd.transform());
    let alpha = cmd.alpha().get();
    let fill = FillSampler::resolve(&m, cmd, list);
    let stroke = StrokeStyle::resolve(cmd);
    cmd.as_rect().into_iter().for_each(|r| {
        fill_rect(fb, &m, r, &fill, alpha);
        stroke_rect(fb, &m, r, &stroke, alpha);
    });
    cmd.as_circle().into_iter().for_each(|(center, radius)| {
        let ax = m.transform_vector(Vec2::new(radius.get(), 0.0));
        let ay = m.transform_vector(Vec2::new(0.0, radius.get()));
        fill_conic(fb, m.transform_point(center), ax, ay, &fill, &stroke, alpha);
    });
    cmd.as_ellipse().into_iter().for_each(|(center, rx, ry, rotation)| {
        let local = Mat3::rotation(rotation);
        let ax = m.transform_vector(local.transform_vector(Vec2::new(rx.get(), 0.0)));
        let ay = m.transform_vector(local.transform_vector(Vec2::new(0.0, ry.get())));
        fill_conic(fb, m.transform_point(center), ax, ay, &fill, &stroke, alpha);
    });
    cmd.as_line().into_iter().for_each(|(a, b, color, width)| {
        raster_line(fb, &m, a, b, color.channels(), width.get(), alpha);
    });
    cmd.as_path().into_iter().for_each(|(points, closed)| {
        fill_path(fb, &m, &points, closed, &fill, &stroke, alpha);
    });
    cmd.as_particle().into_iter().for_each(|(center, size, color)| {
        let h = size.get();
        let quad = Rect::new(center.subtract(Vec2::new(h, h)), Vec2::new(2.0 * h, 2.0 * h));
        fill_rect(fb, &m, quad, &FillSampler::solid(color.channels()), alpha);
    });
    cmd.as_sprite().into_iter().for_each(|(texture, opts)| {
        textures
            .get(texture.raw())
            .into_iter()
            .for_each(|t| blit_sprite(fb, &m, opts, t, alpha));
    });
    cmd.as_text().into_iter().for_each(|(run, opts)| {
        composite_text(fb, &m, &run, opts, alpha, textures);
    });
}

/// Fold a resolved colour with the command alpha into the src-over source colour.
fn premultiply_alpha(color: [f32; 4], alpha: f32) -> [f32; 4] {
    [color[0], color[1], color[2], color[3] * alpha]
}

/// A filled shape's resolved colour **source**: a solid colour, or a gradient
/// sampling the contract's canonical [`SpriteTexture`] ramp by a per-pixel
/// parameter. Evaluated per covered pixel by [`FillSampler::eval`]. A solid fill
/// holds no ramp; an absent/unresolvable fill resolves to a transparent solid, so
/// it composites nothing without a branch.
#[derive(Clone)]
struct FillSampler {
    /// Solid colour (the gradient arms leave this [`TRANSPARENT`] and select the
    /// sampled ramp instead).
    solid: [f32; 4],
    /// The baked gradient ramp (`RAMP_N×1` linear / `RAMP_N×RAMP_N` radial), the
    /// *same* texture the GPU backend samples. `None` for a solid fill.
    ramp: Option<SpriteTexture>,
    /// UV mode: `0` solid, `1` linear, `2` radial.
    mode: usize,
    /// Linear: screen-space start + direction; `inv_len2 = 1/|dir|²`.
    from: Vec2,
    dir: Vec2,
    inv_len2: f32,
    /// Radial: screen-space centre + `1/radius`.
    center: Vec2,
    inv_radius: f32,
}

impl FillSampler {
    /// A solid-colour fill.
    fn solid(color: [f32; 4]) -> Self {
        FillSampler {
            solid: color,
            ramp: None,
            mode: 0,
            from: Vec2::ZERO,
            dir: Vec2::ZERO,
            inv_len2: 0.0,
            center: Vec2::ZERO,
            inv_radius: 0.0,
        }
    }

    /// Resolve a command's fill: a solid colour, a gradient sampling the canonical
    /// ramp, or — for an absent / unknown-paint fill — a transparent solid.
    /// Branchless: the solid and the (mutually exclusive) gradient arms are each an
    /// `Option`, picked by `Option::or`.
    fn resolve(m: &Mat3, cmd: &Draw2dCommand, list: &Draw2dList) -> FillSampler {
        let style = cmd.fill();
        let solid = style
            .and_then(|f| f.fill_color)
            .map(|c| FillSampler::solid(c.channels()));
        let gradient = style
            .and_then(|f| f.fill_paint)
            .and_then(|id| build_gradient_sampler(m, id, list));
        solid.or(gradient).unwrap_or(FillSampler::solid(TRANSPARENT))
    }

    /// The fill colour at screen-space pixel centre `p` (straight, not yet
    /// alpha-folded). Branchless: linear and radial parameters are both computed
    /// and `mode` table-indexes both the UV and the solid-vs-gradient result, so
    /// the unused arm's value is never selected. The ramp is nearest-sampled at
    /// `uv·dim` — the same texel the GPU's nearest sampler picks — so a gradient is
    /// byte-identical across backends.
    fn eval(&self, p: Vec2) -> [f32; 4] {
        let lin = p.subtract(self.from).dot(self.dir) * self.inv_len2;
        let d = p.subtract(self.center).mul_scalar(self.inv_radius);
        let uv = [[0.0, 0.0], [lin, 0.5], [d.x * 0.5 + 0.5, d.y * 0.5 + 0.5]][self.mode];
        let grad = self
            .ramp
            .as_ref()
            .map(|r| r.sample(uv[0] * r.width as f32, uv[1] * r.height as f32))
            .unwrap_or(TRANSPARENT);
        [self.solid, grad, grad][self.mode]
    }
}

/// Build the gradient [`FillSampler`] for paint `id`: bake the contract's
/// canonical ramp (`list.paint_texture`) into a [`SpriteTexture`] and resolve the
/// linear/radial geometry into screen space via `m`. `None` if the id names no
/// paint (the caller then falls back to a transparent fill, matching the GPU
/// path). Branchless via `and_then`/`map`/`or`.
fn build_gradient_sampler(m: &Mat3, id: PaintId, list: &Draw2dList) -> Option<FillSampler> {
    list.paint_texture(id, RAMP_N).and_then(|(w, h, bytes)| {
        let ramp = SpriteTexture {
            width: w,
            height: h,
            rgba: bytes,
        };
        let linear = list.paint_linear(id).map(|(from, to)| {
            let from_s = m.transform_point(from);
            let dir = m.transform_point(to).subtract(from_s);
            FillSampler {
                solid: TRANSPARENT,
                ramp: Some(ramp.clone()),
                mode: 1,
                from: from_s,
                dir,
                inv_len2: 1.0 / dir.length_squared().max(EPS),
                center: Vec2::ZERO,
                inv_radius: 0.0,
            }
        });
        let radial = list.paint_radial(id).map(|(center, radius)| {
            let center_s = m.transform_point(center);
            let radius_s = m.transform_vector(Vec2::new(radius.get(), 0.0)).length();
            FillSampler {
                solid: TRANSPARENT,
                ramp: Some(ramp),
                mode: 2,
                from: Vec2::ZERO,
                dir: Vec2::ZERO,
                inv_len2: 0.0,
                center: center_s,
                inv_radius: 1.0 / radius_s.max(EPS),
            }
        });
        linear.or(radial)
    })
}

/// A resolved stroke: straight colour + world-space width (the screen width is
/// derived per shape from the transform). Absent stroke resolves to a transparent
/// colour and zero width, so the stroke composites nothing without a branch.
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

/// Composite a filled rect: src-over every covered pixel with the fill's per-pixel
/// colour (solid or gradient), folded with the command alpha.
fn fill_rect(fb: &mut SoftwareFramebuffer, m: &Mat3, r: Rect, fill: &FillSampler, alpha: f32) {
    let (minx, miny, maxx, maxy) = transformed_bbox(m, r);
    let (x0, x1) = pixel_range(minx, maxx, fb.width());
    let (y0, y1) = pixel_range(miny, maxy, fb.height());
    (y0..y1).for_each(|y| {
        (x0..x1).for_each(|x| {
            let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
            fb.composite_pixel(x, y, premultiply_alpha(fill.eval(p), alpha));
        })
    });
}

/// Composite a rect's stroke: an inset border of its transformed AABB,
/// `width·scale` thick on each edge (pixels inside the AABB within that inset of
/// any edge). A zero width / transparent stroke collapses the border and
/// composites nothing — branchless (the edge tests combine with `|`).
fn stroke_rect(fb: &mut SoftwareFramebuffer, m: &Mat3, r: Rect, stroke: &StrokeStyle, alpha: f32) {
    let (minx, miny, maxx, maxy) = transformed_bbox(m, r);
    let (x0, x1) = pixel_range(minx, maxx, fb.width());
    let (y0, y1) = pixel_range(miny, maxy, fb.height());
    let w = m.transform_vector(Vec2::new(stroke.width, 0.0)).length();
    let src = premultiply_alpha(stroke.color, alpha);
    (y0..y1).for_each(|y| {
        (x0..x1).for_each(|x| {
            let fx = x as f32 + 0.5;
            let fy = y as f32 + 0.5;
            let on_border =
                (fx < minx + w) | (fx >= maxx - w) | (fy < miny + w) | (fy >= maxy - w);
            on_border.then(|| fb.composite_pixel(x, y, src));
        })
    });
}

/// Composite a filled + stroked conic (circle / ellipse) from its screen-space
/// `center` and two screen-space conjugate semi-diameter vectors `ax`, `ay`. A
/// pixel's `(s, t)` coordinates in the `[ax ay]` basis put it inside the shape
/// when `s² + t² ≤ 1`; the stroke is the annulus from that boundary inward to the
/// radius inset by the stroke width. A degenerate (zero-area) basis inverts to a
/// non-finite `det`, so every test is `false` and nothing draws — branchless, no
/// divide-by-zero panic. The fill colour is sampled per pixel (solid or gradient).
fn fill_conic(
    fb: &mut SoftwareFramebuffer,
    center: Vec2,
    ax: Vec2,
    ay: Vec2,
    fill: &FillSampler,
    stroke: &StrokeStyle,
    alpha: f32,
) {
    let half_x = (ax.x * ax.x + ay.x * ay.x).sqrt();
    let half_y = (ax.y * ax.y + ay.y * ay.y).sqrt();
    let (x0, x1) = pixel_range(center.x - half_x, center.x + half_x, fb.width());
    let (y0, y1) = pixel_range(center.y - half_y, center.y + half_y, fb.height());
    let det = ax.x * ay.y - ay.x * ax.y;
    let mean_radius = (ax.length() + ay.length()) * 0.5;
    let inner = (1.0 - stroke.width / mean_radius).max(0.0);
    let inner2 = inner * inner;
    let stroke_src = premultiply_alpha(stroke.color, alpha);
    (y0..y1).for_each(|y| {
        (x0..x1).for_each(|x| {
            let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
            let d = p.subtract(center);
            let s = (ay.y * d.x - ay.x * d.y) / det;
            let t = (ax.x * d.y - ax.y * d.x) / det;
            let norm2 = s * s + t * t;
            let inside = norm2 <= 1.0;
            let on_stroke = inside & (norm2 >= inner2);
            inside.then(|| fb.composite_pixel(x, y, premultiply_alpha(fill.eval(p), alpha)));
            on_stroke.then(|| fb.composite_pixel(x, y, stroke_src));
        })
    });
}

/// Composite a polygon **path**: an even-odd scanline fill (when `closed`) plus a
/// per-edge polyline stroke (the closing edge included only when `closed`). For
/// each pixel centre `p`, the fill counts ray crossings over the polygon's wrap
/// edges (`inside` iff the count is odd) and the stroke takes the minimum distance
/// to any edge segment (`on_stroke` iff `≤ half_width`). Branchless: the crossing
/// is a `filter().count()`, the distance a `map().fold(min)`, the closing-edge and
/// open-vs-closed choices are `&`/index masks; a horizontal edge's zero `Δy` makes
/// its crossing test `false` (the `&` discards the non-finite intercept) and a
/// zero-length edge's `len²` is `EPS`-floored.
fn fill_path(
    fb: &mut SoftwareFramebuffer,
    m: &Mat3,
    points: &[Vec2],
    closed: bool,
    fill: &FillSampler,
    stroke: &StrokeStyle,
    alpha: f32,
) {
    let pts: Vec<Vec2> = points.iter().map(|p| m.transform_point(*p)).collect();
    let n = pts.len();
    let (minx, miny, maxx, maxy) = pts.iter().fold(
        (f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY),
        |(a, b, c, d), p| (a.min(p.x), b.min(p.y), c.max(p.x), d.max(p.y)),
    );
    let half_w = 0.5 * m.transform_vector(Vec2::new(stroke.width, 0.0)).length();
    // Expand the scan box by the stroke half-width so the stroke's outer band at
    // the polygon's vertices/edges is not clipped to the fill bbox — the GPU's
    // per-edge capsule quads extend by the same `half_w`, so the two agree.
    let (x0, x1) = pixel_range(minx - half_w, maxx + half_w, fb.width());
    let (y0, y1) = pixel_range(miny - half_w, maxy + half_w, fb.height());
    let stroke_src = premultiply_alpha(stroke.color, alpha);
    // (start, end, is_closing) for each wrap edge; empty when `pts` is empty.
    let edges: Vec<(Vec2, Vec2, bool)> = (0..n)
        .map(|i| (pts[i], pts[(i + 1) % n.max(1)], i + 1 == n))
        .collect();
    (y0..y1).for_each(|y| {
        (x0..x1).for_each(|x| {
            let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
            let crossings = edges
                .iter()
                .filter(|(a, b, _)| {
                    let cond_y = (a.y > p.y) != (b.y > p.y);
                    let xint = (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x;
                    cond_y & (p.x < xint)
                })
                .count();
            let inside = (crossings & 1) == 1;
            (closed & inside)
                .then(|| fb.composite_pixel(x, y, premultiply_alpha(fill.eval(p), alpha)));
            let min_d = edges.iter().fold(f32::INFINITY, |acc, (a, b, is_closing)| {
                let excluded = *is_closing & !closed;
                let ab = b.subtract(*a);
                let len2 = ab.length_squared().max(EPS);
                let t = (p.subtract(*a).dot(ab) / len2).clamp(0.0, 1.0);
                let dist = p.subtract(a.add(ab.mul_scalar(t))).length();
                acc.min([dist, f32::INFINITY][usize::from(excluded)])
            });
            (min_d <= half_w).then(|| fb.composite_pixel(x, y, stroke_src));
        })
    });
}

/// Composite a **text glyph run**: lay each glyph out along the pen (advances +
/// alignment) and blit it through the sprite path against the font atlas — a glyph
/// is a textured quad, exactly like a sprite. The pen offset + cell→`(advance,
/// line_height)` scale ride on a per-glyph transform composed onto `m`. An empty
/// run composites nothing; an unloaded atlas no-ops (the texture lookup yields
/// nothing). Branchless: alignment is a table index, the pen a `scan`.
fn composite_text(
    fb: &mut SoftwareFramebuffer,
    m: &Mat3,
    run: &GlyphRun,
    opts: TextDraw2d,
    alpha: f32,
    textures: &Draw2dTextures,
) {
    let atlas = opts.font.atlas_texture();
    let total: f32 = run.glyphs.iter().map(|g| g.advance.get()).sum();
    let start_x = -total * [0.0, 0.5, 1.0][usize::from(opts.align.raw())];
    let line_h = run.line_height.get();
    textures.get(atlas.raw()).into_iter().for_each(|tex| {
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
                blit_sprite(fb, &gm, glyph, tex, alpha);
            });
    });
}

/// Composite a stroked line segment `a`–`b` (both through `m`) of screen-space
/// half-width `0.5·width·scale`: every pixel within that distance of the segment
/// gets the line's own colour, src-over. The projection parameter is clamped to
/// `[0, 1]` so the round-capped endpoints are handled with no branch; a
/// zero-length segment's `length_squared` is floored by `EPS` so the projection
/// never divides by zero.
fn raster_line(fb: &mut SoftwareFramebuffer, m: &Mat3, a: Vec2, b: Vec2, color: [f32; 4], width: f32, alpha: f32) {
    let pa = m.transform_point(a);
    let pb = m.transform_point(b);
    let half_w = 0.5 * m.transform_vector(Vec2::new(width, 0.0)).length();
    let (x0, x1) = pixel_range(pa.x.min(pb.x) - half_w, pa.x.max(pb.x) + half_w, fb.width());
    let (y0, y1) = pixel_range(pa.y.min(pb.y) - half_w, pa.y.max(pb.y) + half_w, fb.height());
    let ab = pb.subtract(pa);
    let len2 = ab.length_squared().max(EPS);
    let src = premultiply_alpha(color, alpha);
    (y0..y1).for_each(|y| {
        (x0..x1).for_each(|x| {
            let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
            let t = (p.subtract(pa).dot(ab) / len2).clamp(0.0, 1.0);
            let proj = pa.add(ab.mul_scalar(t));
            let dist = p.subtract(proj).length();
            (dist <= half_w).then(|| fb.composite_pixel(x, y, src));
        })
    });
}

/// Composite a sprite: the destination quad is the source sub-rect's size,
/// anchored by `opts.anchor` and placed/scaled by `m`. Each covered pixel samples
/// the atlas at its (flipped) UV, tints, folds in the command alpha, and src-over
/// composites.
fn blit_sprite(
    fb: &mut SoftwareFramebuffer,
    m: &Mat3,
    opts: SpriteDraw2d,
    tex: &SpriteTexture,
    alpha: f32,
) {
    let ssize = opts.source.size;
    let origin = Vec2::new(-opts.anchor.x * ssize.x, -opts.anchor.y * ssize.y);
    let dest = Rect::new(origin, ssize);
    let (minx, miny, maxx, maxy) = transformed_bbox(m, dest);
    let (x0, x1) = pixel_range(minx, maxx, fb.width());
    let (y0, y1) = pixel_range(miny, maxy, fb.height());
    let spanx = (maxx - minx).max(EPS);
    let spany = (maxy - miny).max(EPS);
    let tint = opts.tint.channels();
    (y0..y1).for_each(|y| {
        (x0..x1).for_each(|x| {
            let u = ((x as f32 + 0.5) - minx) / spanx;
            let v = ((y as f32 + 0.5) - miny) / spany;
            let uf = [u, 1.0 - u][usize::from(opts.flip_x)];
            let vf = [v, 1.0 - v][usize::from(opts.flip_y)];
            let sx = opts.source.min.x + uf * opts.source.size.x;
            let sy = opts.source.min.y + vf * opts.source.size.y;
            let texel = tex.sample(sx, sy);
            let src = [
                texel[0] * tint[0],
                texel[1] * tint[1],
                texel[2] * tint[2],
                texel[3] * tint[3] * alpha,
            ];
            fb.composite_pixel(x, y, src);
        })
    });
}

/// The transformed rect's continuous screen-space bounding box
/// `(minx, miny, maxx, maxy)` — its four corners through `m`, folded to extents.
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

/// The covered pixel range `[lo, hi)` for a continuous `[min, max)` screen
/// extent on a `dim`-pixel axis: `floor(min)`..`ceil(max)`, clamped to `0..=dim`.
fn pixel_range(min: f32, max: f32, dim: u32) -> (u32, u32) {
    let lo = (min.floor() as i64).clamp(0, dim as i64) as u32;
    let hi = (max.ceil() as i64).clamp(0, dim as i64) as u32;
    (lo, hi)
}


#[cfg(test)]
mod tests;
