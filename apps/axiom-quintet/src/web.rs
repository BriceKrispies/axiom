//! The in-browser (WASM) play surface — **`wasm32` only**.
//!
//! A thin 2D-`<canvas>` adapter over the pure [`QuintetGame`]. It draws the
//! board, the piece tray (the current quintet in a 5×5 grid), the score, and —
//! while the player drags — a snapped placement preview and the floating piece.
//! **PointerEvent** drag-and-drop (mouse, touch, and pen alike): press on the
//! **board** to summon the waiting quintet — it hovers under the cursor — or
//! press the tray piece to pick it up classically. Either way, drag over the
//! board (the preview snaps to the nearest cell and reads green when valid / red
//! when not) and release to drop; releasing anywhere invalid returns the piece
//! to the tray.
//!
//! The on-screen placement is **engine-decided**: each frame the `axiom-layout`
//! solver, driven from the live window size + orientation + safe-area, places a
//! board region and a tray region — side-by-side in landscape, the tray stacked
//! *below* a larger board in portrait — and we draw each part into its rectangle.
//!
//! Every rule lives in the browser-free core; this file makes no gameplay
//! decisions of its own. It is never compiled on native, so the core and
//! `cargo test` stay DOM-free.

use std::cell::RefCell;
use std::rc::Rc;

use axiom_host::{HostSafeAreaInsets, HostViewport, Pixels};
use axiom_kernel::Ratio;
use axiom_layout::{
    solve, Direction, Insets, LayoutRect, LayoutStyle, LayoutTree, LayoutTreeBuilder, NodeId,
};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, PointerEvent};

use crate::board::BOARD_SIZE;
use crate::game::{PlaceResult, QuintetGame};
use crate::quintet::QuintetMask;

/// The board canvas element id (must match `web/index.html`).
const CANVAS_ID: &str = "axiom-quintet-canvas";
/// The status / stuck-message element id.
const STATUS_ID: &str = "status";

/// How far (in cells, Chebyshev) the drag shadow hunts for a valid spot before
/// giving up and showing red — the "magnetic" snap range.
const SNAP_RADIUS: i32 = 2;

/// Layout node ids for the engine layout tree.
const NODE_ROOT: u32 = 0;
const NODE_BOARD: u32 = 1;
const NODE_TRAY: u32 = 2;

const BG: &str = "#15171c";
const GRID_LINE: &str = "#2a2e36";
const EMPTY_FILL: &str = "#1d212a";
const BLOCK_FILL: &str = "#3f7fe0";
const BLOCK_LIGHT: &str = "#6ea2ff";
const BLOCK_DARK: &str = "#2a589e";
const PIECE_FILL: &str = "#f0a830";
const PIECE_LIGHT: &str = "#ffc864";
const PIECE_DARK: &str = "#b87714";
const VALID_FILL: &str = "#2e8b57";
const INVALID_FILL: &str = "#c0392b";
const TEXT: &str = "#d7dbe2";
const TEXT_DIM: &str = "#9aa3b2";

/// The engine-solved on-screen layout, in canvas backing-store (device) pixels:
/// where the board grid and the 5×5 tray grid sit, and their cell sizes. A larger
/// `mini` (tray cell) in portrait is what makes the piece bigger on a phone.
#[derive(Clone, Copy)]
struct Layout {
    canvas_w: f64,
    canvas_h: f64,
    board_x: f64,
    board_y: f64,
    cell: f64,
    tray_x: f64,
    tray_y: f64,
    mini: f64,
}

impl Layout {
    /// A 1×1 placeholder used before the first solve (and as a div-by-zero guard).
    const PLACEHOLDER: Layout = Layout {
        canvas_w: 1.0,
        canvas_h: 1.0,
        board_x: 0.0,
        board_y: 0.0,
        cell: 1.0,
        tray_x: 0.0,
        tray_y: 0.0,
        mini: 1.0,
    };

    /// Baseline for the "NEXT QUINTET" label, just above the tray grid.
    fn label_y(&self) -> f64 {
        self.tray_y - self.mini * 0.35
    }

    /// Top of the score read-out, below the tray grid.
    fn score_y(&self) -> f64 {
        self.tray_y + self.mini * 5.0 + self.mini * 0.9
    }
}

/// The interactive UI state: the pure game plus the current drag.
struct Ui {
    game: QuintetGame,
    /// Whether the player is currently dragging the piece.
    dragging: bool,
    /// Pointer position in canvas backing-store coordinates.
    pointer: (f64, f64),
}

