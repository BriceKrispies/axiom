//! The in-browser (WASM) editor + playtest surface — **`wasm32` only**.
//!
//! A thin 2D-`<canvas>` adapter over the pure, deterministic core
//! ([`crate::app::RoomedPuzzleApp`]). It renders the neutral
//! [`crate::render_model::RenderModel`] with top-down depth cues, routes DOM
//! input into core commands, and runs the fixed-step tick loop — and that is
//! *all* it does. Every gameplay rule, every validation, and the level format
//! live in the browser-free core; this file makes no decisions of its own. It is
//! never compiled on native, so the core and `cargo test` stay DOM-free.
//!
//! ## Division of labour with `web/index.html`
//!
//! The page owns the chrome (palette buttons, group/title inputs, mode buttons,
//! the TOML textarea, the validation panel) and calls the small `#[wasm_bindgen]`
//! API below. This file owns the canvas: it installs the canvas-click painter
//! (edit mode) and the keyboard handler (playtest mode), drives the
//! `requestAnimationFrame` loop that advances ghost replay at the fixed step, and
//! draws every frame. After an edit it refreshes the validation panel and the
//! "Playtest" button so the page stays in sync.
//!
//! ## Fixed step in the shell, not the core
//!
//! The run loop reads the `requestAnimationFrame` timestamp (wall clock) and
//! converts elapsed real time into a whole number of `Tick` commands at 60 Hz.
//! The *core* only ever sees `Tick`s, so it never reads a clock — ghost cadence
//! stays deterministic and replayable regardless of the display refresh rate.

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, KeyboardEvent, MouseEvent};

use crate::actor_state::ActorKind;
use crate::app::{Mode, RoomedPuzzleApp};
use crate::game_state::TICKS_PER_SECOND;
use crate::group_id::GroupId;
use crate::render_model::{Elevation, RenderActor, RenderTile};
use crate::tile_kind::TileKind;

/// The board canvas element id (must match `web/index.html`).
const CANVAS_ID: &str = "axiom-puzzle-canvas";
/// The live-validation panel element id.
const VALIDATION_ID: &str = "validation";
/// The "Playtest" button element id (enabled only when the level validates).
const PLAYTEST_BTN_ID: &str = "btn-playtest";

/// Pixels per grid cell on the canvas backing store.
const CELL_PX: f64 = 48.0;
/// Real milliseconds per fixed tick (60 Hz).
const STEP_MS: f64 = 1000.0 / TICKS_PER_SECOND as f64;
/// Most ticks dispatched in one frame, so a long pause can't spiral.
const MAX_TICKS_PER_FRAME: u32 = 8;

thread_local! {
    /// The single app instance, shared across the DOM callbacks (single-threaded
    /// wasm, so a plain `RefCell` is enough).
    static APP: RefCell<RoomedPuzzleApp> = RefCell::new(RoomedPuzzleApp::new());
}

/// Log a line to the browser console, prefixed so it is easy to spot.
fn log(msg: &str) {
    web_sys::console::log_1(&JsValue::from_str(&format!("[roomed-puzzle] {msg}")));
}

fn window() -> web_sys::Window {
    web_sys::window().expect("a browser window")
}

fn document() -> web_sys::Document {
    window().document().expect("a document")
}

/// The board canvas and its 2D context.
fn canvas_context() -> Option<(HtmlCanvasElement, CanvasRenderingContext2d)> {
    let canvas: HtmlCanvasElement = document().get_element_by_id(CANVAS_ID)?.dyn_into().ok()?;
    let ctx: CanvasRenderingContext2d = canvas.get_context("2d").ok()??.dyn_into().ok()?;
    Some((canvas, ctx))
}

// ===========================================================================
// Browser entry point.
// ===========================================================================

/// Boot the editor/playtest surface: install the canvas painter, the keyboard
/// handler, and the fixed-step run loop, then draw the first frame. Called from
/// the page once the wasm module is ready.
#[wasm_bindgen]
pub fn start() {
    console_error_panic_hook::set_once();
    install_canvas_click();
    install_keyboard();
    start_run_loop();
    refresh_editor_ui();
    log("ready");
}

// ===========================================================================
// Editor-facing API (called from the page chrome).
// ===========================================================================

