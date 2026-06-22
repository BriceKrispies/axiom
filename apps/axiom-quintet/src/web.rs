//! The in-browser (WASM) play surface — **`wasm32` only**.
//!
//! A thin 2D-`<canvas>` adapter over the pure [`QuintetGame`]. It draws the
//! board, the generator panel (the current quintet in a 5×5 mini-grid), the
//! score, and — while the player drags — a snapped placement preview and the
//! floating piece. Pointer drag-and-drop is the only input: press on the
//! generator piece to pick it up, drag over the board (the preview snaps to the
//! nearest cell and reads green when valid / red when not), and release to drop.
//! A valid drop commits the piece; an invalid drop returns it to the panel.
//!
//! Every rule lives in the browser-free core; this file makes no gameplay
//! decisions of its own. It is never compiled on native, so the core and
//! `cargo test` stay DOM-free.

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, MouseEvent};

use crate::board::BOARD_SIZE;
use crate::game::{PlaceResult, QuintetGame};
use crate::quintet::QuintetMask;

/// The board canvas element id (must match `web/index.html`).
const CANVAS_ID: &str = "axiom-quintet-canvas";
/// The status / stuck-message element id.
const STATUS_ID: &str = "status";

/// Pixels per board cell.
const CELL_PX: f64 = 44.0;
/// Outer margin around the board.
const MARGIN: f64 = 24.0;
/// Gap between the board and the generator panel.
const PANEL_GAP: f64 = 48.0;
/// Pixels per cell in the 5×5 generator mini-grid.
const MINI_PX: f64 = 28.0;

/// The board's pixel span.
const BOARD_PX: f64 = CELL_PX * BOARD_SIZE as f64;
/// Left edge of the generator panel.
const PANEL_X: f64 = MARGIN + BOARD_PX + PANEL_GAP;
/// Top of the generator mini-grid (room above for its label).
const PANEL_GRID_Y: f64 = MARGIN + 36.0;

/// Canvas backing-store width and height.
const CANVAS_W: u32 = (PANEL_X + MINI_PX * 5.0 + MARGIN) as u32;
const CANVAS_H: u32 = (MARGIN * 2.0 + BOARD_PX) as u32;

// --- Palette --------------------------------------------------------------
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

// ===========================================================================
// Browser entry point.
// ===========================================================================

/// Boot the play surface: size the canvas, install pointer handlers and the
/// reset hook, start the draw loop, and refresh the status line.
#[wasm_bindgen]
pub fn start() {
    console_error_panic_hook::set_once();
    if let Some((canvas, _)) = canvas_context() {
        canvas.set_width(CANVAS_W);
        canvas.set_height(CANVAS_H);
    }
    install_pointer();
    start_run_loop();
    refresh_status();
    log("ready");
}

/// Reset to a fresh game (called by the page's reset button).
#[wasm_bindgen]
pub fn reset() {
    UI.with(|ui| {
        let mut ui = ui.borrow_mut();
        ui.game.reset();
        ui.dragging = false;
    });
    refresh_status();
}

fn log(msg: &str) {
    web_sys::console::log_1(&JsValue::from_str(&format!("[quintet] {msg}")));
}

// ===========================================================================
// Pointer drag-and-drop.
// ===========================================================================

/// Convert a mouse event's client coordinates into canvas backing-store
/// coordinates, accounting for any CSS scaling of the canvas element.
fn to_canvas_coords(canvas: &HtmlCanvasElement, e: &MouseEvent) -> (f64, f64) {
    let rect = canvas.get_bounding_client_rect();
    let sx = canvas.width() as f64 / rect.width();
    let sy = canvas.height() as f64 / rect.height();
    (
        (e.client_x() as f64 - rect.left()) * sx,
        (e.client_y() as f64 - rect.top()) * sy,
    )
}

/// Is the canvas point inside the generator mini-grid (the pick-up zone)?
fn in_generator(px: f64, py: f64) -> bool {
    px >= PANEL_X
        && px < PANEL_X + MINI_PX * 5.0
        && py >= PANEL_GRID_Y
        && py < PANEL_GRID_Y + MINI_PX * 5.0
}

/// The board cell under a canvas point (may be out of bounds).
fn board_cell(px: f64, py: f64) -> (i32, i32) {
    (
        ((px - MARGIN) / CELL_PX).floor() as i32,
        ((py - MARGIN) / CELL_PX).floor() as i32,
    )
}

/// The snapped board anchor for the dragged piece: the piece is centred on the
/// board cell under the pointer.
fn snapped_anchor(mask: &QuintetMask, px: f64, py: f64) -> (i32, i32) {
    let (cx, cy) = board_cell(px, py);
    (cx - mask.width() / 2, cy - mask.height() / 2)
}