thread_local! {
    static UI: RefCell<Ui> = RefCell::new(Ui {
        game: QuintetGame::new(),
        dragging: false,
        pointer: (0.0, 0.0),
    });

    /// The current engine-solved layout, recomputed when the window size changes.
    static LAYOUT: RefCell<Layout> = const { RefCell::new(Layout::PLACEHOLDER) };

    /// The last `(css_w, css_h)` the layout was solved for, so it is recomputed +
    /// the canvas resized only when the window changes.
    static LAST_SIZE: RefCell<Option<(u32, u32)>> = const { RefCell::new(None) };
}

fn window() -> web_sys::Window {
    web_sys::window().expect("a browser window")
}

fn document() -> web_sys::Document {
    window().document().expect("a document")
}

fn canvas_context() -> Option<(HtmlCanvasElement, CanvasRenderingContext2d)> {
    let canvas: HtmlCanvasElement = document().get_element_by_id(CANVAS_ID)?.dyn_into().ok()?;
    let ctx: CanvasRenderingContext2d = canvas.get_context("2d").ok()??.dyn_into().ok()?;
    Some((canvas, ctx))
}

#[wasm_bindgen]
pub fn quintet_start() {
    console_error_panic_hook::set_once();
    install_pointer();
    start_run_loop();
    refresh_status();
    log("ready");
}

/// Reset to a fresh game (called by the page's reset button). Namespaced like
/// `quintet_start`: the merged gallery crate shares one wasm export namespace
/// across every demo, so a bare `reset`/`undo` here would collide with (or bait
/// a future collision from) another demo's export.
#[wasm_bindgen]
pub fn quintet_reset() {
    UI.with(|ui| {
        let mut ui = ui.borrow_mut();
        ui.game.reset();
        ui.dragging = false;
    });
    refresh_status();
}

/// Rewind the last placement — including restoring any lines it cleared — and
/// put the exact same piece back in the tray (called by the page's undo
/// button). A no-op when nothing has been placed yet.
#[wasm_bindgen]
pub fn quintet_undo() {
    UI.with(|ui| {
        let mut ui = ui.borrow_mut();
        ui.game.undo();
        ui.dragging = false;
    });
    refresh_status();
}

fn log(msg: &str) {
    web_sys::console::log_1(&JsValue::from_str(&format!("[quintet] {msg}")));
}

/// Recomputed only when the window size changed. The engine's `axiom-layout`
/// solver decides where the board and tray go (row in landscape, column — tray
/// below the board — in portrait); we project the solved rects into canvas
/// backing-store pixels.
fn sync_layout() {
    let Some((canvas, _)) = canvas_context() else {
        return;
    };
    let win = window();
    let css_w = win
        .inner_width()
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(1.0)
        .max(1.0);
    let css_h = win
        .inner_height()
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(1.0)
        .max(1.0);
    let key = (css_w as u32, css_h as u32);
    if LAST_SIZE.with(|last| *last.borrow() == Some(key)) {
        return;
    }
    LAST_SIZE.with(|last| *last.borrow_mut() = Some(key));

    let dpr = win.device_pixel_ratio().max(0.5);
    let cw = (css_w * dpr).max(1.0) as u32;
    let ch = (css_h * dpr).max(1.0) as u32;
    canvas.set_width(cw);
    canvas.set_height(ch);

    let scale = Ratio::new(dpr as f32).unwrap_or_else(|_| Ratio::new(1.0).expect("unit ratio"));
    let viewport = HostViewport::new(css_w as u32, css_h as u32, scale)
        .expect("a positive viewport is valid")
        .with_safe_area_insets(read_safe_area_insets());
    let result = solve(&viewport, &build_layout_tree());
    let board = rect_device_px(result.rect(NodeId::from_raw(NODE_BOARD)), dpr);
    let tray = rect_device_px(result.rect(NodeId::from_raw(NODE_TRAY)), dpr);

    // The board node carries aspect 1.0, so its rect is square: cell = side / 10.
    let cell = board.2 / BOARD_SIZE as f64;
    // Fit a centred 5×5 grid in the tray, leaving a margin for the label + score.
    let mini = ((tray.2 * 0.92).min(tray.3 * 0.62) / 5.0).max(1.0);
    let grid = mini * 5.0;
    let tray_x = tray.0 + (tray.2 - grid) * 0.5;
    let tray_y = tray.1 + (tray.3 - grid) * 0.5;

    LAYOUT.with(|layout| {
        *layout.borrow_mut() = Layout {
            canvas_w: cw as f64,
            canvas_h: ch as f64,
            board_x: board.0,
            board_y: board.1,
            cell,
            tray_x,
            tray_y,
            mini,
        };
    });
}