/// Select a palette kind: `"floor"`, `"wall"`, `"entrance"`, `"exit"`,
/// `"button"`, or `"door"`. Unknown values are ignored.
#[wasm_bindgen]
pub fn select_tile(kind: &str) {
    if let Some(kind) = tile_from_str(kind) {
        APP.with(|app| app.borrow_mut().editor_mut().select(kind));
    }
}

/// Set the wiring group new buttons/doors are painted with.
#[wasm_bindgen]
pub fn set_group(group: &str) {
    APP.with(|app| {
        app.borrow_mut()
            .editor_mut()
            .set_paint_group(GroupId::new(group))
    });
    refresh_editor_ui();
}

/// Set the level title.
#[wasm_bindgen]
pub fn set_title(title: &str) {
    APP.with(|app| app.borrow_mut().editor_mut().set_title(title));
}

/// The current level as hand-editable TOML (for the export textarea).
#[wasm_bindgen]
pub fn export_toml() -> String {
    APP.with(|app| {
        app.borrow()
            .editor()
            .to_toml()
            .unwrap_or_else(|e| format!("# could not export level: {e}"))
    })
}

/// Replace the editor from TOML text. Returns `""` on success, or an error
/// message the page can show.
#[wasm_bindgen]
pub fn import_toml(text: &str) -> String {
    let result = APP.with(|app| app.borrow_mut().editor_mut().import_toml(text));
    refresh_editor_ui();
    match result {
        Ok(()) => String::new(),
        Err(e) => e.to_string(),
    }
}

/// The live validation summary (one message per line, or `"Level is valid."`).
#[wasm_bindgen]
pub fn validation_text() -> String {
    APP.with(|app| validation_summary(&app.borrow()))
}

/// May the editor switch to playtest (does the level validate)?
#[wasm_bindgen]
pub fn can_playtest() -> bool {
    APP.with(|app| app.borrow().editor().can_playtest())
}

/// Try to enter playtest. Returns whether the level was valid and the switch
/// happened.
#[wasm_bindgen]
pub fn enter_playtest() -> bool {
    let ok = APP.with(|app| app.borrow_mut().enter_playtest());
    refresh_editor_ui();
    ok
}

/// Return to edit mode, keeping the edited level.
#[wasm_bindgen]
pub fn enter_edit() {
    APP.with(|app| app.borrow_mut().enter_edit());
    refresh_editor_ui();
}

/// The active mode as a string: `"edit"` or `"playtest"`.
#[wasm_bindgen]
pub fn current_mode() -> String {
    APP.with(|app| match app.borrow().mode() {
        Mode::Edit => "edit".to_string(),
        Mode::Playtest => "playtest".to_string(),
    })
}

/// The playtest status line (ghost count, recorded moves, goal/solved).
#[wasm_bindgen]
pub fn status_line() -> String {
    APP.with(|app| {
        app.borrow()
            .playtest()
            .map(|s| s.status_line())
            .unwrap_or_default()
    })
}

// ===========================================================================
// Input plumbing.
// ===========================================================================

/// Map a palette string to a [`TileKind`].
fn tile_from_str(kind: &str) -> Option<TileKind> {
    match kind {
        "floor" => Some(TileKind::Floor),
        "wall" => Some(TileKind::Wall),
        "entrance" => Some(TileKind::Entrance),
        "exit" => Some(TileKind::Exit),
        "button" => Some(TileKind::Button),
        "door" => Some(TileKind::Door),
        _ => None,
    }
}

/// Install the canvas click handler: in edit mode it paints the clicked cell.
fn install_canvas_click() {
    let Some((canvas, _)) = canvas_context() else {
        return;
    };
    let target = canvas.clone();
    let cb = Closure::<dyn FnMut(MouseEvent)>::new(move |e: MouseEvent| {
        let rect = target.get_bounding_client_rect();
        let (mode, w, h) = APP.with(|app| {
            let app = app.borrow();
            let m = app.render_model();
            (app.mode(), m.width, m.height)
        });
        if mode != Mode::Edit || w == 0 || h == 0 {
            return;
        }
        // Normalised click → grid cell (independent of any CSS scaling).
        let u = (e.client_x() as f64 - rect.left()) / rect.width();
        let v = (e.client_y() as f64 - rect.top()) / rect.height();
        let gx = (u * w as f64).floor();
        let gy = (v * h as f64).floor();
        if (0.0..w as f64).contains(&gx) && (0.0..h as f64).contains(&gy) {
            APP.with(|app| app.borrow_mut().editor_mut().paint(gx as u32, gy as u32));
            refresh_editor_ui();
        }
    });
    canvas
        .add_event_listener_with_callback("click", cb.as_ref().unchecked_ref())
        .expect("canvas click listener installs");
    cb.forget();
}

