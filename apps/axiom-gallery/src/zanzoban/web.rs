//! The in-browser (WASM) editor + playtest surface — **`wasm32` only**.
//!
//! A thin 2D-`<canvas>` adapter over the pure, deterministic core
//! ([`crate::zanzoban::app::ZanzobanApp`]). It renders the neutral
//! [`crate::zanzoban::render_model::RenderModel`] with top-down depth cues, routes DOM
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
use std::collections::BTreeMap;

use axiom_host::{HostSafeAreaInsets, HostViewport, Pixels};
use axiom_input::{DeviceFrame, InputState, Tick};
use axiom_kernel::Ratio;
use axiom_layout::{
    solve, Direction, LayoutRect, LayoutStyle, LayoutTree, LayoutTreeBuilder, NodeId,
};
use axiom_math::{Mat4, Vec2};
use axiom_windowing::WindowingApi;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{HtmlCanvasElement, HtmlElement, KeyboardEvent, MouseEvent, PointerEvent};

use crate::zanzoban::app::{Mode, ZanzobanApp};
use crate::zanzoban::game_command::PuzzleCommand;
use crate::zanzoban::group_id::GroupId;
use crate::zanzoban::input_mapping::command_for_swipe;
use crate::zanzoban::scene3d;
use crate::zanzoban::scene3d::{SURFACE_H, SURFACE_W};
use crate::zanzoban::tile_kind::TileKind;

/// Instance-buffer cap for the renderer — generous enough for any real board.
const MAX_INSTANCES: u32 = 4608;

/// The board canvas element id (must match `web/index.html`).
const CANVAS_ID: &str = "axiom-puzzle-canvas";
/// The live-validation panel element id.
const VALIDATION_ID: &str = "validation";
/// The "Playtest" button element id (enabled only when the level validates).
const PLAYTEST_BTN_ID: &str = "btn-playtest";

thread_local! {
    /// The single app instance, shared across the DOM callbacks (single-threaded
    /// wasm, so a plain `RefCell` is enough).
    static APP: RefCell<ZanzobanApp> = RefCell::new(ZanzobanApp::new());

    /// The engine input state (swipe scheme), shared across the pointer
    /// callbacks. Holds the in-progress gesture's state between events.
    static INPUT: RefCell<InputState> = RefCell::new(InputState::new());

    /// The set of currently-down pointers (by browser pointer id) in physical
    /// canvas pixels, the neutral samples folded into the engine `DeviceFrame`.
    static DOWN_POINTERS: RefCell<BTreeMap<i32, (f32, f32)>> = RefCell::new(BTreeMap::new());

    /// The last `(css_w, css_h, cols, rows)` the layout was solved for, so the
    /// engine layout is recomputed + reapplied only when the window or grid changes.
    static LAST_LAYOUT: RefCell<Option<(u32, u32, u32, u32)>> = RefCell::new(None);

    /// Whether the editor is in eraser mode (canvas clicks erase instead of paint).
    static ERASING: RefCell<bool> = const { RefCell::new(false) };

    /// Cached camera view-projection keyed on `(grid_w, grid_h, perspective)`; the
    /// engine `App` that produces it is rebuilt only when the board size or the
    /// camera mode (edit ↔ playtest) changes, not every frame.
    static VIEW_PROJ: RefCell<Option<(u32, u32, bool, Mat4)>> = const { RefCell::new(None) };
}

/// The browser localStorage key prefix for save slots.
const SLOT_PREFIX: &str = "zanzoban.slot.";

/// The fixed panel width (landscape) / height (portrait) the layout reserves for
/// the editor/playtest side panel, in logical pixels.
const PANEL_BASIS: f32 = 340.0;
/// Layout node ids for the board and the side panel.
const NODE_ROOT: u32 = 0;
const NODE_BOARD: u32 = 1;
const NODE_PANEL: u32 = 2;

/// Log a line to the browser console, prefixed so it is easy to spot.
fn log(msg: &str) {
    web_sys::console::log_1(&JsValue::from_str(&format!("[zanzoban] {msg}")));
}

fn window() -> web_sys::Window {
    web_sys::window().expect("a browser window")
}

fn document() -> web_sys::Document {
    window().document().expect("a document")
}

/// The board canvas element (the engine binds it as the render surface; we only
/// need the element for pointer geometry, not a 2D context).
fn canvas_element() -> Option<HtmlCanvasElement> {
    document().get_element_by_id(CANVAS_ID)?.dyn_into().ok()
}