/// A solved rect → `(x, y, w, h)` in device pixels (logical × dpr), or a 1×1
/// placeholder if the node was not placed.
fn rect_device_px(rect: Option<LayoutRect>, dpr: f64) -> (f64, f64, f64, f64) {
    rect.map(|r| {
        (
            r.left().get() as f64 * dpr,
            r.top().get() as f64 * dpr,
            r.width().get() as f64 * dpr,
            r.height().get() as f64 * dpr,
        )
    })
    .unwrap_or((0.0, 0.0, 1.0, 1.0))
}

/// The layout tree: an adaptive root (board beside tray in landscape, board above
/// tray in portrait). The board grows and stays square (the 10×10 grid); the tray
/// takes a smaller share. The root reserves space at the top for the HUD overlay.
fn build_layout_tree() -> LayoutTree {
    let px = |v: f32| Pixels::new(v).expect("finite pixel length");
    let mut builder = LayoutTreeBuilder::new();

    let mut root = LayoutStyle::new();
    root.direction = Direction::Adaptive;
    root.gap = px(14.0);
    // Top inset clears the HUD bar (status + reset); the rest is a small margin.
    root.padding = Insets::new(px(52.0), px(12.0), px(12.0), px(12.0));
    let root_id = builder.root(NodeId::from_raw(NODE_ROOT), root);

    let mut board = LayoutStyle::new();
    board.grow = Ratio::new(3.0).expect("finite grow");
    board.aspect = Ratio::new(1.0).ok();
    board.min_main = px(120.0);
    builder.child(root_id, NodeId::from_raw(NODE_BOARD), board);

    let mut tray = LayoutStyle::new();
    tray.grow = Ratio::new(2.0).expect("finite grow");
    tray.min_main = px(120.0);
    builder.child(root_id, NodeId::from_raw(NODE_TRAY), tray);

    builder.build()
}

/// Read the device safe-area insets via the CSS `env(safe-area-inset-*)` values by
/// measuring a hidden probe element. Falls back to no insets on any failure.
fn read_safe_area_insets() -> HostSafeAreaInsets {
    let doc = document();
    let values = doc
        .create_element("div")
        .ok()
        .map(|probe| {
            let _ = probe.set_attribute(
                "style",
                "position:fixed;visibility:hidden;top:0;left:0;\
                 padding-top:env(safe-area-inset-top);padding-right:env(safe-area-inset-right);\
                 padding-bottom:env(safe-area-inset-bottom);padding-left:env(safe-area-inset-left);",
            );
            let _ = doc.body().map(|b| b.append_child(&probe));
            let read = window()
                .get_computed_style(&probe)
                .ok()
                .flatten()
                .map(|cs| {
                    let edge = |name: &str| {
                        cs.get_property_value(name)
                            .ok()
                            .and_then(|v| v.trim_end_matches("px").trim().parse::<f32>().ok())
                            .unwrap_or(0.0)
                    };
                    (
                        edge("padding-top"),
                        edge("padding-right"),
                        edge("padding-bottom"),
                        edge("padding-left"),
                    )
                })
                .unwrap_or((0.0, 0.0, 0.0, 0.0));
            let _ = doc.body().map(|b| b.remove_child(&probe));
            read
        })
        .unwrap_or((0.0, 0.0, 0.0, 0.0));
    let edge = |v: f32| Pixels::new(v.max(0.0)).unwrap_or_else(|_| Pixels::new(0.0).expect("zero"));
    HostSafeAreaInsets::new(
        edge(values.0),
        edge(values.1),
        edge(values.2),
        edge(values.3),
    )
    .unwrap_or_else(|_| HostSafeAreaInsets::none())
}

/// Convert a pointer event's client coordinates into canvas backing-store
/// coordinates, accounting for any CSS scaling of the canvas element.
fn to_canvas_coords(canvas: &HtmlCanvasElement, e: &PointerEvent) -> (f64, f64) {
    let rect = canvas.get_bounding_client_rect();
    let sx = canvas.width() as f64 / rect.width().max(1.0);
    let sy = canvas.height() as f64 / rect.height().max(1.0);
    (
        (e.client_x() as f64 - rect.left()) * sx,
        (e.client_y() as f64 - rect.top()) * sy,
    )
}

