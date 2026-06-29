//! UI surface + layout (SPEC-09 immediate-mode HUD) composed into the bridge: the
//! `uiBeginFrame` / `uiRect` / `uiText` / `uiSprite` / `uiButton` / `uiViewport` /
//! `uiDrawList` verbs over the engine's [`axiom_interface::UiSurface`], plus the
//! pure `uiSolveLayout` over the engine's responsive flex solver
//! ([`axiom_layout::solve`]). The surface owns button hit-testing and the
//! viewport; the layout layer owns the flex math — neither is re-implemented here.
//!
//! ## Why the bridge keeps its own encoded draw log
//! [`UiDrawItem`](axiom_interface::UiDrawItem) is a data-carrying `enum`, so a
//! consumer can only read it back by `match` — control flow the spine/app
//! branchless discipline forbids. Rather than reach into the enum, the bridge
//! writes each draw it issues into its **own** little-endian byte log as it
//! issues it (the `(tag, fields[, string])` framing below), and `uiDrawList`
//! returns that buffer. The [`UiSurface`] is still the genuine engine seam: it
//! computes `uiButton` activation and holds the viewport. This is the same
//! "marshal to bytes at the boundary" rule the `world` component codec uses.
//!
//! ## Boundary convention (slice / scalar) + draw-log framing
//! A point / bounds crosses as a `&[f64]` slice (`bounds = [x, y, w, h]`,
//! `pos = [x, y]`) — one slice per vector keeps each method within the engine's
//! argument-count budget. A colour crosses as its packed `0xRRGGBBAA` `u32`, a
//! texture as its raw [`HandleId`](axiom_kernel::HandleId). Each draw-log item is
//! a `u8` tag, then a fixed block of little-endian `f64`s, then — for text/button
//! — a `u32`-LE length-prefixed UTF-8 label:
//! - `1` rect:   `[x, y, w, h, fill, stroke, strokeWidth]`
//! - `2` text:   `[x, y, color, size]` + label
//! - `3` sprite: `[texture, x, y, w, h]`
//! - `4` button: `[x, y, w, h, fill, stroke, strokeWidth, activated]` + label
//!
//! `uiSolveLayout` takes the viewport plus a flat fixed-width node table and
//! returns each node's solved `[x, y, w, h]` rect in input order.

use axiom_host::{HostViewport, Pixels};
use axiom_interface::{
    PointerSnapshot, UiColor, UiFill, UiRect, UiSpriteOpts, UiSurface, UiTextOpts, UiUnit,
    UiViewport,
};
use axiom_kernel::{HandleId, Ratio};
use axiom_layout::{solve, Align, Direction, Justify, LayoutStyle, LayoutTreeBuilder, NodeId};

use crate::GameBridge;

/// Draw-log tags (see the module header framing).
const TAG_RECT: u8 = 1;
const TAG_TEXT: u8 = 2;
const TAG_SPRITE: u8 = 3;
const TAG_BUTTON: u8 = 4;

/// The number of `f64` columns in one `uiSolveLayout` node record:
/// `[parent, directionIdx, justifyIdx, alignIdx, gap, basis, grow]`.
const NODE_STRIDE: usize = 7;

/// The `i`-th element of a boundary slice as a scalar (missing ⇒ `0`).
fn at(s: &[f64], i: usize) -> f64 {
    *s.get(i).unwrap_or(&0.0)
}

/// A screen-space unit from a boundary scalar.
fn unit(value: f64) -> UiUnit {
    UiUnit::new(value as f32)
}

/// A [`UiRect`] from a `[x, y, w, h]` boundary slice.
fn ui_rect(bounds: &[f64]) -> UiRect {
    UiRect::new(
        unit(at(bounds, 0)),
        unit(at(bounds, 1)),
        unit(at(bounds, 2)),
        unit(at(bounds, 3)),
    )
}