/// Boot the editor/playtest surface: install the canvas painter, the keyboard
/// handler, and the fixed-step run loop, then draw the first frame. Called from
/// the page once the wasm module is ready.
#[wasm_bindgen]
pub fn zanzoban_start() {
    console_error_panic_hook::set_once();
    install_canvas_click();
    install_keyboard();
    install_swipe();
    refresh_editor_ui();
    // Render the board through the Axiom engine's instanced-cube renderer. The
    // windowing loop owns backend selection (?backend=webgpu|webgl2|canvas2d,
    // auto-cascading otherwise), the canvas binding, and the rAF loop; we supply
    // the cube mesh once and one frame of instances (built from the render model)
    // per animation frame.
    let (vertices, indices) = cube_geometry();
    let mut windowing = WindowingApi::new();
    windowing
        .configure_surface(SURFACE_W, SURFACE_H)
        .expect("a positive surface is valid");
    log("ready");
    // Single cube mesh (id 0) with one 1×1 opaque-white material (id 0), so the
    // sampled albedo is (1,1,1,1) and each draw's colour reduces to its per-
    // instance colour. The lit-mesh batch format wants 36 floats/instance.
    let meshes = vec![(0_u64, vertices, indices)];
    let materials = vec![(0_u64, 1_u32, 1_u32, vec![255_u8, 255, 255, 255])];
    let _ = windowing.run_web_multi(CANVAS_ID, meshes, materials, MAX_INSTANCES, frame);
}

/// Source the engine's canonical cube geometry (position/normal/uv interleaved in
/// the backend's vertex format) by building a one-cube `App` and reading its mesh
/// vertex stream — the same stream retro FPS feeds the web loop.
fn cube_geometry() -> (Vec<f32>, Vec<u32>) {
    use axiom::prelude as ax;
    let unit = ax::Ratio::new(1.0).expect("unit channel is finite");
    let app = ax::App::new()
        .window(ax::Window::new(SURFACE_W, SURFACE_H))
        .add_plugins(ax::DefaultPlugins)
        .setup(move |world, meshes, materials| {
            let cube = meshes.add(ax::Mesh::cube());
            let material =
                materials.add(ax::Material::lit(ax::Color::linear_rgb(unit, unit, unit)));
            world.spawn((
                ax::Transform::IDENTITY,
                ax::Renderable {
                    mesh: cube,
                    material,
                },
            ));
        })
        .build();
    app.mesh_vertex_stream()
}

/// The lit directional light (fixed look), one per frame.
type Light = (u32, [f32; 3], [f32; 3], f32);
/// A per-`(mesh, material)` instance batch: `(mesh_id, material_id, floats, count)`.
type Batch = (u64, u64, Vec<f32>, u32);
/// The full frame tuple `run_web_multi` consumes.
type FrameOut = (
    [f32; 4],
    Vec<Light>,
    [f32; 16],
    Vec<Batch>,
    [f32; 16],
    Vec<bool>,
    Option<axiom_host::SdfScene>,
);

/// An identity 4×4 (column-major): an unused shadow / camera matrix.
const IDENTITY_4X4: [f32; 16] = [
    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
];

/// One animation frame: advance the playtest sim (if playing), then build the
/// instance batch from the current render model. Playtest uses an angled
/// perspective diorama; edit uses a steep near-top-down view.
fn frame(_tick: u64) -> FrameOut {
    APP.with(|app| {
        let mut app = app.borrow_mut();
        let perspective = app.mode() == Mode::Playtest;
        if perspective {
            if let Some(session) = app.playtest_mut() {
                session.tick();
            }
        }
        let model = app.render_model();
        sync_layout(model.width, model.height);
        let vp = cached_view_proj(model.width, model.height, perspective);
        let (clear, instances, count) = scene3d::build_instances(&model, vp);
        let lights = vec![(0_u32, [0.4_f32, 0.7, 0.6], [1.0_f32, 1.0, 1.0], 1.0_f32)];
        (
            clear,
            lights,
            IDENTITY_4X4,
            vec![(0_u64, 0_u64, instances, count)],
            vp.as_cols_array(),
            Vec::new(),
            None,
        )
    })
}

/// The camera view-projection for the board, cached per `(grid_w, grid_h, mode)`.
fn cached_view_proj(w: u32, h: u32, perspective: bool) -> Mat4 {
    VIEW_PROJ.with(|cache| {
        let mut cache = cache.borrow_mut();
        let hit = cache
            .as_ref()
            .and_then(|(cw, ch, cp, vp)| (*cw == w && *ch == h && *cp == perspective).then_some(*vp));
        hit.unwrap_or_else(|| {
            let vp = scene3d::view_projection(w, h, perspective);
            *cache = Some((w, h, perspective, vp));
            vp
        })
    })
}

