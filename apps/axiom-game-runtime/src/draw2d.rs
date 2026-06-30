//! 2D drawing (SPEC-10: draw2d) composed into the bridge: the particle, render
//! target, and shape verbs the TS `Frame` surface drives, every one forwarding to
//! the engine's [`axiom_draw2d::Draw2dApi`] builder. The builder owns the
//! transform stack, the layer sort, and the particle field; this module only
//! marshals scalars in and the neutral, layer-sorted command list out — nothing
//! is rasterized or re-implemented here.
//!
//! ## Presentation-only (§17.5)
//! Everything here is display data: the particle field feeds no sim-readable
//! getter, and `advance_particles` steps on the **presentation** delta the host
//! measured, never a fixed sim tick — so a 2D draw can never perturb determinism.
//!
//! ## Boundary convention (the established slice / scalar / handle rule)
//! A point / bounds crosses as a `&[f64]` slice (`bounds = [x, y, w, h]`); a
//! colour as its packed `0xRRGGBBAA` `u32`; an emitter recipe as one flat
//! `&[f64]` config slice (count, lifetime, speed, spread, gravityX, gravityY,
//! size, colorStart, colorEnd, layer) — one slice keeps the call within the
//! engine's argument-count budget. Handles cross as raw `u64` (`f64` at the JS
//! edge). [`GameBridge::draw2d_finish`] returns the sorted command list as a flat,
//! self-describing `[kind, layer, submission, len, …geometry]` stream — the
//! deterministic `(kind, layer, submission)` ordering a `Frame` consumer always
//! read, now followed by the `len` per-shape geometry columns a 2D presenter
//! rasterizes (see that method for the per-kind payload layout).

use axiom_draw2d::{Draw2dApi, EmitterConfig, EmitterId, SpriteAnimation};
use axiom_host::{
    Common2d, Draw2dCommand, Fill2d, Rect, RenderTargetId, Rgba, SpriteDraw2d, Stroke2d, TextAlign,
    TextDraw2d, TextureId,
};
use axiom_kernel::{Meters, Radians, Ratio, Seconds};
use axiom_math::{Mat3, Vec2};

use crate::{font, GameBridge};

/// The `i`-th element of a boundary slice as a scalar (missing ⇒ `0`).
fn at(s: &[f64], i: usize) -> f64 {
    *s.get(i).unwrap_or(&0.0)
}

/// A finite [`Meters`] from a boundary scalar (non-finite ⇒ zero).
fn meters(value: f64) -> Meters {
    Meters::new(value as f32).unwrap_or_else(|_| Meters::new(0.0).expect("0.0 is finite"))
}

/// A finite [`Seconds`] from a boundary scalar (non-finite ⇒ zero).
fn seconds(value: f64) -> Seconds {
    Seconds::new(value as f32).unwrap_or_else(|_| Seconds::new(0.0).expect("0.0 is finite"))
}

/// A finite [`Radians`] from a boundary scalar (non-finite ⇒ zero).
fn radians(value: f64) -> Radians {
    Radians::new(value as f32).unwrap_or_else(|_| Radians::new(0.0).expect("0.0 is finite"))
}

/// A [`Fill2d`] from a packed solid fill plus a packed stroke colour + width. The
/// stroke is always attached — a transparent (`0x00000000`) colour with a `0`
/// width composites nothing, so an unstroked shape needs no branch at the
/// boundary.
fn styled_fill(fill: u32, stroke: u32, stroke_width: f64) -> Fill2d {
    Fill2d::color(rgba(fill)).with_stroke(Stroke2d::new(rgba(stroke), meters(stroke_width)))
}

/// A `Vec2` from a 2-element boundary slice (missing entries read `0`).
fn vec2(s: &[f64]) -> Vec2 {
    Vec2::new(at(s, 0) as f32, at(s, 1) as f32)
}

/// An [`Rgba`] from a packed `0xRRGGBBAA` value (each channel `0..1`).
fn rgba(packed: u32) -> Rgba {
    let channel = |shift: u32| Ratio::finite_or_zero(((packed >> shift) & 0xFF) as f32 / 255.0);
    Rgba::new(channel(24), channel(16), channel(8), channel(0))
}

/// The per-draw [`Common2d`] (z-layer + alpha) from boundary scalars.
fn common(layer: i32, alpha: f64) -> Common2d {
    Common2d::new(layer, Ratio::finite_or_zero(alpha as f32))
}

/// A [`TextAlign`] from its boundary discriminant (`0`/`1`/`2`); any other value
/// resolves to left, the contract default.
fn text_align(raw: u8) -> TextAlign {
    [TextAlign::LEFT, TextAlign::CENTER, TextAlign::RIGHT]
        .get(usize::from(raw))
        .copied()
        .unwrap_or(TextAlign::LEFT)
}

/// The six affine columns `[a, b, c, d, tx, ty]` of a baked 2D transform, as the
/// `f64`s the boundary carries — the placement a sprite/text presenter applies
/// (Canvas2D `setTransform(a, b, c, d, tx, ty)`). A 2D affine `Mat3` keeps its
/// linear part in columns 0–1 and its translation in column 2.
fn affine_columns(m: Mat3) -> [f64; 6] {
    let t = m.as_cols_array();
    [
        f64::from(t[0]),
        f64::from(t[1]),
        f64::from(t[3]),
        f64::from(t[4]),
        f64::from(t[6]),
        f64::from(t[7]),
    ]
}