/// A finite [`Pixels`] from a boundary scalar (non-finite ⇒ zero).
fn pixels(value: f64) -> Pixels {
    Pixels::new(value as f32).unwrap_or_else(|_| Pixels::new(0.0).expect("0.0 is finite"))
}

/// Append a block of little-endian `f64`s to the draw log.
fn push_f64s(buf: &mut Vec<u8>, values: &[f64]) {
    values
        .iter()
        .for_each(|value| buf.extend_from_slice(&value.to_le_bytes()));
}

/// Append a `u32`-LE length-prefixed UTF-8 label to the draw log.
fn push_label(buf: &mut Vec<u8>, label: &str) {
    buf.extend_from_slice(&(label.len() as u32).to_le_bytes());
    buf.extend_from_slice(label.as_bytes());
}

/// Resolve a flex node record into a [`LayoutStyle`], selecting the enum fields
/// by dense index (out-of-range ⇒ the mobile-first default) — a table select,
/// never a branch. Unspecified style fields keep `LayoutStyle::new()` defaults.
fn style_from(rec: &[f64]) -> LayoutStyle {
    let mut style = LayoutStyle::new();
    style.direction = [Direction::Row, Direction::Column, Direction::Adaptive]
        .into_iter()
        .nth(at(rec, 1) as usize)
        .unwrap_or(Direction::Row);
    style.justify = [
        Justify::Start,
        Justify::Center,
        Justify::End,
        Justify::SpaceBetween,
    ]
    .into_iter()
    .nth(at(rec, 2) as usize)
    .unwrap_or(Justify::Start);
    style.align = [Align::Start, Align::Center, Align::End, Align::Stretch]
        .into_iter()
        .nth(at(rec, 3) as usize)
        .unwrap_or(Align::Start);
    style.gap = pixels(at(rec, 4));
    style.basis = pixels(at(rec, 5));
    style.grow = Ratio::finite_or_zero(at(rec, 6) as f32);
    style
}

/// The UI state the bridge owns: the engine immediate-mode [`UiSurface`] (for
/// button hit-testing + the viewport) and the bridge's own encoded draw log.
#[derive(Debug, Default)]
pub(crate) struct UiState {
    surface: UiSurface,
    log: Vec<u8>,
}

impl UiState {
    /// A fresh UI state.
    pub(crate) fn new() -> Self {
        UiState::default()
    }

    /// Begin a frame: install the viewport + pointer snapshot and clear the log.
    fn begin_frame(&mut self, viewport: &[f64], pointer: &[f64], pressed_edge: bool) {
        self.surface.begin_frame(
            UiViewport {
                width: unit(at(viewport, 0)),
                height: unit(at(viewport, 1)),
            },
            PointerSnapshot {
                x: unit(at(pointer, 0)),
                y: unit(at(pointer, 1)),
                pressed_edge,
            },
        );
        self.log.clear();
    }

    /// Draw a filled/stroked rectangle.
    fn rect(&mut self, bounds: &[f64], fill: u32, stroke: u32, stroke_w: f64) {
        let style = UiFill {
            fill: UiColor::new(fill),
            stroke: UiColor::new(stroke),
            stroke_width: unit(stroke_w),
        };
        self.surface.rect(ui_rect(bounds), style);
        self.log.push(TAG_RECT);
        push_f64s(
            &mut self.log,
            &[
                at(bounds, 0),
                at(bounds, 1),
                at(bounds, 2),
                at(bounds, 3),
                f64::from(fill),
                f64::from(stroke),
                stroke_w,
            ],
        );
    }

    /// Draw a run of text.
    fn text(&mut self, value: &str, pos: &[f64], color: u32, size: f64) {
        self.surface.text(
            value,
            UiTextOpts {
                x: unit(at(pos, 0)),
                y: unit(at(pos, 1)),
                color: UiColor::new(color),
                size: unit(size),
            },
        );
        self.log.push(TAG_TEXT);
        push_f64s(
            &mut self.log,
            &[at(pos, 0), at(pos, 1), f64::from(color), size],
        );
        push_label(&mut self.log, value);
    }