/// Leave a ghost and reset the current life (the `q` action), wired to the
/// on-screen Playtest button so touch users have the action keyboard `q` gives.
#[wasm_bindgen]
pub fn playtest_freeze() {
    APP.with(|app| {
        if let Some(session) = app.borrow_mut().playtest_mut() {
            session.apply(PuzzleCommand::ResetLifeFromRecording);
        }
    });
}

/// Restart the level fresh (the `r` action), wired to the on-screen Playtest
/// button so touch users have the action keyboard `r` gives.
#[wasm_bindgen]
pub fn playtest_restart() {
    APP.with(|app| {
        if let Some(session) = app.borrow_mut().playtest_mut() {
            session.apply(PuzzleCommand::RestartLevelFresh);
        }
    });
}

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

/// The current level title (for hydrating the title field after import/load).
#[wasm_bindgen]
pub fn level_title() -> String {
    APP.with(|app| app.borrow().editor().title().to_string())
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

// ---- Mechanics config (add-ons) -------------------------------------------------

/// Enable/disable afterimage decay (`lifetime` used only when `enabled`).
#[wasm_bindgen]
pub fn set_decay(enabled: bool, lifetime: u32) {
    APP.with(|app| {
        app.borrow_mut()
            .editor_mut()
            .set_decay(enabled.then_some(lifetime.max(1)))
    });
    refresh_editor_ui();
}

/// Enable/disable the ghost budget (`par < 0` means no par).
#[wasm_bindgen]
pub fn set_budget(enabled: bool, max_ghosts: u32, par: i32) {
    let budget = enabled.then(|| (max_ghosts.max(1), (par >= 0).then_some(par as u32)));
    APP.with(|app| app.borrow_mut().editor_mut().set_budget(budget));
    refresh_editor_ui();
}

/// Enable/disable latching switches.
#[wasm_bindgen]
pub fn set_switches(on: bool) {
    APP.with(|app| app.borrow_mut().editor_mut().set_switches(on));
    refresh_editor_ui();
}

/// Enable/disable pushable crates.
#[wasm_bindgen]
pub fn set_crates(on: bool) {
    APP.with(|app| app.borrow_mut().editor_mut().set_crates(on));
    refresh_editor_ui();
}

/// Enable/disable lethal hazards.
#[wasm_bindgen]
pub fn set_hazards(on: bool) {
    APP.with(|app| app.borrow_mut().editor_mut().set_hazards(on));
    refresh_editor_ui();
}

/// Whether decay is enabled (for hydrating the panel after import/load).
#[wasm_bindgen]
pub fn decay_enabled() -> bool {
    APP.with(|app| app.borrow().editor().rules().decay.is_some())
}
/// The current decay lifetime (a sensible default when decay is off).
#[wasm_bindgen]
pub fn decay_lifetime() -> u32 {
    APP.with(|app| {
        app.borrow()
            .editor()
            .rules()
            .decay
            .map(|d| d.lifetime_steps)
            .unwrap_or(8)
    })
}
/// Whether the budget is enabled.
#[wasm_bindgen]
pub fn budget_enabled() -> bool {
    APP.with(|app| app.borrow().editor().rules().budget.is_some())
}
/// The current budget cap (default when off).
#[wasm_bindgen]
pub fn budget_max() -> u32 {
    APP.with(|app| {
        app.borrow()
            .editor()
            .rules()
            .budget
            .map(|b| b.max_ghosts)
            .unwrap_or(3)
    })
}
/// The current par (`-1` when unset).
#[wasm_bindgen]
pub fn budget_par() -> i32 {
    APP.with(|app| {
        app.borrow()
            .editor()
            .rules()
            .budget
            .and_then(|b| b.par)
            .map(|p| p as i32)
            .unwrap_or(-1)
    })
}
/// Whether switches are enabled.
#[wasm_bindgen]
pub fn switches_on() -> bool {
    APP.with(|app| app.borrow().editor().rules().switches)
}
/// Whether crates are enabled.
#[wasm_bindgen]
pub fn crates_on() -> bool {
    APP.with(|app| app.borrow().editor().rules().crates)
}
/// Whether hazards are enabled.
#[wasm_bindgen]
pub fn hazards_on() -> bool {
    APP.with(|app| app.borrow().editor().rules().hazards)
}

/// The palette kinds available right now, as comma-joined slugs — the page shows
/// only these buttons (add-on tiles appear when their add-on is enabled).
#[wasm_bindgen]
pub fn available_tiles() -> String {
    APP.with(|app| {
        app.borrow()
            .editor()
            .available_kinds()
            .iter()
            .map(|k| k.slug())
            .collect::<Vec<_>>()
            .join(",")
    })
}

// ---- Eraser, undo/redo, resize --------------------------------------------------

/// Toggle the eraser tool (canvas clicks erase instead of paint).
#[wasm_bindgen]
pub fn set_erasing(on: bool) {
    ERASING.with(|e| *e.borrow_mut() = on);
}

/// Undo the last edit.
#[wasm_bindgen]
pub fn undo() {
    APP.with(|app| {
        app.borrow_mut().editor_mut().undo();
    });
    refresh_editor_ui();
}
/// Redo the last undone edit.
#[wasm_bindgen]
pub fn redo() {
    APP.with(|app| {
        app.borrow_mut().editor_mut().redo();
    });
    refresh_editor_ui();
}
/// Is there anything to undo?
#[wasm_bindgen]
pub fn can_undo() -> bool {
    APP.with(|app| app.borrow().editor().can_undo())
}
/// Is there anything to redo?
#[wasm_bindgen]
pub fn can_redo() -> bool {
    APP.with(|app| app.borrow().editor().can_redo())
}

/// Resize the grid, preserving the overlapping region.
#[wasm_bindgen]
pub fn resize_grid(w: u32, h: u32) {
    APP.with(|app| app.borrow_mut().editor_mut().resize_preserving(w, h));
    refresh_editor_ui();
}
/// Current grid width (for hydrating the resize inputs).
#[wasm_bindgen]
pub fn grid_width() -> u32 {
    APP.with(|app| app.borrow().editor().width())
}
/// Current grid height.
#[wasm_bindgen]
pub fn grid_height() -> u32 {
    APP.with(|app| app.borrow().editor().height())
}

// ---- Library: templates + localStorage slots ------------------------------------

/// The built-in template names, one per line.
#[wasm_bindgen]
pub fn list_templates() -> String {
    crate::zanzoban::templates::TEMPLATES
        .iter()
        .map(|(name, _)| *name)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Load a built-in template into the editor. Returns `""` on success.
#[wasm_bindgen]
pub fn load_template(name: &str) -> String {
    match crate::zanzoban::templates::template(name) {
        Some(toml) => import_toml(toml),
        None => format!("no template named {name}"),
    }
}

/// The browser localStorage handle, if available.
fn storage() -> Option<web_sys::Storage> {
    window().local_storage().ok().flatten()
}

/// Save the current level into a named browser slot. Returns `""` on success.
#[wasm_bindgen]
pub fn save_slot(name: &str) -> String {
    let toml = APP.with(|app| app.borrow().editor().to_toml());
    match (toml, storage()) {
        (Ok(text), Some(store)) => {
            let _ = store.set_item(&format!("{SLOT_PREFIX}{name}"), &text);
            String::new()
        }
        (Err(e), _) => e.to_string(),
        (_, None) => "browser storage unavailable".to_string(),
    }
}

/// Load a named browser slot into the editor. Returns `""` on success.
#[wasm_bindgen]
pub fn load_slot(name: &str) -> String {
    match storage().and_then(|s| s.get_item(&format!("{SLOT_PREFIX}{name}")).ok().flatten()) {
        Some(toml) => import_toml(&toml),
        None => format!("no saved slot named {name}"),
    }
}

/// Every saved slot name, one per line.
#[wasm_bindgen]
pub fn list_slots() -> String {
    let Some(store) = storage() else {
        return String::new();
    };
    let n = store.length().unwrap_or(0);
    (0..n)
        .filter_map(|i| store.key(i).ok().flatten())
        .filter_map(|k| k.strip_prefix(SLOT_PREFIX).map(str::to_string))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Delete a named saved slot.
#[wasm_bindgen]
pub fn delete_slot(name: &str) {
    if let Some(store) = storage() {
        let _ = store.remove_item(&format!("{SLOT_PREFIX}{name}"));
    }
}

/// Map a palette slug to a [`TileKind`] (single source: [`TileKind::from_slug`]).
fn tile_from_str(kind: &str) -> Option<TileKind> {
    TileKind::from_slug(kind)
}

/// Install the canvas click handler: in edit mode it paints the clicked cell.
fn install_canvas_click() {
    let Some(canvas) = canvas_element() else {
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
            let erasing = ERASING.with(|e| *e.borrow());
            APP.with(|app| {
                let mut app = app.borrow_mut();
                let editor = app.editor_mut();
                if erasing {
                    editor.erase(gx as u32, gy as u32);
                } else {
                    editor.paint(gx as u32, gy as u32);
                }
            });
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

/// Install unified pointer (touch / mouse / pen) swipe handling on the canvas: in
/// playtest mode a swipe steps the player one cell in the swiped direction, so
/// the puzzle is playable without a keyboard. Edit-mode click-painting is
/// untouched (it runs in the other mode). PointerEvents are the one browser API
/// that reports touch, mouse, and pen as one shape — the same neutral
/// `(position, is_down)` samples the engine's `axiom-input` module consumes.
fn install_swipe() {
    let Some(canvas) = canvas_element() else {
        return;
    };
    // down / move: record this pointer's physical position, then evaluate.
    for name in ["pointerdown", "pointermove"] {
        let target = canvas.clone();
        let cb = Closure::<dyn FnMut(PointerEvent)>::new(move |e: PointerEvent| {
            e.prevent_default();
            let pos = pointer_position(&target, &e);
            DOWN_POINTERS.with(|p| {
                p.borrow_mut().insert(e.pointer_id(), pos);
            });
            process_swipe(&target);
        });
        canvas
            .add_event_listener_with_callback(name, cb.as_ref().unchecked_ref())
            .expect("pointer listener installs");
        cb.forget();
    }
    // up / cancel / leave: the pointer lifts — drop it, then evaluate. This is the
    // frame on which the swipe gesture completes.
    for name in ["pointerup", "pointercancel", "pointerleave"] {
        let target = canvas.clone();
        let cb = Closure::<dyn FnMut(PointerEvent)>::new(move |e: PointerEvent| {
            e.prevent_default();
            DOWN_POINTERS.with(|p| {
                p.borrow_mut().remove(&e.pointer_id());
            });
            process_swipe(&target);
        });
        canvas
            .add_event_listener_with_callback(name, cb.as_ref().unchecked_ref())
            .expect("pointer listener installs");
        cb.forget();
    }
}

/// Feed the current down-pointer set to the swipe synthesizer; on a completed
/// swipe (playtest mode only), step the player one cell. A no-op in edit mode, so
/// it never interferes with the click painter.
fn process_swipe(canvas: &HtmlCanvasElement) {
    if APP.with(|app| app.borrow().mode()) != Mode::Playtest {
        return;
    }
    let surface = Vec2::new(canvas.width() as f32, canvas.height() as f32);
    let samples: Vec<(Vec2, bool)> = DOWN_POINTERS.with(|p| {
        p.borrow()
            .values()
            .map(|(x, y)| (Vec2::new(*x, *y), true))
            .collect()
    });
    // Fold the pointer samples into the engine's per-tick snapshot and read the
    // completed swipe. The gesture is purely pointer-driven, so the tick is a
    // constant here (this app reads no keyboard actions through `InputState`).
    let command = INPUT
        .with(|s| {
            let mut input = s.borrow_mut();
            input.sample(Tick::ZERO, &DeviceFrame::new(surface, &[], &samples));
            input.swipe()
        })
        .and_then(command_for_swipe);
    if let Some(command) = command {
        APP.with(|app| {
            if let Some(session) = app.borrow_mut().playtest_mut() {
                session.apply(command);
            }
        });
    }
}

/// A pointer event's client coordinates → physical canvas pixels (the backing
/// store the swipe synthesizer measures in), independent of CSS scaling.
fn pointer_position(canvas: &HtmlCanvasElement, e: &PointerEvent) -> (f32, f32) {
    let rect = canvas.get_bounding_client_rect();
    let sx = canvas.width() as f64 / rect.width().max(1.0);
    let sy = canvas.height() as f64 / rect.height().max(1.0);
    let x = (e.client_x() as f64 - rect.left()) * sx;
    let y = (e.client_y() as f64 - rect.top()) * sy;
    (x as f32, y as f32)
}

/// The side-panel element id (must match `web/index.html`).
const SIDE_ID: &str = "side";

/// Recompute and apply the on-screen layout from the live viewport — but only when
/// the window size or grid changed. The engine's `axiom-layout` solver decides
/// where the board and the side panel go: side-by-side in landscape, the panel
/// stacked *below* the board in portrait, the whole thing inset by the device safe
/// area. We just apply the solved rectangles to the DOM. The board node carries the
/// grid's aspect ratio, so its fixed-resolution (`pixelated`) canvas scales crisply
/// to whatever cell it is given.
fn sync_layout(cols: u32, rows: u32) {
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
    let key = (css_w as u32, css_h as u32, cols, rows);
    if LAST_LAYOUT.with(|last| *last.borrow() == Some(key)) {
        return;
    }
    LAST_LAYOUT.with(|last| *last.borrow_mut() = Some(key));

    let dpr = win.device_pixel_ratio().max(0.5) as f32;
    let scale = Ratio::new(dpr).unwrap_or_else(|_| Ratio::new(1.0).expect("unit ratio is finite"));
    let viewport = HostViewport::new(css_w as u32, css_h as u32, scale)
        .expect("a positive viewport is valid")
        .with_safe_area_insets(read_safe_area_insets());

    let result = solve(&viewport, &build_layout_tree(cols, rows));
    position_element(CANVAS_ID, result.rect(NodeId::from_raw(NODE_BOARD)));
    position_element(SIDE_ID, result.rect(NodeId::from_raw(NODE_PANEL)));
}

/// The layout tree: an adaptive root (row in landscape, column in portrait) with a
/// grid-aspect board that grows to fill, and a fixed-size side panel.
fn build_layout_tree(cols: u32, rows: u32) -> LayoutTree {
    let mut builder = LayoutTreeBuilder::new();
    let mut root_style = LayoutStyle::new();
    root_style.direction = Direction::Adaptive;
    let root = builder.root(NodeId::from_raw(NODE_ROOT), root_style);

    let mut board = LayoutStyle::new();
    board.grow = Ratio::new(1.0).expect("unit grow is finite");
    board.aspect = Ratio::new(cols.max(1) as f32 / rows.max(1) as f32).ok();
    builder.child(root, NodeId::from_raw(NODE_BOARD), board);

    let mut panel = LayoutStyle::new();
    panel.basis = Pixels::new(PANEL_BASIS).expect("finite panel basis");
    builder.child(root, NodeId::from_raw(NODE_PANEL), panel);

    builder.build()
}

/// Apply a solved rect to a DOM element's absolute position + size (logical px).
fn position_element(id: &str, rect: Option<LayoutRect>) {
    let element = document()
        .get_element_by_id(id)
        .and_then(|e| e.dyn_into::<HtmlElement>().ok());
    rect.zip(element).into_iter().for_each(|(r, el)| {
        let style = el.style();
        let _ = style.set_property("left", &format!("{}px", r.left().get()));
        let _ = style.set_property("top", &format!("{}px", r.top().get()));
        let _ = style.set_property("width", &format!("{}px", r.width().get()));
        let _ = style.set_property("height", &format!("{}px", r.height().get()));
    });
}

/// Read the device safe-area insets the browser exposes via the CSS
/// `env(safe-area-inset-*)` values, in logical pixels, by measuring a hidden probe
/// element. Falls back to no insets on any failure.
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

/// The validation summary text for the current editor state.
fn validation_summary(app: &ZanzobanApp) -> String {
    let report = app.editor().validate();
    if report.is_valid() {
        "Level is valid — you can playtest.".to_string()
    } else {
        report.messages().join("\n")
    }
}

/// Refresh the page's validation panel and the "Playtest" button after an edit.
fn refresh_editor_ui() {
    let (summary, playable, can_undo, can_redo) = APP.with(|app| {
        let app = app.borrow();
        (
            validation_summary(&app),
            app.editor().can_playtest(),
            app.editor().can_undo(),
            app.editor().can_redo(),
        )
    });
    let doc = document();
    if let Some(panel) = doc.get_element_by_id(VALIDATION_ID) {
        panel.set_text_content(Some(&summary));
    }
    set_disabled(&doc, PLAYTEST_BTN_ID, !playable);
    set_disabled(&doc, "btn-undo", !can_undo);
    set_disabled(&doc, "btn-redo", !can_redo);
}

/// Set or clear the `disabled` attribute on an element by id.
fn set_disabled(doc: &web_sys::Document, id: &str, disabled: bool) {
    if let Some(el) = doc.get_element_by_id(id) {
        if disabled {
            let _ = el.set_attribute("disabled", "");
        } else {
            let _ = el.remove_attribute("disabled");
        }
    }
}