/// Pack a resolved [`Rgba`] (channels in `0..1`) back to a `0xRRGGBBAA` u32, as
/// the `f64` the boundary carries — the inverse of the inbound [`rgba`] unpack.
fn pack_rgba(color: Rgba) -> f64 {
    let [r, g, b, a] = color.channels();
    let quantize = |value: f32| ((value.clamp(0.0, 1.0) * 255.0).round() as u32) & 0xFF;
    f64::from((quantize(r) << 24) | (quantize(g) << 16) | (quantize(b) << 8) | quantize(a))
}

/// The `[fillRGBA, strokeRGBA, strokeWidth]` columns of a filled shape's style. A
/// missing fill colour (a gradient paint, or a stroke-only shape) or a missing
/// stroke packs as transparent `0` / zero width — the same "composites nothing"
/// convention the inbound [`styled_fill`] uses.
fn fill_columns(fill: Option<Fill2d>) -> [f64; 3] {
    let fill_rgba = fill.and_then(|f| f.fill_color).map_or(0.0, pack_rgba);
    let stroke = fill.and_then(|f| f.stroke);
    [
        fill_rgba,
        stroke.map_or(0.0, |s| pack_rgba(s.color)),
        stroke.map_or(0.0, |s| f64::from(s.width.get())),
    ]
}

impl GameBridge {
    /// Register a particle emitter from a flat config slice (`draw2dCreateEmitter`)
    /// `[count, lifetime, speed, spread, gravityX, gravityY, size, colorStart,
    /// colorEnd, layer]`; returns its raw [`EmitterId`].
    pub fn draw2d_create_emitter(&mut self, config: &[f64]) -> u64 {
        let recipe = EmitterConfig {
            count: at(config, 0) as u32,
            lifetime: seconds(at(config, 1)),
            speed: meters(at(config, 2)),
            spread: Ratio::finite_or_zero(at(config, 3) as f32),
            gravity: Vec2::new(at(config, 4) as f32, at(config, 5) as f32),
            size: meters(at(config, 6)),
            color_start: rgba(at(config, 7) as u32),
            color_end: rgba(at(config, 8) as u32),
            layer: at(config, 9) as i32,
        };
        u64::from(self.draw2d.create_emitter(recipe).raw())
    }

    /// Spawn a burst from emitter `id` at `at_point` flying along `direction`
    /// (`draw2dEmit`); an unknown id is a no-op.
    pub fn draw2d_emit(&mut self, id: u64, at_point: &[f64], direction: &[f64]) {
        self.draw2d
            .emit(EmitterId::from_raw(id as u32), vec2(at_point), vec2(direction));
    }

    /// Step the live particles by the presentation delta `dt` seconds and append
    /// each survivor as a particle-quad command (`draw2dAdvanceParticles`).
    pub fn draw2d_advance_particles(&mut self, dt: f64) {
        self.draw2d.advance_particles(seconds(dt));
    }

    /// Create an off-screen render target (`draw2dCreateRenderTarget`), returning
    /// its raw [`RenderTargetId`].
    pub fn draw2d_create_render_target(&mut self, width: u32, height: u32) -> u64 {
        u64::from(self.draw2d.create_render_target(width, height).raw())
    }

    /// Route subsequent draws into `target` (`draw2dBeginTarget`).
    pub fn draw2d_begin_target(&mut self, target: u64) {
        self.draw2d.begin_target(RenderTargetId::from_raw(target as u32));
    }

    /// Stop routing into a render target (`draw2dEndTarget`).
    pub fn draw2d_end_target(&mut self) {
        self.draw2d.end_target();
    }

    /// The texture handle naming `target`'s off-screen surface (`draw2dTargetTexture`).
    pub fn draw2d_target_texture(&self, target: u64) -> u64 {
        self.draw2d.target_texture(RenderTargetId::from_raw(target as u32)).raw()
    }

    /// Set the 2D camera (`draw2dCamera2d`); `center = [x, y]`, `zoom` a positive
    /// scale (non-finite ⇒ zero).
    pub fn draw2d_camera2d(&mut self, center: &[f64], zoom: f64) {
        self.draw2d
            .set_camera2d(vec2(center), Ratio::finite_or_zero(zoom as f32));
    }

    /// Draw a filled / stroked rectangle (`draw2dRect`); `bounds = [x, y, w, h]`.
    pub fn draw2d_rect(&mut self, bounds: &[f64], fill: u32, stroke: u32, stroke_width: f64, layer: i32, alpha: f64) {
        let rect = Rect::new(
            Vec2::new(at(bounds, 0) as f32, at(bounds, 1) as f32),
            Vec2::new(at(bounds, 2) as f32, at(bounds, 3) as f32),
        );
        self.draw2d
            .rect(rect, styled_fill(fill, stroke, stroke_width), common(layer, alpha));
    }