    /// Draw a textured sprite.
    fn sprite(&mut self, texture: u64, bounds: &[f64]) {
        self.surface.sprite(
            HandleId::from_raw(texture),
            UiSpriteOpts {
                x: unit(at(bounds, 0)),
                y: unit(at(bounds, 1)),
                w: unit(at(bounds, 2)),
                h: unit(at(bounds, 3)),
            },
        );
        self.log.push(TAG_SPRITE);
        push_f64s(
            &mut self.log,
            &[
                texture as f64,
                at(bounds, 0),
                at(bounds, 1),
                at(bounds, 2),
                at(bounds, 3),
            ],
        );
    }

    /// Draw an immediate-mode button, returning whether it was activated this
    /// frame (the engine's pure `(bounds, pointer)` truth table).
    fn button(&mut self, bounds: &[f64], label: &str, fill: u32, stroke: u32, stroke_w: f64) -> bool {
        let style = UiFill {
            fill: UiColor::new(fill),
            stroke: UiColor::new(stroke),
            stroke_width: unit(stroke_w),
        };
        let activated = self.surface.button(ui_rect(bounds), label, style);
        self.log.push(TAG_BUTTON);
        push_f64s(
            &mut self.log,
            &[
                at(bounds, 0),
                at(bounds, 1),
                at(bounds, 2),
                at(bounds, 3),
                f64::from(fill),
                f64::from(stroke),
                stroke_w,
                f64::from(u8::from(activated)),
            ],
        );
        push_label(&mut self.log, label);
        activated
    }

    /// This frame's viewport as `[width, height]`.
    fn viewport(&self) -> Vec<f64> {
        let vp = self.surface.viewport();
        vec![f64::from(vp.width.get()), f64::from(vp.height.get())]
    }
}

impl GameBridge {
    /// Begin a UI frame (`uiBeginFrame`): install the `[width, height]` viewport +
    /// `[x, y]` pointer and the press edge, clearing last frame's draw log.
    pub fn ui_begin_frame(&mut self, viewport: &[f64], pointer: &[f64], pressed: bool) {
        self.ui.begin_frame(viewport, pointer, pressed);
    }

    /// Draw a filled/stroked rectangle (`uiRect`); `bounds = [x, y, w, h]`.
    pub fn ui_rect(&mut self, bounds: &[f64], fill: u32, stroke: u32, stroke_w: f64) {
        self.ui.rect(bounds, fill, stroke, stroke_w);
    }

    /// Draw a run of text (`uiText`); `pos = [x, y]`.
    pub fn ui_text(&mut self, value: &str, pos: &[f64], color: u32, size: f64) {
        self.ui.text(value, pos, color, size);
    }

    /// Draw a textured sprite (`uiSprite`); `bounds = [x, y, w, h]`.
    pub fn ui_sprite(&mut self, texture: u64, bounds: &[f64]) {
        self.ui.sprite(texture, bounds);
    }

    /// Draw an immediate-mode button (`uiButton`); returns activation this frame.
    pub fn ui_button(&mut self, bounds: &[f64], label: &str, fill: u32, stroke: u32, sw: f64) -> bool {
        self.ui.button(bounds, label, fill, stroke, sw)
    }

    /// This frame's viewport `[width, height]` (`uiViewport`).
    pub fn ui_viewport(&self) -> Vec<f64> {
        self.ui.viewport()
    }

    /// This frame's accumulated screen-space draw log as bytes (`uiDrawList`),
    /// framed as the module header documents.
    pub fn ui_draw_list(&self) -> Vec<u8> {
        self.ui.log.clone()
    }

