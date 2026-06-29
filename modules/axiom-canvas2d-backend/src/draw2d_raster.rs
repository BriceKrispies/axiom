//! Software-raster consumer of the host-neutral [`axiom_host::Draw2dList`].
//!
//! Composites a frame's ordered 2D draw commands onto an RGBA framebuffer with
//! **src-over alpha blending** (via [`SoftwareFramebuffer::composite_pixel`]) —
//! the verified-missing "no alpha blending" fix on the software backend. The
//! list arrives already `(layer, submission)`-sorted by the host, so iterating
//! `Draw2dList::commands` in order is correct painter's order; each command's
//! resolved `alpha`, baked `Mat3` transform, and `layer` are honoured.
//!
//! ## Landed vs deferred (first cut)
//! This cut lands the two primitives that prove 2D compositing end to end:
//! **filled rect** (alpha-composited) and **sprite** (atlas source-rect blit with
//! tint, per-axis flip, and alpha). The remaining [`axiom_host::Draw2dCommand`]
//! kinds — circle, ellipse, line, path, gradient/paint fills, stroke, and text
//! glyph runs — are **explicitly deferred**: their `KIND_*` commands are
//! recognised and skipped (a no-op via their `as_*` accessor returning `None`),
//! never mis-rasterized. A rect whose fill is a *paint* (gradient) likewise
//! composites nothing until the paint path lands. They join the surface as
//! follow-up work behind the same branchless dispatch.
//!
//! ## Coordinate model (v1)
//! Draw coordinates are framebuffer pixels. Each command's baked `Mat3` (composed
//! with the list's optional `Camera2d`) is applied to the rect corners; the
//! **translation + scale** components are honoured exactly. A rotating/shearing
//! transform fills the transformed axis-aligned bounding box (an approximation) —
//! exact rotated rasterization is deferred with the other primitives.
//!
//! Pure Rust — no browser types — so it builds and is fully covered on native.

use std::collections::HashMap;

use axiom_host::{Draw2dCommand, Draw2dList, Rect, Rgba, SpriteDraw2d};
use axiom_math::{Mat3, Vec2};

use crate::software_framebuffer::SoftwareFramebuffer;

/// Smallest span used to normalize sprite UVs, so a zero-extent destination
/// never divides by zero.
const EPS: f32 = 1e-6;

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
        .for_each(|cmd| composite_command(&mut fb, &camera, cmd, textures));
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

/// Rasterize one command's landed kinds onto `fb`. Dispatch is branchless: each
/// kind's `as_*` accessor is `Some` only for its own `KIND_*`, so a non-matching
/// (or deferred) command runs zero fill iterations.
fn composite_command(
    fb: &mut SoftwareFramebuffer,
    camera: &Mat3,
    cmd: &Draw2dCommand,
    textures: &Draw2dTextures,
) {
    let m = camera.multiply(cmd.transform());
    let alpha = cmd.alpha().get();
    cmd.as_rect().into_iter().for_each(|r| {
        // A solid-colour fill composites; a paint (gradient) fill is deferred, so
        // its colour resolves to fully transparent (a no-op composite).
        let color = cmd
            .fill()
            .and_then(|f| f.fill_color)
            .map(Rgba::channels)
            .unwrap_or([0.0, 0.0, 0.0, 0.0]);
        fill_rect(fb, &m, r, color, alpha);
    });
    cmd.as_sprite().into_iter().for_each(|(texture, opts)| {
        textures
            .get(texture.raw())
            .into_iter()
            .for_each(|t| blit_sprite(fb, &m, opts, t, alpha));
    });
}

/// Composite a filled rect: fold the rect's resolved colour with the command
/// alpha and src-over every covered pixel.
fn fill_rect(fb: &mut SoftwareFramebuffer, m: &Mat3, r: Rect, color: [f32; 4], alpha: f32) {
    let (minx, miny, maxx, maxy) = transformed_bbox(m, r);
    let (x0, x1) = pixel_range(minx, maxx, fb.width());
    let (y0, y1) = pixel_range(miny, maxy, fb.height());
    let src = [color[0], color[1], color[2], color[3] * alpha];
    (y0..y1).for_each(|y| (x0..x1).for_each(|x| fb.composite_pixel(x, y, src)));
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
mod tests {
    use super::*;
    use axiom_host::{Common2d, Fill2d, PaintId, TextureId};
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
    fn paint_fill_is_deferred_and_composites_nothing() {
        let mut list = Draw2dList::default();
        list.push_command(Draw2dCommand::rect(
            header(0, 0, 1.0),
            rect(Vec2::ZERO, Vec2::new(8.0, 8.0)),
            Fill2d::paint(PaintId::from_raw(0)),
        ));
        list.sort_commands();
        let (bytes, _, _) = render(&list, 4, 4, &Draw2dTextures::default());
        assert!(bytes.iter().all(|&b| b == 0), "paint fill draws nothing yet");
    }

    #[test]
    fn deferred_kind_is_skipped() {
        // A circle (deferred) leaves the buffer untouched — its as_rect/as_sprite
        // are both None, so dispatch runs no fill.
        let mut list = Draw2dList::default();
        list.push_command(Draw2dCommand::circle(
            header(0, 0, 1.0),
            Vec2::new(2.0, 2.0),
            Meters::new(2.0).unwrap(),
            Fill2d::color(rgba(1.0, 1.0, 1.0, 1.0)),
        ));
        list.sort_commands();
        let (bytes, _, _) = render(&list, 4, 4, &Draw2dTextures::default());
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
}