    /// Draw a filled / stroked circle (`draw2dCircle`); `center = [x, y]`.
    pub fn draw2d_circle(&mut self, center: &[f64], radius: f64, fill: u32, stroke: u32, stroke_width: f64, layer: i32, alpha: f64) {
        self.draw2d.circle(
            vec2(center),
            meters(radius),
            styled_fill(fill, stroke, stroke_width),
            common(layer, alpha),
        );
    }

    /// Draw a filled / stroked (optionally rotated) ellipse (`draw2dEllipse`);
    /// `geom = [centerX, centerY, rx, ry, rotation]` (rotation in radians).
    pub fn draw2d_ellipse(&mut self, geom: &[f64], fill: u32, stroke: u32, stroke_width: f64, layer: i32, alpha: f64) {
        self.draw2d.ellipse(
            Vec2::new(at(geom, 0) as f32, at(geom, 1) as f32),
            meters(at(geom, 2)),
            meters(at(geom, 3)),
            radians(at(geom, 4)),
            styled_fill(fill, stroke, stroke_width),
            common(layer, alpha),
        );
    }

    /// Draw a straight line segment (`draw2dLine`) of its own `color` and `width`;
    /// `a = [x, y]`, `b = [x, y]`.
    pub fn draw2d_line(&mut self, a: &[f64], b: &[f64], color: u32, width: f64, layer: i32, alpha: f64) {
        self.draw2d
            .line(vec2(a), vec2(b), rgba(color), meters(width), common(layer, alpha));
    }

    /// Sample a flip-book animation (`draw2dSampleAnimation`, §10.2). `frames`
    /// arrives flattened as `[x, y, w, h, …]` (one `Rect` per 4 scalars), `fps` is
    /// the integer frame rate, `elapsed` the presentation seconds, and `looping`
    /// whether the index wraps (else clamps to the last frame). Returns the sampled
    /// sub-rect as `[x, y, w, h]`. Pure: the native [`Draw2dApi::sample_animation`]
    /// owns the index math, so the boundary never recomputes it (one source of
    /// truth); a trailing partial chunk (length not a multiple of 4) is dropped.
    pub fn draw2d_sample_animation(&self, frames: &[f64], fps: f64, elapsed: f64, looping: bool) -> Vec<f64> {
        let rects: Vec<Rect> = frames
            .chunks_exact(4)
            .map(|f| Rect::new(Vec2::new(f[0] as f32, f[1] as f32), Vec2::new(f[2] as f32, f[3] as f32)))
            .collect();
        let anim = SpriteAnimation { frames: rects, fps: fps as u32 };
        let sampled = Draw2dApi::sample_animation(&anim, seconds(elapsed), looping);
        vec![
            f64::from(sampled.min.x),
            f64::from(sampled.min.y),
            f64::from(sampled.size.x),
            f64::from(sampled.size.y),
        ]
    }

    /// Draw a textured sprite (`draw2dSprite`); `texture` is a `loadTexture` handle
    /// and `opts` is the flat slice `[posX, posY, rotation, scaleX, scaleY, anchorX,
    /// anchorY, srcX, srcY, srcW, srcH, tintRGBA, flipX, flipY, layer, alpha]`. A
    /// zero `srcW`/`srcH` means "the whole texture" — the source size is only known
    /// once the app-side decode resolves the pixels, so the presenter substitutes
    /// the bitmap dimensions. Placement (pos · rotation · scale) is baked onto the
    /// command's transform; the per-sprite `source`/`anchor`/`tint`/`flips` ride on
    /// the style.
    pub fn draw2d_sprite(&mut self, texture: u64, opts: &[f64]) {
        let pos = Vec2::new(at(opts, 0) as f32, at(opts, 1) as f32);
        let scale = Vec2::new(at(opts, 3) as f32, at(opts, 4) as f32);
        let anchor = Vec2::new(at(opts, 5) as f32, at(opts, 6) as f32);
        let source = Rect::new(
            Vec2::new(at(opts, 7) as f32, at(opts, 8) as f32),
            Vec2::new(at(opts, 9) as f32, at(opts, 10) as f32),
        );
        let style = SpriteDraw2d::new(
            source,
            anchor,
            rgba(at(opts, 11) as u32),
            at(opts, 12) != 0.0,
            at(opts, 13) != 0.0,
        );
        let placement = Mat3::translation(pos)
            .multiply(Mat3::rotation(radians(at(opts, 2))))
            .multiply(Mat3::scale(scale));
        let depth = self.draw2d.push_transform(placement);
        self.draw2d
            .sprite(TextureId::from_raw(texture), style, common(at(opts, 14) as i32, at(opts, 15)));
        self.draw2d.pop_transform(depth);
    }

    /// Draw a line of text (`draw2dText`) in the built-in monospace font; `opts` is
    /// `[posX, posY, fontSize, colorRGBA, align, layer, alpha]`. The string is
    /// resolved to a glyph run against the baked atlas ([`crate::font`]); placement
    /// is baked onto the command's transform.
    pub fn draw2d_text(&mut self, value: &str, opts: &[f64]) {
        let run = font::glyph_run(value, at(opts, 2) as f32);
        let style = TextDraw2d::new(
            font::BUILTIN_FONT,
            rgba(at(opts, 3) as u32),
            text_align(at(opts, 4) as u8),
        );
        let placement = Mat3::translation(Vec2::new(at(opts, 0) as f32, at(opts, 1) as f32));
        let depth = self.draw2d.push_transform(placement);
        self.draw2d
            .text(run, style, common(at(opts, 5) as i32, at(opts, 6)));
        self.draw2d.pop_transform(depth);
    }