    /// Solve a responsive flex layout (`uiSolveLayout`) over the engine's
    /// [`axiom_layout::solve`]: `nodes` is a flat table of `NODE_STRIDE`-wide
    /// records `[parent, directionIdx, justifyIdx, alignIdx, gap, basis, grow]`
    /// (a negative / out-of-range `parent` is a root). Returns each node's solved
    /// `[x, y, w, h]` rect in input order (empty for a node the solver dropped).
    pub fn ui_solve_layout(&self, vw: f64, vh: f64, nodes: &[f64]) -> Vec<f64> {
        let viewport = HostViewport::new(vw as u32, vh as u32, Ratio::finite_or_zero(1.0))
            .unwrap_or_else(|_| {
                HostViewport::new(1, 1, Ratio::finite_or_zero(1.0)).expect("1x1 viewport is valid")
            });
        let count = nodes.len() / NODE_STRIDE;
        let mut builder = LayoutTreeBuilder::new();
        let mut builder_idx: Vec<usize> = Vec::new();
        (0..count).for_each(|i| {
            let rec = &nodes[i * NODE_STRIDE..i * NODE_STRIDE + NODE_STRIDE];
            let id = NodeId::from_raw(i as u32);
            let style = style_from(rec);
            // A negative `parent` (a root) fails the `usize` conversion, yielding
            // `None`; any in-range parent resolves to its builder index.
            let parent = usize::try_from(at(rec, 0) as i64)
                .ok()
                .and_then(|p| builder_idx.get(p).copied());
            let bidx = parent
                .map(|p| builder.child(p, id, style))
                .unwrap_or_else(|| builder.root(id, style));
            builder_idx.push(bidx);
        });
        let result = solve(&viewport, &builder.build());
        (0..count)
            .flat_map(|i| {
                result
                    .rect(NodeId::from_raw(i as u32))
                    .map(|r| {
                        vec![
                            f64::from(r.left().get()),
                            f64::from(r.top().get()),
                            f64::from(r.width().get()),
                            f64::from(r.height().get()),
                        ]
                    })
                    .unwrap_or_default()
            })
            .collect()
    }
}

#[cfg(target_arch = "wasm32")]
mod wasm_exports {
    use wasm_bindgen::prelude::*;

    use crate::wasm::WasmGame;

    #[wasm_bindgen]
    impl WasmGame {
        /// Begin a UI frame (`uiBeginFrame`).
        #[wasm_bindgen(js_name = uiBeginFrame)]
        pub fn ui_begin_frame(&mut self, viewport: &[f64], pointer: &[f64], pressed: bool) {
            self.bridge.ui_begin_frame(viewport, pointer, pressed);
        }

        /// Draw a filled/stroked rectangle (`uiRect`).
        #[wasm_bindgen(js_name = uiRect)]
        pub fn ui_rect(&mut self, bounds: &[f64], fill: u32, stroke: u32, stroke_w: f64) {
            self.bridge.ui_rect(bounds, fill, stroke, stroke_w);
        }

        /// Draw a run of text (`uiText`).
        #[wasm_bindgen(js_name = uiText)]
        pub fn ui_text(&mut self, value: String, pos: &[f64], color: u32, size: f64) {
            self.bridge.ui_text(&value, pos, color, size);
        }

        /// Draw a textured sprite (`uiSprite`).
        #[wasm_bindgen(js_name = uiSprite)]
        pub fn ui_sprite(&mut self, texture: f64, bounds: &[f64]) {
            self.bridge.ui_sprite(texture as u64, bounds);
        }

        /// Draw an immediate-mode button (`uiButton`); returns activation.
        #[wasm_bindgen(js_name = uiButton)]
        pub fn ui_button(
            &mut self,
            bounds: &[f64],
            label: String,
            fill: u32,
            stroke: u32,
            sw: f64,
        ) -> bool {
            self.bridge.ui_button(bounds, &label, fill, stroke, sw)
        }

        /// This frame's viewport `[width, height]` (`uiViewport`).
        #[wasm_bindgen(js_name = uiViewport)]
        pub fn ui_viewport(&self) -> Vec<f64> {
            self.bridge.ui_viewport()
        }