/// Install the mouse-based drag handlers: pick up on the generator piece, track
/// the drag on the window, and drop on release.
fn install_pointer() {
    let Some((canvas, _)) = canvas_context() else {
        return;
    };

    // Press on the generator piece → start dragging.
    let down_canvas = canvas.clone();
    let down = Closure::<dyn FnMut(MouseEvent)>::new(move |e: MouseEvent| {
        let (px, py) = to_canvas_coords(&down_canvas, &e);
        UI.with(|ui| {
            let mut ui = ui.borrow_mut();
            ui.pointer = (px, py);
            if !ui.game.is_stuck() && in_generator(px, py) {
                ui.dragging = true;
            }
        });
    });
    canvas
        .add_event_listener_with_callback("mousedown", down.as_ref().unchecked_ref())
        .expect("mousedown listener installs");
    down.forget();

    // Move anywhere → update the pointer (drag follows the cursor off-canvas too).
    let move_canvas = canvas.clone();
    let moved = Closure::<dyn FnMut(MouseEvent)>::new(move |e: MouseEvent| {
        let (px, py) = to_canvas_coords(&move_canvas, &e);
        UI.with(|ui| ui.borrow_mut().pointer = (px, py));
    });
    window()
        .add_event_listener_with_callback("mousemove", moved.as_ref().unchecked_ref())
        .expect("mousemove listener installs");
    moved.forget();

    // Release → drop the piece if the snapped placement is valid.
    let up_canvas = canvas.clone();
    let up = Closure::<dyn FnMut(MouseEvent)>::new(move |e: MouseEvent| {
        let (px, py) = to_canvas_coords(&up_canvas, &e);
        let placed = UI.with(|ui| {
            let mut ui = ui.borrow_mut();
            ui.pointer = (px, py);
            if !ui.dragging {
                return false;
            }
            ui.dragging = false;
            let anchor = ui.game.current().map(|mask| snapped_anchor(mask, px, py));
            match anchor {
                Some((ax, ay)) => matches!(ui.game.try_place(ax, ay), PlaceResult::Placed(_)),
                None => false,
            }
        });
        if placed {
            refresh_status();
        }
    });
    window()
        .add_event_listener_with_callback("mouseup", up.as_ref().unchecked_ref())
        .expect("mouseup listener installs");
    up.forget();
}

// ===========================================================================
// The draw loop.
// ===========================================================================

fn start_run_loop() {
    let frame = Rc::new(RefCell::new(None::<Closure<dyn FnMut()>>));
    let frame_outer = frame.clone();
    *frame_outer.borrow_mut() = Some(Closure::<dyn FnMut()>::new(move || {
        UI.with(|ui| draw(&ui.borrow()));
        request_frame(frame.borrow().as_ref());
    }));
    request_frame(frame_outer.borrow().as_ref());
}

fn request_frame(cb: Option<&Closure<dyn FnMut()>>) {
    if let Some(cb) = cb {
        let _ = window().request_animation_frame(cb.as_ref().unchecked_ref());
    }
}

// ===========================================================================
// Rendering.
// ===========================================================================