/// Is the canvas point inside the tray's 5×5 grid (the pick-up zone)?
fn in_generator(px: f64, py: f64, layout: &Layout) -> bool {
    px >= layout.tray_x
        && px < layout.tray_x + layout.mini * 5.0
        && py >= layout.tray_y
        && py < layout.tray_y + layout.mini * 5.0
}

/// Is the canvas point inside the 10×10 board grid (the press-to-summon zone)?
fn in_board(px: f64, py: f64, layout: &Layout) -> bool {
    let span = layout.cell * BOARD_SIZE as f64;
    px >= layout.board_x
        && px < layout.board_x + span
        && py >= layout.board_y
        && py < layout.board_y + span
}

/// The board cell under a canvas point (may be out of bounds).
fn board_cell(px: f64, py: f64, layout: &Layout) -> (i32, i32) {
    (
        ((px - layout.board_x) / layout.cell).floor() as i32,
        ((py - layout.board_y) / layout.cell).floor() as i32,
    )
}

/// The snapped board anchor for the dragged piece (centred on the cell under the
/// pointer).
fn snapped_anchor(mask: &QuintetMask, px: f64, py: f64, layout: &Layout) -> (i32, i32) {
    let (cx, cy) = board_cell(px, py, layout);
    (cx - mask.width() / 2, cy - mask.height() / 2)
}

/// Install the PointerEvent drag handlers on the canvas, with pointer capture so a
/// touch/mouse drag keeps tracking even if it leaves the element. Works for mouse,
/// touch, and pen with one code path.
fn install_pointer() {
    let Some((canvas, _)) = canvas_context() else {
        return;
    };

    // Press on the board (summon the waiting piece under the cursor) or on the
    // tray piece (classic pick-up) → capture the pointer and start dragging.
    let down_canvas = canvas.clone();
    let down = Closure::<dyn FnMut(PointerEvent)>::new(move |e: PointerEvent| {
        e.prevent_default();
        let _ = down_canvas.set_pointer_capture(e.pointer_id());
        let (px, py) = to_canvas_coords(&down_canvas, &e);
        let layout = LAYOUT.with(|l| *l.borrow());
        UI.with(|ui| {
            let mut ui = ui.borrow_mut();
            ui.pointer = (px, py);
            if !ui.game.is_stuck()
                && (in_board(px, py, &layout) || in_generator(px, py, &layout))
            {
                ui.dragging = true;
            }
        });
    });
    canvas
        .add_event_listener_with_callback("pointerdown", down.as_ref().unchecked_ref())
        .expect("pointerdown listener installs");
    down.forget();

    // Move → update the pointer (capture routes moves here even off-element).
    let move_canvas = canvas.clone();
    let moved = Closure::<dyn FnMut(PointerEvent)>::new(move |e: PointerEvent| {
        let (px, py) = to_canvas_coords(&move_canvas, &e);
        UI.with(|ui| ui.borrow_mut().pointer = (px, py));
    });
    canvas
        .add_event_listener_with_callback("pointermove", moved.as_ref().unchecked_ref())
        .expect("pointermove listener installs");
    moved.forget();

    // Release → drop the piece if the snapped placement is valid.
    let up_canvas = canvas.clone();
    let up = Closure::<dyn FnMut(PointerEvent)>::new(move |e: PointerEvent| {
        let _ = up_canvas.release_pointer_capture(e.pointer_id());
        let (px, py) = to_canvas_coords(&up_canvas, &e);
        let layout = LAYOUT.with(|l| *l.borrow());
        let placed = UI.with(|ui| {
            let mut ui = ui.borrow_mut();
            ui.pointer = (px, py);
            if !ui.dragging {
                return false;
            }
            ui.dragging = false;
            let desired = ui
                .game
                .current()
                .map(|mask| snapped_anchor(mask, px, py, &layout));
            match desired.and_then(|d| ui.game.snap_anchor(d, SNAP_RADIUS)) {
                Some((ax, ay)) => matches!(ui.game.try_place(ax, ay), PlaceResult::Placed(_)),
                None => false,
            }
        });
        if placed {
            refresh_status();
        }
    });
    canvas
        .add_event_listener_with_callback("pointerup", up.as_ref().unchecked_ref())
        .expect("pointerup listener installs");
    up.forget();

    // Cancel (e.g. a touch interrupted) → abandon the drag.
    let cancel = Closure::<dyn FnMut(PointerEvent)>::new(move |_e: PointerEvent| {
        UI.with(|ui| ui.borrow_mut().dragging = false);
    });
    canvas
        .add_event_listener_with_callback("pointercancel", cancel.as_ref().unchecked_ref())
        .expect("pointercancel listener installs");
    cancel.forget();
}