    /// Measure `value` at `font_size` in the built-in font (`draw2dMeasureText`),
    /// returning `[width, height]` in surface units. Pure monospace arithmetic — no
    /// atlas, so it is platform-reproducible (SPEC-04 §9).
    pub fn draw2d_measure_text(&self, value: &str, font_size: f64) -> Vec<f64> {
        let (width, height) = font::measure(value, font_size as f32);
        vec![f64::from(width), f64::from(height)]
    }

    /// Finish the frame and return the layer-sorted [`axiom_host::Draw2dList`]
    /// itself, for the engine's live 2D presenter (`axiom-windowing`) to rasterize
    /// through the same WebGPU → WebGL2 → Canvas 2D cascade a 3D scene uses. This is
    /// the structured peer of [`Self::draw2d_finish`] (which flattens the same list
    /// to the numeric stream the TS `Frame.finish` contract carries); both drain the
    /// per-frame surface (particles persist), so a frame uses exactly one of them.
    /// The browser boot path drives this via [`crate::wasm::WasmGame`]; the flat
    /// variant remains for the SDK's neutral `Frame.finish` contract and its tests.
    pub fn draw2d_finish_list(&mut self) -> axiom_host::Draw2dList {
        self.draw2d.finish()
    }

    /// Finish the frame and return the layer-sorted main command list as a flat,
    /// self-describing stream a 2D presenter can rasterize (`draw2dFinish`). Each
    /// command is `[kind, layer, submission, len, …geometry]`: the `len` payload
    /// columns that follow depend on `kind`, so a consumer can advance by
    /// `4 + len` past a kind it does not handle. Geometry is in the surface's own
    /// units; colours are packed `0xRRGGBBAA`; `alpha` is the resolved `0..1`:
    ///
    /// - `RECT`     (1): `[minX, minY, sizeW, sizeH, fillRGBA, strokeRGBA, strokeWidth, alpha]`
    /// - `CIRCLE`   (2): `[centerX, centerY, radius, fillRGBA, strokeRGBA, strokeWidth, alpha]`
    /// - `ELLIPSE`  (3): `[centerX, centerY, rx, ry, rotation, fillRGBA, strokeRGBA, strokeWidth, alpha]`
    /// - `LINE`     (4): `[aX, aY, bX, bY, colorRGBA, width, alpha]`
    /// - `PARTICLE` (8): `[centerX, centerY, size, colorRGBA, alpha]`
    /// - `SPRITE`   (6): `[texId, a, b, c, d, tx, ty, srcX, srcY, srcW, srcH,
    ///   anchorX, anchorY, tintRGBA, flipX, flipY, alpha]` — `(a..ty)` is the baked
    ///   2D affine (Canvas2D `setTransform`); `srcW`/`srcH` of `0` mean "the whole
    ///   texture"; `flipX`/`flipY` are `0`/`1`.
    /// - `TEXT_GLYPHS` (7): `[atlasTexId, a, b, c, d, tx, ty, colorRGBA, align,
    ///   lineHeight, alpha, glyphCount, (srcX, srcY, srcW, srcH, advance) ×
    ///   glyphCount]` — the glyphs lay out left-to-right from the baked transform's
    ///   origin, each sampling its atlas sub-rect; `atlasTexId` is the reserved
    ///   font-atlas handle the harness bakes.
    /// - other kinds (path): `len = 0` — ordering kept, geometry not yet flattened.
    ///
    /// Resets the per-frame surface for the next frame (particles persist). The
    /// `(kind, layer, submission)` prefix preserves the deterministic ordering the
    /// list always carried; the geometry is purely additive.
    pub fn draw2d_finish(&mut self) -> Vec<f64> {
        let list = self.draw2d.finish();
        let mut out: Vec<f64> = Vec::new();
        for cmd in list.commands() {
            let alpha = f64::from(cmd.alpha().get());
            let style = fill_columns(cmd.fill());
            let payload: Vec<f64> = match cmd.kind_code() {
                Draw2dCommand::KIND_RECT => {
                    let r = cmd.as_rect().expect("a RECT command carries rect geometry");
                    vec![
                        f64::from(r.min.x),
                        f64::from(r.min.y),
                        f64::from(r.size.x),
                        f64::from(r.size.y),
                        style[0],
                        style[1],
                        style[2],
                        alpha,
                    ]
                }
                Draw2dCommand::KIND_CIRCLE => {
                    let (center, radius) = cmd.as_circle().expect("a CIRCLE command carries circle geometry");
                    vec![
                        f64::from(center.x),
                        f64::from(center.y),
                        f64::from(radius.get()),
                        style[0],
                        style[1],
                        style[2],
                        alpha,
                    ]
                }
                Draw2dCommand::KIND_ELLIPSE => {
                    let (center, rx, ry, rotation) = cmd.as_ellipse().expect("an ELLIPSE command carries ellipse geometry");
                    vec![
                        f64::from(center.x),
                        f64::from(center.y),
                        f64::from(rx.get()),
                        f64::from(ry.get()),
                        f64::from(rotation.get()),
                        style[0],
                        style[1],
                        style[2],
                        alpha,
                    ]
                }
                Draw2dCommand::KIND_LINE => {
                    let (a, b, color, width) = cmd.as_line().expect("a LINE command carries line geometry");
                    vec![
                        f64::from(a.x),
                        f64::from(a.y),
                        f64::from(b.x),
                        f64::from(b.y),
                        pack_rgba(color),
                        f64::from(width.get()),
                        alpha,
                    ]
                }
                Draw2dCommand::KIND_PARTICLE_QUAD => {
                    let (center, size, color) = cmd.as_particle().expect("a PARTICLE_QUAD command carries particle geometry");
                    vec![
                        f64::from(center.x),
                        f64::from(center.y),
                        f64::from(size.get()),
                        pack_rgba(color),
                        alpha,
                    ]
                }
                Draw2dCommand::KIND_SPRITE => {
                    let (texture, opts) = cmd.as_sprite().expect("a SPRITE command carries sprite geometry");
                    let [a, b, c, d, tx, ty] = affine_columns(cmd.transform());
                    vec![
                        texture.raw() as f64,
                        a, b, c, d, tx, ty,
                        f64::from(opts.source.min.x),
                        f64::from(opts.source.min.y),
                        f64::from(opts.source.size.x),
                        f64::from(opts.source.size.y),
                        f64::from(opts.anchor.x),
                        f64::from(opts.anchor.y),
                        pack_rgba(opts.tint),
                        f64::from(u8::from(opts.flip_x)),
                        f64::from(u8::from(opts.flip_y)),
                        alpha,
                    ]
                }
                Draw2dCommand::KIND_TEXT_GLYPHS => {
                    let (run, opts) = cmd.as_text().expect("a TEXT_GLYPHS command carries a glyph run");
                    let [a, b, c, d, tx, ty] = affine_columns(cmd.transform());
                    let mut payload = vec![
                        font::FONT_ATLAS_TEXTURE.raw() as f64,
                        a, b, c, d, tx, ty,
                        pack_rgba(opts.color),
                        f64::from(opts.align.raw()),
                        f64::from(run.line_height.get()),
                        alpha,
                        run.glyphs.len() as f64,
                    ];
                    run.glyphs.iter().for_each(|g| {
                        payload.extend_from_slice(&[
                            f64::from(g.source.min.x),
                            f64::from(g.source.min.y),
                            f64::from(g.source.size.x),
                            f64::from(g.source.size.y),
                            f64::from(g.advance.get()),
                        ]);
                    });
                    payload
                }
                _ => Vec::new(),
            };
            out.push(f64::from(cmd.kind_code()));
            out.push(f64::from(cmd.layer()));
            out.push(f64::from(cmd.submission_index()));
            out.push(payload.len() as f64);
            out.extend(payload);
        }
        out
    }
}