fn draw(ui: &Ui) {
    let Some((_, ctx)) = canvas_context() else {
        return;
    };

    ctx.set_global_alpha(1.0);
    ctx.set_fill_style_str(BG);
    ctx.fill_rect(0.0, 0.0, CANVAS_W as f64, CANVAS_H as f64);

    draw_board(&ctx, ui);
    draw_drag_preview(&ctx, ui);
    draw_generator(&ctx, ui);
    draw_floating_piece(&ctx, ui);
    draw_score(&ctx, ui);
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
    let inset = 1.0;
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

fn draw_board(ctx: &CanvasRenderingContext2d, ui: &Ui) {
    ctx.set_global_alpha(1.0);
    for y in 0..BOARD_SIZE as i32 {
        for x in 0..BOARD_SIZE as i32 {
            let px = MARGIN + x as f64 * CELL_PX;
            let py = MARGIN + y as f64 * CELL_PX;
            if ui.game.board().is_filled(x, y) {
                draw_block(ctx, px, py, CELL_PX, BLOCK_FILL, BLOCK_LIGHT, BLOCK_DARK);
            } else {
                ctx.set_fill_style_str(EMPTY_FILL);
                ctx.fill_rect(px + 1.0, py + 1.0, CELL_PX - 2.0, CELL_PX - 2.0);
            }
        }
    }
    // Board frame.
    ctx.set_stroke_style_str(GRID_LINE);
    ctx.set_line_width(1.0);
    ctx.stroke_rect(MARGIN - 0.5, MARGIN - 0.5, BOARD_PX + 1.0, BOARD_PX + 1.0);
}

/// While dragging, paint the snapped placement preview onto the board — green
/// when the whole placement is valid, red otherwise.
fn draw_drag_preview(ctx: &CanvasRenderingContext2d, ui: &Ui) {
    if !ui.dragging {
        return;
    }
    let Some(mask) = ui.game.current() else {
        return;
    };
    let (ax, ay) = snapped_anchor(mask, ui.pointer.0, ui.pointer.1);
    let valid = ui.game.can_place_current(ax, ay);
    let color = if valid { VALID_FILL } else { INVALID_FILL };
    ctx.set_global_alpha(0.55);
    ctx.set_fill_style_str(color);
    for &(mx, my) in mask.cells() {
        let (bx, by) = (ax + mx, ay + my);
        if bx >= 0 && by >= 0 && bx < BOARD_SIZE as i32 && by < BOARD_SIZE as i32 {
            let px = MARGIN + bx as f64 * CELL_PX;
            let py = MARGIN + by as f64 * CELL_PX;
            ctx.fill_rect(px + 1.0, py + 1.0, CELL_PX - 2.0, CELL_PX - 2.0);
        }
    }
    ctx.set_global_alpha(1.0);
}

/// The generator panel: a label and the current quintet in a 5×5 mini-grid (or a
/// stuck message).
fn draw_generator(ctx: &CanvasRenderingContext2d, ui: &Ui) {
    ctx.set_global_alpha(1.0);
    ctx.set_fill_style_str(TEXT_DIM);
    ctx.set_font("600 13px system-ui, sans-serif");
    let _ = ctx.fill_text("NEXT QUINTET", PANEL_X, MARGIN + 16.0);

    // The 5×5 backing grid.
    for gy in 0..5 {
        for gx in 0..5 {
            let px = PANEL_X + gx as f64 * MINI_PX;
            let py = PANEL_GRID_Y + gy as f64 * MINI_PX;
            ctx.set_fill_style_str(EMPTY_FILL);
            ctx.fill_rect(px + 1.0, py + 1.0, MINI_PX - 2.0, MINI_PX - 2.0);
        }
    }

    match ui.game.current() {
        Some(mask) => {
            // Centre the shape within the 5×5 preview.
            let off_x = (5 - mask.width()) / 2;
            let off_y = (5 - mask.height()) / 2;
            // Dim the panel piece while it is being dragged out.
            ctx.set_global_alpha(if ui.dragging { 0.3 } else { 1.0 });
            for &(mx, my) in mask.cells() {
                let px = PANEL_X + (off_x + mx) as f64 * MINI_PX;
                let py = PANEL_GRID_Y + (off_y + my) as f64 * MINI_PX;
                draw_block(ctx, px, py, MINI_PX, PIECE_FILL, PIECE_LIGHT, PIECE_DARK);
            }
            ctx.set_global_alpha(1.0);
        }
        None => {
            ctx.set_fill_style_str(INVALID_FILL);
            ctx.set_font("600 14px system-ui, sans-serif");
            let _ = ctx.fill_text("No quintet fits", PANEL_X, PANEL_GRID_Y + MINI_PX * 2.5);
            let _ = ctx.fill_text(
                "— press Reset",
                PANEL_X,
                PANEL_GRID_Y + MINI_PX * 2.5 + 20.0,
            );
        }
    }
}

/// While dragging, draw a translucent copy of the piece following the cursor so
/// the interaction is unmistakable.
fn draw_floating_piece(ctx: &CanvasRenderingContext2d, ui: &Ui) {
    if !ui.dragging {
        return;
    }
    let Some(mask) = ui.game.current() else {
        return;
    };
    let half_w = mask.width() as f64 * CELL_PX / 2.0;
    let half_h = mask.height() as f64 * CELL_PX / 2.0;
    let origin_x = ui.pointer.0 - half_w;
    let origin_y = ui.pointer.1 - half_h;
    ctx.set_global_alpha(0.7);
    for &(mx, my) in mask.cells() {
        let px = origin_x + mx as f64 * CELL_PX;
        let py = origin_y + my as f64 * CELL_PX;
        draw_block(ctx, px, py, CELL_PX, PIECE_FILL, PIECE_LIGHT, PIECE_DARK);
    }
    ctx.set_global_alpha(1.0);
}

fn draw_score(ctx: &CanvasRenderingContext2d, ui: &Ui) {
    let y = PANEL_GRID_Y + MINI_PX * 5.0 + 44.0;
    ctx.set_global_alpha(1.0);
    ctx.set_fill_style_str(TEXT_DIM);
    ctx.set_font("600 13px system-ui, sans-serif");
    let _ = ctx.fill_text("SCORE", PANEL_X, y);
    ctx.set_fill_style_str(TEXT);
    ctx.set_font("700 34px system-ui, sans-serif");
    let _ = ctx.fill_text(&ui.game.score().to_string(), PANEL_X, y + 36.0);
}

// ===========================================================================
// Status line sync (the page's stuck / playing message).
// ===========================================================================

fn refresh_status() {
    let stuck = UI.with(|ui| ui.borrow().game.is_stuck());
    if let Some(el) = document().get_element_by_id(STATUS_ID) {
        let msg = if stuck {
            "No quintet fits anywhere — press Reset to start over."
        } else {
            "Drag the quintet onto the board. Fill rows and columns to clear them."
        };
        el.set_text_content(Some(msg));
    }
}