/// Install the keyboard handler: in playtest mode it routes arrows/WASD/q/r into
/// core commands.
fn install_keyboard() {
    let cb = Closure::<dyn FnMut(KeyboardEvent)>::new(move |e: KeyboardEvent| {
        let key = e.key();
        let handled = APP.with(|app| {
            let mut app = app.borrow_mut();
            if app.mode() != Mode::Playtest {
                return false;
            }
            app.playtest_mut().and_then(|s| s.apply_key(&key)).is_some()
        });
        if handled {
            e.prevent_default();
        }
    });
    window()
        .add_event_listener_with_callback("keydown", cb.as_ref().unchecked_ref())
        .expect("keydown listener installs");
    cb.forget();
}

// ===========================================================================
// The fixed-step run loop.
// ===========================================================================

/// Start the `requestAnimationFrame` loop: advance ghost replay at the fixed
/// step (playtest only) and draw every frame.
fn start_run_loop() {
    let frame = Rc::new(RefCell::new(None::<Closure<dyn FnMut(f64)>>));
    let frame_outer = frame.clone();
    let mut last_ts = 0.0_f64;
    let mut accumulator = 0.0_f64;

    *frame_outer.borrow_mut() = Some(Closure::<dyn FnMut(f64)>::new(move |ts: f64| {
        let dt = if last_ts == 0.0 { 0.0 } else { ts - last_ts };
        last_ts = ts;

        APP.with(|app| {
            let mut app = app.borrow_mut();
            if app.mode() == Mode::Playtest {
                accumulator += dt;
                let mut dispatched = 0;
                while accumulator >= STEP_MS && dispatched < MAX_TICKS_PER_FRAME {
                    if let Some(session) = app.playtest_mut() {
                        session.tick();
                    }
                    accumulator -= STEP_MS;
                    dispatched += 1;
                }
            } else {
                accumulator = 0.0;
            }
            draw(&app);
        });

        request_frame(frame.borrow().as_ref());
    }));
    request_frame(frame_outer.borrow().as_ref());
}

/// Schedule the next animation frame.
fn request_frame(cb: Option<&Closure<dyn FnMut(f64)>>) {
    if let Some(cb) = cb {
        let _ = window().request_animation_frame(cb.as_ref().unchecked_ref());
    }
}

// ===========================================================================
// Rendering — top-down depth cues on a 2D canvas.
// ===========================================================================

/// Draw the current frame (edit grid or live game) onto the board canvas.
fn draw(app: &RoomedPuzzleApp) {
    let Some((canvas, ctx)) = canvas_context() else {
        return;
    };
    let model = app.render_model();
    let want_w = (model.width as f64 * CELL_PX) as u32;
    let want_h = (model.height as f64 * CELL_PX) as u32;
    if canvas.width() != want_w {
        canvas.set_width(want_w);
    }
    if canvas.height() != want_h {
        canvas.set_height(want_h);
    }

    // Background.
    ctx.set_global_alpha(1.0);
    ctx.set_fill_style_str("#15171c");
    ctx.fill_rect(0.0, 0.0, want_w as f64, want_h as f64);

    model.cells.iter().for_each(|cell| {
        let x = cell.coord.x as f64 * CELL_PX;
        let y = cell.coord.y as f64 * CELL_PX;
        draw_tile(&ctx, x, y, cell.tile);
    });
    model
        .actors
        .iter()
        .for_each(|actor| draw_actor(&ctx, actor));
}