#[cfg(target_arch = "wasm32")]
mod wasm_exports {
    use wasm_bindgen::prelude::*;

    use crate::wasm::WasmGame;

    #[wasm_bindgen]
    impl WasmGame {
        /// Register a particle emitter from a flat config slice (`draw2dCreateEmitter`).
        #[wasm_bindgen(js_name = draw2dCreateEmitter)]
        pub fn draw2d_create_emitter(&mut self, config: &[f64]) -> f64 {
            self.bridge.draw2d_create_emitter(config) as f64
        }

        /// Spawn a particle burst (`draw2dEmit`).
        #[wasm_bindgen(js_name = draw2dEmit)]
        pub fn draw2d_emit(&mut self, id: f64, at_point: &[f64], direction: &[f64]) {
            self.bridge.draw2d_emit(id as u64, at_point, direction);
        }

        /// Step the live particles (`draw2dAdvanceParticles`).
        #[wasm_bindgen(js_name = draw2dAdvanceParticles)]
        pub fn draw2d_advance_particles(&mut self, dt: f64) {
            self.bridge.draw2d_advance_particles(dt);
        }

        /// Create an off-screen render target (`draw2dCreateRenderTarget`).
        #[wasm_bindgen(js_name = draw2dCreateRenderTarget)]
        pub fn draw2d_create_render_target(&mut self, width: u32, height: u32) -> f64 {
            self.bridge.draw2d_create_render_target(width, height) as f64
        }

        /// Route subsequent draws into a render target (`draw2dBeginTarget`).
        #[wasm_bindgen(js_name = draw2dBeginTarget)]
        pub fn draw2d_begin_target(&mut self, target: f64) {
            self.bridge.draw2d_begin_target(target as u64);
        }

        /// Stop routing into a render target (`draw2dEndTarget`).
        #[wasm_bindgen(js_name = draw2dEndTarget)]
        pub fn draw2d_end_target(&mut self) {
            self.bridge.draw2d_end_target();
        }