        /// This frame's accumulated draw log as bytes (`uiDrawList`).
        #[wasm_bindgen(js_name = uiDrawList)]
        pub fn ui_draw_list(&self) -> Vec<u8> {
            self.bridge.ui_draw_list()
        }

        /// Solve a responsive flex layout (`uiSolveLayout`), returning flat
        /// `[x, y, w, h]` rects per node in input order.
        #[wasm_bindgen(js_name = uiSolveLayout)]
        pub fn ui_solve_layout(&self, vw: f64, vh: f64, nodes: &[f64]) -> Vec<f64> {
            self.bridge.ui_solve_layout(vw, vh, nodes)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{demo_app, GameBridge};

    const STEP: u64 = 1_000_000;

    fn bridge() -> GameBridge {
        GameBridge::new(demo_app().build(), 0, STEP, 1)
    }

    /// Drive a scripted UI frame and return its draw-log bytes + button activation.
    fn ui_frame_bytes(pointer_pressed: bool) -> (Vec<u8>, bool) {
        let mut b = bridge();
        b.ui_begin_frame(&[320.0, 240.0], &[110.0, 60.0], pointer_pressed);
        b.ui_rect(&[0.0, 0.0, 320.0, 240.0], 0x1020_30ff, 0x0000_00ff, 1.0);
        b.ui_text("hp", &[8.0, 8.0], 0xffff_ffff, 12.0);
        b.ui_sprite(7, &[10.0, 10.0, 16.0, 16.0]);
        // The button sits under the pointer (110,60), so it activates on a press edge.
        let activated = b.ui_button(&[100.0, 50.0, 40.0, 20.0], "ok", 0x00ff_00ff, 0x0, 2.0);
        (b.ui_draw_list(), activated)
    }

    #[test]
    fn a_ui_frame_builds_a_deterministic_draw_log_and_button_activates() {
        let (bytes, activated) = ui_frame_bytes(true);
        // The pointer is inside the button on a press edge ⇒ activated.
        assert!(activated);
        // Four items were drawn, so the log is non-empty and replays byte-identically.
        assert!(!bytes.is_empty());
        assert_eq!(ui_frame_bytes(true).0, bytes);
        // No press edge ⇒ the same geometry but the button does not activate, and
        // the activated flag flips the log bytes.
        let (bytes_idle, idle) = ui_frame_bytes(false);
        assert!(!idle);
        assert_ne!(bytes_idle, bytes);
        // begin_frame clears the log: the viewport reads back what was installed.
        let mut b = bridge();
        b.ui_begin_frame(&[320.0, 240.0], &[0.0, 0.0], false);
        assert_eq!(b.ui_viewport(), vec![320.0, 240.0]);
        assert!(b.ui_draw_list().is_empty());
    }

    #[test]
    fn solve_layout_splits_a_row_into_two_equal_children() {
        let b = bridge();
        // A root row (direction=Row, grow=0) with two grow=1 children that should
        // split the 200-wide viewport into two 100-wide columns.
        let nodes = [
            -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, // 0: root row
            0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, // 1: child grow 1
            0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, // 2: child grow 1
        ];
        let rects = b.ui_solve_layout(200.0, 100.0, &nodes);
        // Three nodes × 4 components each.
        assert_eq!(rects.len(), 12);
        // The root spans the whole viewport.
        assert_eq!(&rects[0..4], &[0.0, 0.0, 200.0, 100.0]);
        // The two children each take half the width and tile left-to-right.
        assert_eq!(rects[6], 100.0, "child 0 width");
        assert_eq!(rects[10], 100.0, "child 1 width");
        assert_eq!(rects[8], 100.0, "child 1 starts at x=100");
        // Pure: the same tree solves byte-identically.
        assert_eq!(b.ui_solve_layout(200.0, 100.0, &nodes), rects);
    }
}