/// Colour palette for a tile: `(fill, light_edge, dark_edge)`.
fn tile_colors(tile: RenderTile) -> (&'static str, &'static str, &'static str) {
    match tile {
        RenderTile::Floor => ("#262b36", "#2f3542", "#1c2029"),
        RenderTile::Wall => ("#5b6373", "#737d8f", "#363c47"),
        RenderTile::Entrance => ("#2e6b3f", "#3f8a54", "#1f4a2c"),
        RenderTile::Exit => ("#b8902f", "#e0b552", "#7c5f1d"),
        RenderTile::Button { .. } => ("#a23b3b", "#c45656", "#6e2727"),
        RenderTile::Door { open: false } => ("#7a5a36", "#9a7548", "#503a22"),
        RenderTile::Door { open: true } => ("#191d25", "#232935", "#0f1217"),
    }
}

/// Draw one cell with its depth cue: flat fill for floor, beveled raised for
/// walls/closed doors/released buttons, beveled recessed for open doors/pressed
/// buttons. A 1px inset keeps adjacent blocks visually separated.
fn draw_tile(ctx: &CanvasRenderingContext2d, x: f64, y: f64, tile: RenderTile) {
    let (fill, light, dark) = tile_colors(tile);
    let inset = 1.0;
    let px = x + inset;
    let py = y + inset;
    let s = CELL_PX - inset * 2.0;

    ctx.set_global_alpha(1.0);
    ctx.set_fill_style_str(fill);
    ctx.fill_rect(px, py, s, s);

    let bevel = match tile.elevation() {
        Elevation::Flat => 0.0,
        Elevation::Raised | Elevation::Recessed => s * 0.18,
        Elevation::SlightlyRaised | Elevation::SlightlyRecessed => s * 0.10,
    };
    if bevel <= 0.0 {
        return;
    }
    let raised = matches!(
        tile.elevation(),
        Elevation::Raised | Elevation::SlightlyRaised
    );
    let (top_left, bottom_right) = if raised { (light, dark) } else { (dark, light) };

    // Top + left edges.
    ctx.set_fill_style_str(top_left);
    ctx.fill_rect(px, py, s, bevel);
    ctx.fill_rect(px, py, bevel, s);
    // Bottom + right edges.
    ctx.set_fill_style_str(bottom_right);
    ctx.fill_rect(px, py + s - bevel, s, bevel);
    ctx.fill_rect(px + s - bevel, py, bevel, s);
}

/// Draw one actor as a solid (player) or translucent, outlined (ghost) block.
fn draw_actor(ctx: &CanvasRenderingContext2d, actor: &RenderActor) {
    let margin = CELL_PX * 0.16;
    let x = actor.coord.x as f64 * CELL_PX + margin;
    let y = actor.coord.y as f64 * CELL_PX + margin;
    let s = CELL_PX - margin * 2.0;

    let (fill, outline) = match actor.kind {
        ActorKind::Player => ("#3f7fe0", "#bcd4ff"),
        ActorKind::Ghost => ("#54c6d6", "#eafcff"),
    };

    // Translucent fill (opaque for the player), then a near-opaque outline so a
    // ghost still reads as a distinct solid block, not an invisible outline.
    ctx.set_global_alpha(actor.alpha as f64);
    ctx.set_fill_style_str(fill);
    ctx.fill_rect(x, y, s, s);

    ctx.set_global_alpha((actor.alpha as f64 * 1.6).min(1.0));
    ctx.set_stroke_style_str(outline);
    ctx.set_line_width(2.0);
    ctx.stroke_rect(x, y, s, s);
    ctx.set_global_alpha(1.0);
}

// ===========================================================================
// Editor UI sync.
// ===========================================================================

/// The validation summary text for the current editor state.
fn validation_summary(app: &RoomedPuzzleApp) -> String {
    let report = app.editor().validate();
    if report.is_valid() {
        "Level is valid — you can playtest.".to_string()
    } else {
        report.messages().join("\n")
    }
}

/// Refresh the page's validation panel and the "Playtest" button after an edit.
fn refresh_editor_ui() {
    let (summary, playable) = APP.with(|app| {
        let app = app.borrow();
        (validation_summary(&app), app.editor().can_playtest())
    });
    let doc = document();
    if let Some(panel) = doc.get_element_by_id(VALIDATION_ID) {
        panel.set_text_content(Some(&summary));
    }
    if let Some(button) = doc.get_element_by_id(PLAYTEST_BTN_ID) {
        if playable {
            let _ = button.remove_attribute("disabled");
        } else {
            let _ = button.set_attribute("disabled", "");
        }
    }
}