fn start_run_loop() {
    let frame = Rc::new(RefCell::new(None::<Closure<dyn FnMut()>>));
    let frame_outer = frame.clone();
    *frame_outer.borrow_mut() = Some(Closure::<dyn FnMut()>::new(move || {
        sync_layout();
        let layout = LAYOUT.with(|l| *l.borrow());
        UI.with(|ui| draw(&ui.borrow(), &layout));
        request_frame(frame.borrow().as_ref());
    }));
    request_frame(frame_outer.borrow().as_ref());
}

fn request_frame(cb: Option<&Closure<dyn FnMut()>>) {
    if let Some(cb) = cb {
        let _ = window().request_animation_frame(cb.as_ref().unchecked_ref());
    }
}

fn draw(ui: &Ui, layout: &Layout) {
    let Some((_, ctx)) = canvas_context() else {
        return;
    };

    ctx.set_global_alpha(1.0);
    ctx.set_fill_style_str(BG);
    ctx.fill_rect(0.0, 0.0, layout.canvas_w, layout.canvas_h);

    draw_board(&ctx, ui, layout);
    draw_drag_preview(&ctx, ui, layout);
    draw_generator(&ctx, ui, layout);
    draw_floating_piece(&ctx, ui, layout);
    draw_score(&ctx, ui, layout);
}

/// A single beveled block (filled board cell or piece cell).
fn draw_block(
    ctx: &CanvasRenderingContext2d,
    x: f64,
    y: f64,
    size: f64,
    fill: &str,
    light: &str,
    dark: &str,
) {
    let inset = (size * 0.03).max(1.0);
    let px = x + inset;
    let py = y + inset;
    let s = size - inset * 2.0;
    let bevel = s * 0.18;

    ctx.set_fill_style_str(fill);
    ctx.fill_rect(px, py, s, s);
    // Top + left highlight.
    ctx.set_fill_style_str(light);
    ctx.fill_rect(px, py, s, bevel);
    ctx.fill_rect(px, py, bevel, s);
    // Bottom + right shadow.
    ctx.set_fill_style_str(dark);
    ctx.fill_rect(px, py + s - bevel, s, bevel);
    ctx.fill_rect(px + s - bevel, py, bevel, s);
}

fn draw_board(ctx: &CanvasRenderingContext2d, ui: &Ui, layout: &Layout) {
    let cell = layout.cell;
    ctx.set_global_alpha(1.0);
    for y in 0..BOARD_SIZE as i32 {
        for x in 0..BOARD_SIZE as i32 {
            let px = layout.board_x + x as f64 * cell;
            let py = layout.board_y + y as f64 * cell;
            if ui.game.board().is_filled(x, y) {
                draw_block(ctx, px, py, cell, BLOCK_FILL, BLOCK_LIGHT, BLOCK_DARK);
            } else {
                ctx.set_fill_style_str(EMPTY_FILL);
                ctx.fill_rect(px + 1.0, py + 1.0, cell - 2.0, cell - 2.0);
            }
        }
    }
    // Board frame.
    let span = cell * BOARD_SIZE as f64;
    ctx.set_stroke_style_str(GRID_LINE);
    ctx.set_line_width(1.0);
    ctx.stroke_rect(
        layout.board_x - 0.5,
        layout.board_y - 0.5,
        span + 1.0,
        span + 1.0,
    );
}

/// While dragging, paint the magnetic placement shadow onto the board (green at
/// the nearest valid anchor within [`SNAP_RADIUS`], else red at the cursor).
fn draw_drag_preview(ctx: &CanvasRenderingContext2d, ui: &Ui, layout: &Layout) {
    if !ui.dragging {
        return;
    }
    let Some(mask) = ui.game.current() else {
        return;
    };
    let desired = snapped_anchor(mask, ui.pointer.0, ui.pointer.1, layout);
    let (anchor, color) = match ui.game.snap_anchor(desired, SNAP_RADIUS) {
        Some(snapped) => (snapped, VALID_FILL),
        None => (desired, INVALID_FILL),
    };
    let cell = layout.cell;
    ctx.set_global_alpha(0.55);
    ctx.set_fill_style_str(color);
    for &(mx, my) in mask.cells() {
        let (bx, by) = (anchor.0 + mx, anchor.1 + my);
        if bx >= 0 && by >= 0 && bx < BOARD_SIZE as i32 && by < BOARD_SIZE as i32 {
            let px = layout.board_x + bx as f64 * cell;
            let py = layout.board_y + by as f64 * cell;
            ctx.fill_rect(px + 1.0, py + 1.0, cell - 2.0, cell - 2.0);
        }
    }
    ctx.set_global_alpha(1.0);
}