        /// The texture handle naming a render target's surface (`draw2dTargetTexture`).
        #[wasm_bindgen(js_name = draw2dTargetTexture)]
        pub fn draw2d_target_texture(&self, target: f64) -> f64 {
            self.bridge.draw2d_target_texture(target as u64) as f64
        }

        /// Set the 2D camera (`draw2dCamera2d`).
        #[wasm_bindgen(js_name = draw2dCamera2d)]
        pub fn draw2d_camera2d(&mut self, center: &[f64], zoom: f64) {
            self.bridge.draw2d_camera2d(center, zoom);
        }

        /// Draw a filled / stroked rectangle (`draw2dRect`).
        #[wasm_bindgen(js_name = draw2dRect)]
        pub fn draw2d_rect(&mut self, bounds: &[f64], fill: u32, stroke: u32, stroke_width: f64, layer: i32, alpha: f64) {
            self.bridge.draw2d_rect(bounds, fill, stroke, stroke_width, layer, alpha);
        }

        /// Draw a filled / stroked circle (`draw2dCircle`).
        #[wasm_bindgen(js_name = draw2dCircle)]
        pub fn draw2d_circle(
            &mut self,
            center: &[f64],
            radius: f64,
            fill: u32,
            stroke: u32,
            stroke_width: f64,
            layer: i32,
            alpha: f64,
        ) {
            self.bridge.draw2d_circle(center, radius, fill, stroke, stroke_width, layer, alpha);
        }

        /// Draw a filled / stroked ellipse (`draw2dEllipse`).
        #[wasm_bindgen(js_name = draw2dEllipse)]
        pub fn draw2d_ellipse(&mut self, geom: &[f64], fill: u32, stroke: u32, stroke_width: f64, layer: i32, alpha: f64) {
            self.bridge.draw2d_ellipse(geom, fill, stroke, stroke_width, layer, alpha);
        }

        /// Draw a straight line segment (`draw2dLine`).
        #[wasm_bindgen(js_name = draw2dLine)]
        pub fn draw2d_line(&mut self, a: &[f64], b: &[f64], color: u32, width: f64, layer: i32, alpha: f64) {
            self.bridge.draw2d_line(a, b, color, width, layer, alpha);
        }

        /// Sample a flip-book animation (`draw2dSampleAnimation`, §10.2).
        #[wasm_bindgen(js_name = draw2dSampleAnimation)]
        pub fn draw2d_sample_animation(&self, frames: &[f64], fps: f64, elapsed: f64, looping: bool) -> Vec<f64> {
            self.bridge.draw2d_sample_animation(frames, fps, elapsed, looping)
        }

        /// Draw a textured sprite (`draw2dSprite`).
        #[wasm_bindgen(js_name = draw2dSprite)]
        pub fn draw2d_sprite(&mut self, texture: f64, opts: &[f64]) {
            self.bridge.draw2d_sprite(texture as u64, opts);
        }

        /// Draw a line of monospace text (`draw2dText`).
        #[wasm_bindgen(js_name = draw2dText)]
        pub fn draw2d_text(&mut self, value: String, opts: &[f64]) {
            self.bridge.draw2d_text(&value, opts);
        }

        /// Measure `value` at `font_size`, returning `[width, height]` (`draw2dMeasureText`).
        #[wasm_bindgen(js_name = draw2dMeasureText)]
        pub fn draw2d_measure_text(&self, value: String, font_size: f64) -> Vec<f64> {
            self.bridge.draw2d_measure_text(&value, font_size)
        }

        /// Finish the frame, returning the flat command list (`draw2dFinish`).
        #[wasm_bindgen(js_name = draw2dFinish)]
        pub fn draw2d_finish(&mut self) -> Vec<f64> {
            self.bridge.draw2d_finish()
        }
    }
}

#[cfg(test)]
mod tests {
    use axiom_host::Draw2dCommand;

    use crate::{demo_app, GameBridge};

    const STEP: u64 = 1_000_000;

    fn bridge() -> GameBridge {
        GameBridge::new(demo_app().build(), 0, STEP, 1)
    }

    /// A particle emitter recipe: 3 particles, layer 5, opaque→clear fade.
    fn emitter() -> [f64; 10] {
        [
            3.0,                  // count
            2.0,                  // lifetime
            10.0,                 // speed
            0.25,                 // spread
            0.0,                  // gravityX
            -4.0,                 // gravityY
            0.5,                  // size
            f64::from(u32::MAX),  // colorStart 0xffffffff
            0.0,                  // colorEnd   0x00000000
            5.0,                  // layer
        ]
    }

    /// Build one frame: a render-target-routed rect, a main-list circle (layer 1),
    /// and a 3-particle burst (layer 5). Returns the flat finished command list.
    fn frame() -> Vec<f64> {
        let mut b = bridge();
        b.draw2d_camera2d(&[0.0, 0.0], 1.0);
        let target = b.draw2d_create_render_target(64, 32);
        b.draw2d_begin_target(target);
        b.draw2d_rect(&[0.0, 0.0, 10.0, 10.0], 0xff00_00ff, 0x0000_00ff, 1.0, 0, 1.0);
        b.draw2d_end_target();
        b.draw2d_circle(&[1.0, 1.0], 2.0, 0x00ff_00ff, 0, 0.0, 1, 1.0);
        let e = b.draw2d_create_emitter(&emitter());
        b.draw2d_emit(e, &[0.0, 0.0], &[1.0, 0.0]);
        b.draw2d_advance_particles(0.5);
        b.draw2d_finish()
    }