/// The tray: a label and the current quintet in a 5×5 grid (or a stuck message).
fn draw_generator(ctx: &CanvasRenderingContext2d, ui: &Ui, layout: &Layout) {
    let mini = layout.mini;
    ctx.set_global_alpha(1.0);
    ctx.set_fill_style_str(TEXT_DIM);
    ctx.set_font(&format!(
        "600 {}px system-ui, sans-serif",
        (mini * 0.42).max(11.0) as i32
    ));
    let _ = ctx.fill_text("NEXT QUINTET", layout.tray_x, layout.label_y());

    // The 5×5 backing grid.
    for gy in 0..5 {
        for gx in 0..5 {
            let px = layout.tray_x + gx as f64 * mini;
            let py = layout.tray_y + gy as f64 * mini;
            ctx.set_fill_style_str(EMPTY_FILL);
            ctx.fill_rect(px + 1.0, py + 1.0, mini - 2.0, mini - 2.0);
        }
    }

    match ui.game.current() {
        Some(mask) => {
            // Centre the shape within the 5×5 preview.
            let off_x = (5 - mask.width()) / 2;
            let off_y = (5 - mask.height()) / 2;
            ctx.set_global_alpha(if ui.dragging { 0.3 } else { 1.0 });
            for &(mx, my) in mask.cells() {
                let px = layout.tray_x + (off_x + mx) as f64 * mini;
                let py = layout.tray_y + (off_y + my) as f64 * mini;
                draw_block(ctx, px, py, mini, PIECE_FILL, PIECE_LIGHT, PIECE_DARK);
            }
            ctx.set_global_alpha(1.0);
        }
        None => {
            ctx.set_fill_style_str(INVALID_FILL);
            ctx.set_font(&format!(
                "600 {}px system-ui, sans-serif",
                (mini * 0.5).max(12.0) as i32
            ));
            let _ = ctx.fill_text(
                "No quintet fits — press Reset",
                layout.tray_x,
                layout.tray_y + mini * 2.5,
            );
        }
    }
}

/// While dragging, draw a translucent copy of the piece following the cursor at
/// board-cell size, so the interaction reads clearly.
fn draw_floating_piece(ctx: &CanvasRenderingContext2d, ui: &Ui, layout: &Layout) {
    if !ui.dragging {
        return;
    }
    let Some(mask) = ui.game.current() else {
        return;
    };
    let cell = layout.cell;
    let origin_x = ui.pointer.0 - mask.width() as f64 * cell / 2.0;
    let origin_y = ui.pointer.1 - mask.height() as f64 * cell / 2.0;
    ctx.set_global_alpha(0.7);
    for &(mx, my) in mask.cells() {
        let px = origin_x + mx as f64 * cell;
        let py = origin_y + my as f64 * cell;
        draw_block(ctx, px, py, cell, PIECE_FILL, PIECE_LIGHT, PIECE_DARK);
    }
    ctx.set_global_alpha(1.0);
}

fn draw_score(ctx: &CanvasRenderingContext2d, ui: &Ui, layout: &Layout) {
    let mini = layout.mini;
    let y = layout.score_y();
    ctx.set_global_alpha(1.0);
    ctx.set_fill_style_str(TEXT_DIM);
    ctx.set_font(&format!(
        "600 {}px system-ui, sans-serif",
        (mini * 0.42).max(11.0) as i32
    ));
    let _ = ctx.fill_text("SCORE", layout.tray_x, y);
    ctx.set_fill_style_str(TEXT);
    ctx.set_font(&format!(
        "700 {}px system-ui, sans-serif",
        (mini * 1.1).max(24.0) as i32
    ));
    let _ = ctx.fill_text(&ui.game.score().to_string(), layout.tray_x, y + mini * 1.3);
}

fn refresh_status() {
    let stuck = UI.with(|ui| ui.borrow().game.is_stuck());
    if let Some(el) = document().get_element_by_id(STATUS_ID) {
        let msg = if stuck {
            "No quintet fits anywhere — press Reset to start over."
        } else {
            "Press the board to place the quintet (or drag it from the tray). Fill rows and columns to clear them."
        };
        el.set_text_content(Some(msg));
    }
}