    /// Decode the self-describing list into `(kind, layer, payload)` records by
    /// walking the `[kind, layer, submission, len, …payload]` stream.
    fn records(list: &[f64]) -> Vec<(u32, i32, Vec<f64>)> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < list.len() {
            let kind = list[i] as u32;
            let layer = list[i + 1] as i32;
            let len = list[i + 3] as usize;
            out.push((kind, layer, list[i + 4..i + 4 + len].to_vec()));
            i += 4 + len;
        }
        out
    }

    #[test]
    fn a_frame_builds_a_layer_sorted_command_list_and_replays() {
        let list = frame();
        let recs = records(&list);
        // The render-target rect routes off the main list; the main list holds the
        // circle + 3 particle quads, layer-sorted: the layer-1 circle
        // (KIND_CIRCLE = 2) precedes the layer-5 particle quads (KIND_PARTICLE_QUAD = 8).
        let kinds_layers: Vec<(u32, i32)> = recs.iter().map(|(k, l, _)| (*k, *l)).collect();
        assert_eq!(kinds_layers, vec![(2, 1), (8, 5), (8, 5), (8, 5)]);
        // Each circle carries its 7 geometry columns; each particle quad its 5.
        assert_eq!(recs[0].2.len(), 7);
        assert_eq!(recs[1].2.len(), 5);
        // Same facade calls + same dt ⇒ byte-identical command list.
        assert_eq!(frame(), list);
    }

    #[test]
    fn ellipse_and_line_verbs_record_their_kinds_in_layer_order() {
        let mut b = bridge();
        // An ellipse on layer 0 and a line on layer 2, submitted line-first.
        b.draw2d_line(&[0.0, 0.0], &[10.0, 0.0], 0xffff_00ff, 2.0, 2, 1.0);
        b.draw2d_ellipse(&[5.0, 5.0, 4.0, 2.0, 0.5], 0x00ff_00ff, 0xff00_00ff, 1.0, 0, 1.0);
        let recs = records(&b.draw2d_finish());
        // Layer-sorted so the layer-0 ellipse (KIND 3, 9 geometry columns)
        // precedes the layer-2 line (KIND 4, 7 geometry columns).
        let kinds_layers: Vec<(u32, i32)> = recs.iter().map(|(k, l, _)| (*k, *l)).collect();
        assert_eq!(kinds_layers, vec![(3, 0), (4, 2)]);
        assert_eq!(recs[0].2.len(), 9);
        assert_eq!(recs[1].2.len(), 7);
    }

    #[test]
    fn shape_geometry_and_colors_flatten_into_the_payload() {
        let mut b = bridge();
        // A red-filled, blue-stroked rect; a green circle; a yellow line.
        b.draw2d_rect(&[1.0, 2.0, 30.0, 40.0], 0xff00_00ff, 0x0000_ffff, 3.0, 0, 1.0);
        b.draw2d_circle(&[5.0, 6.0], 7.0, 0x00ff_00ff, 0, 0.0, 0, 0.5);
        b.draw2d_line(&[0.0, 0.0], &[8.0, 9.0], 0xffff_00ff, 2.0, 0, 1.0);
        let recs = records(&b.draw2d_finish());
        // RECT: [minX, minY, w, h, fillRGBA, strokeRGBA, strokeWidth, alpha].
        assert_eq!(recs[0].0, Draw2dCommand::KIND_RECT);
        assert_eq!(recs[0].2, vec![1.0, 2.0, 30.0, 40.0, f64::from(0xff00_00ffu32), f64::from(0x0000_ffffu32), 3.0, 1.0]);
        // CIRCLE: [cx, cy, r, fillRGBA, strokeRGBA, strokeWidth, alpha]; alpha 0.5.
        assert_eq!(recs[1].0, Draw2dCommand::KIND_CIRCLE);
        assert_eq!(recs[1].2, vec![5.0, 6.0, 7.0, f64::from(0x00ff_00ffu32), 0.0, 0.0, 0.5]);
        // LINE: [aX, aY, bX, bY, colorRGBA, width, alpha]; a line carries no fill.
        assert_eq!(recs[2].0, Draw2dCommand::KIND_LINE);
        assert_eq!(recs[2].2, vec![0.0, 0.0, 8.0, 9.0, f64::from(0xffff_00ffu32), 2.0, 1.0]);
    }

    #[test]
    fn sprite_flattens_its_transform_source_anchor_tint_and_flips() {
        let mut b = bridge();
        // A sprite at (10, 20), scale (2, 3), centred anchor, 16×16 source sub-rect,
        // white tint, no flips, layer 0, opaque.
        b.draw2d_sprite(
            0x1000_0000,
            &[10.0, 20.0, 0.0, 2.0, 3.0, 0.5, 0.5, 0.0, 0.0, 16.0, 16.0, f64::from(u32::MAX), 0.0, 0.0, 0.0, 1.0],
        );
        let recs = records(&b.draw2d_finish());
        assert_eq!(recs.len(), 1);
        let (kind, _layer, p) = &recs[0];
        assert_eq!(*kind, Draw2dCommand::KIND_SPRITE);
        assert_eq!(p.len(), 17);
        // texId, then the baked affine: translate (10,20) · scale (2,3) ⇒
        // [a=2, b=0, c=0, d=3, tx=10, ty=20].
        assert_eq!(p[0], f64::from(0x1000_0000u32));
        assert_eq!(&p[1..7], &[2.0, 0.0, 0.0, 3.0, 10.0, 20.0]);
        // source sub-rect, anchor, white tint, no flips, opaque.
        assert_eq!(&p[7..11], &[0.0, 0.0, 16.0, 16.0]);
        assert_eq!(&p[11..13], &[0.5, 0.5]);
        assert_eq!(p[13], f64::from(u32::MAX));
        assert_eq!(&p[14..17], &[0.0, 0.0, 1.0]);
    }

    #[test]
    fn text_flattens_a_glyph_run_against_the_baked_atlas() {
        let mut b = bridge();
        // "Hi" at (5, 6), size 16, red, left-aligned, layer 1, opaque.
        b.draw2d_text("Hi", &[5.0, 6.0, 16.0, f64::from(0xff00_00ffu32), 0.0, 1.0, 1.0]);
        let recs = records(&b.draw2d_finish());
        let (kind, layer, p) = &recs[0];
        assert_eq!(*kind, Draw2dCommand::KIND_TEXT_GLYPHS);
        assert_eq!(*layer, 1);
        // Header: atlas texId, translation affine (5,6), red, align 0, line height
        // 16, opaque, then a glyph count of 2.
        assert_eq!(p[0], f64::from(crate::font::FONT_ATLAS_TEXTURE.raw() as u32));
        assert_eq!(&p[1..7], &[1.0, 0.0, 0.0, 1.0, 5.0, 6.0]);
        assert_eq!(p[7], f64::from(0xff00_00ffu32));
        assert_eq!(&p[8..12], &[0.0, 16.0, 1.0, 2.0]); // align, lineHeight, alpha, glyphCount
        assert_eq!(p.len(), 12 + 2 * 5);
        // 'H' (code 72) is atlas cell 40 → column 8, row 2 → source (64, 32, 8, 16),
        // advance 8 (= size · 0.5).
        assert_eq!(&p[12..17], &[64.0, 32.0, 8.0, 16.0, 8.0]);
    }

    #[test]
    fn measure_text_is_monospace() {
        let b = bridge();
        // 5 chars · size 20 · 0.5 advance ratio = 50 wide, 20 tall.
        assert_eq!(b.draw2d_measure_text("score", 20.0), vec![50.0, 20.0]);
        assert_eq!(b.draw2d_measure_text("", 16.0), vec![0.0, 16.0]);
    }

    #[test]
    fn a_sprite_and_text_frame_replays_byte_identically() {
        let build = || {
            let mut b = bridge();
            b.draw2d_sprite(
                0x1000_0000,
                &[1.0, 2.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, f64::from(u32::MAX), 0.0, 0.0, 0.0, 1.0],
            );
            b.draw2d_text("HUD", &[0.0, 0.0, 12.0, f64::from(u32::MAX), 1.0, 5.0, 1.0]);
            b.draw2d_finish()
        };
        assert_eq!(build(), build());
    }

    #[test]
    fn render_target_handles_are_stable_and_distinct() {
        let mut b = bridge();
        let a = b.draw2d_create_render_target(16, 16);
        let c = b.draw2d_create_render_target(32, 32);
        assert_ne!(a, c);
        // The target's surface texture is stable for a given target.
        assert_eq!(b.draw2d_target_texture(a), b.draw2d_target_texture(a));
    }

    #[test]
    fn sample_animation_marshals_frames_and_selects_by_loop() {
        let b = bridge();
        // Three distinct sub-rects, flattened [x, y, w, h, …], at 2 fps.
        let frames = [
            0.0, 0.0, 1.0, 1.0, //
            10.0, 0.0, 1.0, 1.0, //
            20.0, 0.0, 1.0, 1.0,
        ];
        // elapsed 1.0s ⇒ index floor(2.0) = 2 ⇒ the third frame.
        assert_eq!(b.draw2d_sample_animation(&frames, 2.0, 1.0, true), vec![20.0, 0.0, 1.0, 1.0]);
        // elapsed 2.0s ⇒ index 4: non-looping clamps to the last, looping wraps to 1.
        assert_eq!(b.draw2d_sample_animation(&frames, 2.0, 2.0, false), vec![20.0, 0.0, 1.0, 1.0]);
        assert_eq!(b.draw2d_sample_animation(&frames, 2.0, 2.0, true), vec![10.0, 0.0, 1.0, 1.0]);
        // An empty book marshals to the inert zero-rect.
        assert_eq!(b.draw2d_sample_animation(&[], 2.0, 1.0, true), vec![0.0, 0.0, 0.0, 0.0]);
    }
}
