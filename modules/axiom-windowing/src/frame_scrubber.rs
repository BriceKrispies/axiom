//! The shared **frame scrubber** overlay every browser game gets for free.
//!
//! The web run loop ([`crate::WindowingApi::run_web_multi`] /
//! [`crate::WindowingApi::run_web_streaming`]) is the single chokepoint every
//! live browser game funnels through (directly, via `run_web`, or via
//! `App::run`). Mounting the scrubber there means a uniform dev overlay appears
//! on *all* game screens without any per-app wiring.
//!
//! Each live frame it records the exact data handed to the backend's present
//! into an `axiom-recording` timeline as opaque bytes. There are two recording
//! arms, one per present shape: the **3D** arm ([`FrameScrubber::record`] /
//! [`FrameScrubber::scrub_frame`]) carries `(clear, lights, light view-projection,
//! draw batches)` and round-trips them through [`encode`]/[`decode`]; the **2D**
//! arm ([`FrameScrubber::record_2d`] / [`FrameScrubber::scrub_2d_frame`]) carries
//! `(clear, Draw2dList)`. A given presenter only ever drives one arm — a 2D game's
//! `present_2d` uses the 2D arm, a 3D run loop uses the 3D arm — so they share one
//! recorder/timeline without tagging frame kinds. When the user scrubs (Back / Fwd
//! / Live), the run loop stops calling the app's frame closure — freezing the live
//! sim — and re-presents the recorded frame instead. The timeline is never mutated
//! and no frame is forked; this is purely a read-only view over already presented
//! frames.
//!
//! ## Why the 2D arm keeps a clone store
//! The recorder accounts memory, computes hashes, and evicts over an *opaque byte*
//! payload, so the 2D arm still encodes each frame's `Draw2dList` to bytes
//! ([`encode_2d`]) — that is what makes the overlay's frames / range / mem / hash
//! read-outs grow exactly as they do for a 3D game. But this `windowing` module is
//! only allowed to depend on the kernel, the host layer, and the interface layer —
//! **not** on `axiom-math` — so it cannot *rebuild* a `Draw2dList` from those bytes
//! (reconstructing the host 2D contract would mean naming the `Vec2`/`Mat3`
//! primitives it is built from). Reading a list needs no math name (field access +
//! `as_cols_array`), but writing one back does. So the exact frame is replayed from
//! a parallel store of `Draw2dList` clones ([`FrameScrubber::frames_2d`]) kept
//! position-aligned with the recorder's timeline (trimmed to its retained length on
//! every record, so it never outgrows the budget). Replay is therefore byte-exact
//! by construction (a clone), and the byte payload drives only the bookkeeping.
//!
//! ## Built on the interface layer
//! The overlay's *model* is an [`axiom_interface`] panel: its header (mode), its
//! read-out rows (frames / range / mem / hash), and its action buttons
//! (rev / back / fwd / live / fork) are all set through [`InterfaceApi`], which
//! emits a neutral [`axiom_interface::InterfaceDrawList`]. This wasm arm is just
//! the platform binding: it paints that draw list into the DOM, routes pointer
//! drags into the panel's drag model (so the panel is draggable), and maps a
//! clicked button's action id back to a recorder operation. No UI structure is
//! hand-rolled here any more — the layer owns it.
//!
//! This is wasm32-only platform-edge code (it owns DOM nodes), like the rest of
//! the live run loop — it never enters the deterministic native core.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

use axiom_interface::{InterfaceApi, InterfaceDrawItem, PanelId};
use axiom_recording::RecordingApi;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{Element, EventTarget, KeyboardEvent, MouseEvent, PointerEvent};

/// The frame scrubber is a **developer-console** overlay, not a public-showcase
/// one. It mounts only when the host page has opted in by setting the global
/// `window.__axiom_dev_tools = true` — the workspace dev console does this before
/// booting a cartridge; the public gallery never does, so its apps get no
/// scrubber. Read as a truthy JS value (via `Reflect`, since it is an arbitrary
/// host-set global), defaulting to `false` when absent. wasm32-only platform edge.
fn dev_tools_enabled() -> bool {
    web_sys::window()
        .and_then(|window| {
            js_sys::Reflect::get(window.as_ref(), &JsValue::from_str("__axiom_dev_tools")).ok()
        })
        .map(|value| value.is_truthy())
        .unwrap_or(false)
}

/// One directional light as the run loop carries it: `(index, dir, colour, intensity)`.
type Light = (u32, [f32; 3], [f32; 3], f32);
/// One draw batch: `(mesh_id, material_id, instance_floats, instance_count)`.
type Batch = (u64, u64, Vec<f32>, u32);
/// The full per-frame argument set handed to the backend's `present`.
pub(crate) type PresentArgs = ([f32; 4], Vec<Light>, [f32; 16], Vec<Batch>);

/// Produces the app's serialized sim state for the current frame (recorded as the
/// frame's opaque `state_bytes`). Supplied by a forkable run-loop entry.
pub(crate) type SnapshotHook = Rc<dyn Fn() -> Vec<u8>>;
/// Restores the app's sim state from a recorded frame's `state_bytes` (the fork).
pub(crate) type RestoreHook = Rc<dyn Fn(&[u8])>;

/// Action ids carried by the interface buttons; the delegated click handler maps
/// each back to a recorder operation. The layer stays neutral about their meaning.
const REV: u32 = 0;
const BACK: u32 = 1;
const FWD: u32 = 2;
const LIVE: u32 = 3;
const FORK: u32 = 4;

/// The injected stylesheet for the scrubber panel. Base look lives here; the
/// per-frame position/width is applied inline from the panel's draw rect.
const SCRUBBER_CSS: &str = "\
.axiom-scrub{position:fixed;z-index:2147483600;box-sizing:border-box;\
min-width:280px;max-width:calc(100vw - 16px);font:600 12px ui-monospace,monospace;\
color:#e8ecf2;background:rgba(10,12,16,0.9);border:1px solid #2a2e36;\
border-radius:8px;padding:6px 8px;-webkit-user-select:none;user-select:none;}\
.axiom-scrub-header{cursor:move;padding:2px 2px 6px;border-bottom:1px solid #2a2e36;\
margin-bottom:6px;white-space:nowrap;}\
.axiom-scrub-row{display:flex;justify-content:space-between;gap:10px;}\
.axiom-scrub-k{color:#8a93a3;}\
.axiom-scrub-v{text-align:right;}\
.axiom-scrub-actions{display:flex;gap:6px;flex-wrap:wrap;margin-top:6px;}\
.axiom-scrub-btn{font:600 12px ui-monospace,monospace;color:#e8ecf2;background:#1b1f27;\
border:1px solid #3a3f49;border-radius:6px;padding:3px 9px;cursor:pointer;}\
.axiom-scrub--hidden{display:none;}";

/// The fixed DOM skeleton the binding repaints in place: the panel root, its
/// header (drag handle), and the row/action containers, plus the by-position
/// caches of the row spans and button elements it reuses across repaints (so a
/// click never lands on a node replaced mid-gesture).
#[derive(Clone)]
struct Nodes {
    root: Element,
    header: Element,
    rows_box: Element,
    actions_box: Element,
    rows: Rc<RefCell<Vec<(Element, Element)>>>,
    buttons: Rc<RefCell<Vec<Element>>>,
}

/// The cloneable state the per-frame repaint and every DOM event closure share:
/// the recorder, the reverse/active flags, the interface model + this scrubber's
/// panel, whether a fork button is offered, and the DOM nodes.
#[derive(Clone)]
struct Shared {
    recorder: Rc<RefCell<RecordingApi>>,
    reverse: Rc<Cell<bool>>,
    active: Rc<Cell<bool>>,
    // Whether the overlay is shown. The scrubber always mounts (so it is always
    // summonable), but starts hidden unless the host opts in via
    // `window.__axiom_dev_tools`. Toggled by the F2 hotkey and the debug overlay's
    // `scrubber` command; while hidden the run loop records nothing (zero cost).
    visible: Rc<Cell<bool>>,
    interface: Rc<RefCell<InterfaceApi>>,
    panel: PanelId,
    has_fork: bool,
    nodes: Nodes,
}

/// The shared frame scrubber: the recorder + flags + interface panel, plus the
/// app's snapshot hook (for forkable run loops). When `reverse` is set the run
/// loop walks the selection one frame older every tick (auto-rewind) until it
/// reaches the oldest frame.
pub(crate) struct FrameScrubber {
    shared: Shared,
    snapshot: Option<SnapshotHook>,
    /// The authoritative recorded 2D frames — each `(frame index, clear colour,
    /// the exact Draw2dList)` — kept position-aligned with the recorder's
    /// timeline. The recorder's opaque bytes drive its memory/hash/eviction
    /// bookkeeping; the exact frame replayed while scrubbing comes from here,
    /// because this module cannot rebuild a `Draw2dList` from bytes without
    /// naming the `axiom-math` primitives it is built from (see the module docs).
    /// Trimmed to the timeline's retained length on every record, so it never
    /// outgrows the recorder's budget. Empty for a 3D presenter (which uses the
    /// byte-decoding 3D arm instead).
    frames_2d: RefCell<VecDeque<(u64, [f32; 4], axiom_host::Draw2dList)>>,
}

impl FrameScrubber {
    /// Build the recorder (browser-safe budget) and mount the overlay as a
    /// draggable interface panel. `snapshot`/`restore` are the app's fork hooks:
    /// when both are present the overlay records sim state each frame and offers a
    /// `⏏ fork` button. Pass `None`/`None` for scrub-only games. Returns `None` if
    /// there is no DOM (then the run loop simply records nothing and presents live).
    pub(crate) fn mount(
        snapshot: Option<SnapshotHook>,
        restore: Option<RestoreHook>,
    ) -> Option<FrameScrubber> {
        // The overlay always mounts (so the F2 hotkey and the debug `scrubber`
        // command can always summon it), but starts HIDDEN unless the host opts in
        // via `window.__axiom_dev_tools` (the workspace dev console does; the public
        // gallery does not). While hidden the run loop records nothing, so a hidden
        // scrubber costs nothing per frame.
        let recorder = Rc::new(RefCell::new(RecordingApi::browser_safe().ok()?));
        let reverse = Rc::new(Cell::new(false));
        let active = Rc::new(Cell::new(true));
        let visible = Rc::new(Cell::new(dev_tools_enabled()));
        let window = web_sys::window()?;
        let document = window.document()?;
        let body = document.body()?;

        inject_style(&document, &body);

        // The interface model owns the panel: shown + pinned (a dev tool that
        // stays up), seeded near the bottom-left so it does not sit under the
        // top-left debug overlay; draggable from there.
        let mut interface = InterfaceApi::new();
        let panel = interface.add_panel();
        interface.show(panel);
        interface.pin(panel);
        interface.set_panel_width(panel, 360);
        let start_y = window
            .inner_height()
            .ok()
            .and_then(|v| v.as_f64())
            .map(|h| (h as i32 - 150).max(8))
            .unwrap_or(8);
        interface.set_panel_position(panel, 8, start_y);

        let nodes = build_dom(&document, &body)?;

        let shared = Shared {
            recorder,
            reverse,
            active,
            visible,
            interface: Rc::new(RefCell::new(interface)),
            panel,
            has_fork: restore.is_some(),
            nodes,
        };

        install_click(&shared, restore);
        install_drag(&window, &shared);
        install_focus_listeners(&window, &document, &shared);
        install_visibility_toggle(&window, &shared);
        // Reflect the initial hidden/visible state onto the DOM before the first
        // repaint.
        apply_visibility(&shared);

        let scrubber = FrameScrubber {
            shared,
            snapshot,
            frames_2d: RefCell::new(VecDeque::new()),
        };
        repaint(&scrubber.shared);
        Some(scrubber)
    }

    /// Whether the run loop should keep stepping the app (live) versus freezing
    /// it and re-presenting a recorded frame (scrubbing).
    pub(crate) fn is_live(&self) -> bool {
        self.shared.recorder.borrow().is_live()
    }

    /// Whether the game is focused/running. Cleared on focus loss (Escape /
    /// window blur / tab hidden) and set again on return; while false the run
    /// loop freezes the game entirely (no tick, no app step, no present).
    pub(crate) fn is_active(&self) -> bool {
        self.shared.active.get()
    }

    /// Record one live frame's present arguments (encoded as the opaque render
    /// artifact). Recording errors (e.g. an over-budget frame) are non-fatal.
    pub(crate) fn record(
        &self,
        frame: u64,
        clear: [f32; 4],
        lights: &[Light],
        light_vp: [f32; 16],
        batches: &[Batch],
    ) {
        // A hidden scrubber records nothing and has no visible panel to repaint —
        // zero per-frame cost until it is summoned (F2 / the `scrubber` command).
        self.shared.visible.get().then(|| {
            // Skip recording while paused (game unfocused): the frame still presents
            // live, but the timeline does not grow.
            self.shared.active.get().then(|| {
                let render_bytes = encode(clear, lights, light_vp, batches);
                // Capture the app's sim state for this frame (empty when not
                // forkable); it rides in the recorder's `state_bytes` slot for a
                // later fork.
                let state_bytes = self
                    .snapshot
                    .as_ref()
                    .map(|snap| snap())
                    .unwrap_or_default();
                let _ = self.shared.recorder.borrow_mut().record_frame(
                    frame,
                    frame,
                    Vec::new(),
                    Vec::new(),
                    state_bytes,
                    render_bytes,
                );
            });
            repaint(&self.shared);
        });
    }

    /// The frame to present this scrub tick: first advance auto-rewind (if
    /// armed), then return the selected recorded frame's present arguments.
    pub(crate) fn scrub_frame(&self) -> Option<PresentArgs> {
        tick_reverse(&self.shared);
        selected_present(&self.shared)
    }

    /// Record one live **2D** frame — the 2D peer of [`Self::record`]. The frame's
    /// `(clear, list)` is encoded to the recorder's opaque bytes (for the memory /
    /// hash / eviction bookkeeping the overlay reads out) and the exact list is
    /// stashed in the clone store for byte-exact replay while scrubbing. Skips
    /// recording while paused (game unfocused): the frame still presents live, but
    /// the timeline does not grow. Recording errors (e.g. an over-budget frame) are
    /// non-fatal and leave the clone store untouched (so it stays aligned).
    pub(crate) fn record_2d(&self, frame: u64, clear: [f32; 4], list: &axiom_host::Draw2dList) {
        // A hidden scrubber records nothing and has no visible panel to repaint.
        self.shared.visible.get().then(|| {
            self.shared.active.get().then(|| {
                let render_bytes = encode_2d(clear, list);
                // Capture the app's sim state for this frame (empty when not
                // forkable); it rides in the recorder's `state_bytes` slot for a
                // later fork.
                let state_bytes = self
                    .snapshot
                    .as_ref()
                    .map(|snap| snap())
                    .unwrap_or_default();
                let recorded = self
                    .shared
                    .recorder
                    .borrow_mut()
                    .record_frame(
                        frame,
                        frame,
                        Vec::new(),
                        Vec::new(),
                        state_bytes,
                        render_bytes,
                    )
                    .is_ok();
                // Mirror the push only when the recorder actually retained the frame,
                // then trim the store's front to the timeline's retained length so the
                // two stay position-aligned through the recorder's FIFO eviction.
                recorded.then(|| {
                    let retained = self.shared.recorder.borrow().frame_count();
                    let mut store = self.frames_2d.borrow_mut();
                    store.push_back((frame, clear, list.clone()));
                    while store.len() > retained {
                        store.pop_front();
                    }
                });
            });
            repaint(&self.shared);
        });
    }

    /// The **2D** frame to present this scrub tick — the 2D peer of
    /// [`Self::scrub_frame`]. First advance auto-rewind (if armed), then return the
    /// selected recorded frame's `(clear, Draw2dList)` from the clone store (looked
    /// up by index with the same first-match semantics the recorder's `get_frame`
    /// uses). `None` if the frame was evicted or nothing is recorded.
    pub(crate) fn scrub_2d_frame(&self) -> Option<([f32; 4], axiom_host::Draw2dList)> {
        tick_reverse(&self.shared);
        let selected = self.shared.recorder.borrow().selected_frame()?;
        self.frames_2d
            .borrow()
            .iter()
            .find(|(frame, ..)| *frame == selected.raw())
            .map(|(_, clear, list)| (*clear, list.clone()))
    }
}

/// If reverse playback is armed, step the selection one frame older. Reaching the
/// oldest retained frame stops playback there (no wrap-around).
fn tick_reverse(s: &Shared) {
    let armed = s.reverse.get();
    // `then` keeps this a no-op when not armed; `step_back` Errs at the oldest
    // edge, which disarms playback so it rests on the oldest frame.
    let stepped = armed
        .then(|| s.recorder.borrow_mut().step_back())
        .unwrap_or(Ok(()));
    s.reverse.set(armed & stepped.is_ok());
    armed.then(|| repaint(s));
}

/// The present arguments of the selected recorded frame to re-present; `None` if
/// the frame was evicted or the payload is unreadable.
fn selected_present(s: &Shared) -> Option<PresentArgs> {
    let recorder = s.recorder.borrow();
    let selected = recorder.selected_frame()?;
    let capture = recorder.frame(selected).ok()?;
    decode(capture.render_bytes())
}

/// Push this tick's recorder stats into the interface panel (header / rows /
/// actions), then paint the panel's neutral draw list into the DOM.
fn repaint(s: &Shared) {
    {
        let recorder = s.recorder.borrow();
        let mode = mode_string(&recorder, s.reverse.get(), s.active.get());
        let rows = stat_rows(&recorder);
        let mut interface = s.interface.borrow_mut();
        interface.set_panel_header(s.panel, "REWIND", &mode);
        interface.set_panel_rows(s.panel, &rows);
        let mut actions: Vec<(u32, &str)> = vec![
            (REV, "◀◀ rev"),
            (BACK, "◀ back"),
            (FWD, "fwd ▶"),
            (LIVE, "▶ live"),
        ];
        s.has_fork.then(|| actions.push((FORK, "⏏ fork")));
        interface.set_panel_actions(s.panel, &actions);
    }
    let list = s.interface.borrow().draw_list(s.panel);
    render(&s.nodes, list.items());
}

/// The header's mode text: `rec LIVE`/`rec PAUSED` when following live frames,
/// or `SCRUB @ N` / `◀◀ REV @ N` while scrubbing.
fn mode_string(recorder: &RecordingApi, reverse: bool, active: bool) -> String {
    let live = recorder.is_live();
    let label = if reverse { "◀◀ REV" } else { "SCRUB" };
    let live_label = if active { "rec LIVE" } else { "rec PAUSED" };
    if live {
        live_label.to_string()
    } else {
        recorder
            .selected_frame()
            .map(|f| format!("{label} @ {}", f.raw()))
            .unwrap_or_else(|| label.to_string())
    }
}

/// The read-out rows: frame count, retained range, memory used vs. budget, and
/// the focused frame's hash.
fn stat_rows(recorder: &RecordingApi) -> Vec<(String, String)> {
    let live = recorder.is_live();
    let oldest = recorder
        .oldest_frame_index()
        .map(|f| f.raw().to_string())
        .unwrap_or_else(|_| "-".to_string());
    let latest = recorder
        .latest_frame_index()
        .map(|f| f.raw().to_string())
        .unwrap_or_else(|_| "-".to_string());
    let focus = if live {
        recorder.latest_frame_index().ok()
    } else {
        recorder.selected_frame()
    };
    let hash = focus
        .and_then(|f| recorder.frame(f).ok().map(|c| c.final_hash()))
        .map(|h| format!("{h:016x}"))
        .unwrap_or_else(|| "-".repeat(16));
    vec![
        ("frames".to_string(), recorder.frame_count().to_string()),
        ("range".to_string(), format!("{oldest}–{latest}")),
        (
            "mem".to_string(),
            format!(
                "{} / {} KiB",
                recorder.current_bytes() / 1024,
                recorder.max_bytes() / 1024
            ),
        ),
        ("hash".to_string(), hash),
    ]
}

/// Build the fixed DOM skeleton (root + header + row/action containers) and
/// append it to the page.
fn build_dom(document: &web_sys::Document, body: &Element) -> Option<Nodes> {
    let root = document.create_element("div").ok()?;
    root.set_class_name("axiom-scrub");
    let header = document.create_element("div").ok()?;
    header.set_class_name("axiom-scrub-header");
    let rows_box = document.create_element("div").ok()?;
    rows_box.set_class_name("axiom-scrub-rows");
    let actions_box = document.create_element("div").ok()?;
    actions_box.set_class_name("axiom-scrub-actions");
    root.append_child(&header).ok()?;
    root.append_child(&rows_box).ok()?;
    root.append_child(&actions_box).ok()?;
    body.append_child(&root).ok()?;
    Some(Nodes {
        root,
        header,
        rows_box,
        actions_box,
        rows: Rc::new(RefCell::new(Vec::new())),
        buttons: Rc::new(RefCell::new(Vec::new())),
    })
}

/// Paint the panel's neutral draw list into the DOM: position from the `Panel`
/// rect, header text, the read-out rows, and the action buttons. `ConsoleInput`
/// (always emitted by the layer) is ignored — the scrubber has no console.
fn render(nodes: &Nodes, items: &[InterfaceDrawItem]) {
    let mut row_specs: Vec<(String, String)> = Vec::new();
    let mut button_specs: Vec<(u32, String)> = Vec::new();
    items.iter().for_each(|item| match item {
        InterfaceDrawItem::Panel { x, y, width, .. } => {
            let _ = nodes
                .root
                .set_attribute("style", &format!("left:{x}px;top:{y}px;width:{width}px;"));
        }
        InterfaceDrawItem::Header { primary, secondary } => {
            nodes
                .header
                .set_text_content(Some(&format!("{primary} · {secondary}")));
        }
        InterfaceDrawItem::Row { label, value } => row_specs.push((label.clone(), value.clone())),
        InterfaceDrawItem::Button { action, label } => button_specs.push((*action, label.clone())),
        InterfaceDrawItem::ConsoleLine { .. } | InterfaceDrawItem::ConsoleInput { .. } => {}
    });
    web_sys::window()
        .and_then(|w| w.document())
        .into_iter()
        .for_each(|document| {
            sync_rows(&document, nodes, &row_specs);
            sync_buttons(&document, nodes, &button_specs);
        });
}

/// Reconcile the read-out rows by position: reuse existing label/value spans,
/// creating any that the row set grew into. The scrubber's row set is fixed, so
/// after the first paint this only updates text.
fn sync_rows(document: &web_sys::Document, nodes: &Nodes, specs: &[(String, String)]) {
    let mut cache = nodes.rows.borrow_mut();
    specs
        .iter()
        .enumerate()
        .for_each(|(index, (label, value))| {
            (index >= cache.len())
                .then(|| make_row(document))
                .flatten()
                .into_iter()
                .for_each(|(row, key, val)| {
                    let _ = nodes.rows_box.append_child(&row);
                    cache.push((key, val));
                });
            cache.get(index).into_iter().for_each(|(key, val)| {
                key.set_text_content(Some(label));
                val.set_text_content(Some(value));
            });
        });
}

/// Create one read-out row (`<div><span.k/><span.v/></div>`), returning the row
/// plus its key/value spans, or `None` if element creation failed.
fn make_row(document: &web_sys::Document) -> Option<(Element, Element, Element)> {
    let row = document.create_element("div").ok()?;
    row.set_class_name("axiom-scrub-row");
    let key = document.create_element("span").ok()?;
    key.set_class_name("axiom-scrub-k");
    let val = document.create_element("span").ok()?;
    val.set_class_name("axiom-scrub-v");
    row.append_child(&key).ok()?;
    row.append_child(&val).ok()?;
    Some((row, key, val))
}

/// Reconcile the action buttons by position: reuse existing buttons, creating any
/// the action set grew into. Each carries its `data-action` id so the delegated
/// click handler can route it. Buttons are stable after the first paint, so a
/// click is never lost to a replaced node.
fn sync_buttons(document: &web_sys::Document, nodes: &Nodes, specs: &[(u32, String)]) {
    let mut cache = nodes.buttons.borrow_mut();
    specs
        .iter()
        .enumerate()
        .for_each(|(index, (action, label))| {
            (index >= cache.len())
                .then(|| make_button(document))
                .flatten()
                .into_iter()
                .for_each(|button| {
                    let _ = nodes.actions_box.append_child(&button);
                    cache.push(button);
                });
            cache.get(index).into_iter().for_each(|button| {
                let _ = button.set_attribute("data-action", &action.to_string());
                button.set_text_content(Some(label));
            });
        });
}

/// Create one `<button.axiom-scrub-btn>`, or `None` if creation failed.
fn make_button(document: &web_sys::Document) -> Option<Element> {
    let button = document.create_element("button").ok()?;
    button.set_class_name("axiom-scrub-btn");
    Some(button)
}

/// Inject the scrubber stylesheet once (appended to the body).
fn inject_style(document: &web_sys::Document, body: &Element) {
    document
        .create_element("style")
        .ok()
        .into_iter()
        .for_each(|style| {
            style.set_text_content(Some(SCRUBBER_CSS));
            let _ = body.append_child(&style);
        });
}

/// Install one delegated click listener on the actions container: it reads the
/// clicked button's `data-action` id and dispatches it to the recorder. Fork uses
/// the app's `restore` hook (held in the closure).
fn install_click(s: &Shared, restore: Option<RestoreHook>) {
    let shared = s.clone();
    let cb = Closure::<dyn FnMut(MouseEvent)>::new(move |e: MouseEvent| {
        let action = e
            .target()
            .and_then(|t| t.dyn_into::<Element>().ok())
            .and_then(|el| el.closest("button").ok().flatten())
            .and_then(|button| button.get_attribute("data-action"))
            .and_then(|raw| raw.parse::<u32>().ok());
        action
            .into_iter()
            .for_each(|action| dispatch(&shared, &restore, action));
    });
    let _ = s
        .nodes
        .actions_box
        .add_event_listener_with_callback("click", cb.as_ref().unchecked_ref());
    cb.forget();
}

/// Map a clicked action id to its recorder operation, then repaint.
fn dispatch(s: &Shared, restore: &Option<RestoreHook>, action: u32) {
    match action {
        // Reverse playback: enter scrub at the newest frame (if live) and arm the
        // auto-rewind flag; the run loop walks one frame older each tick.
        REV => {
            let latest = s.recorder.borrow().latest_frame_index().ok();
            latest.into_iter().for_each(|latest| {
                let _ = s.recorder.borrow_mut().enter_scrub(latest.raw());
            });
            s.reverse.set(true);
        }
        // Manual controls cancel reverse playback.
        BACK => {
            s.reverse.set(false);
            let _ = s.recorder.borrow_mut().step_back();
        }
        FWD => {
            s.reverse.set(false);
            let _ = s.recorder.borrow_mut().step_forward();
        }
        LIVE => {
            s.reverse.set(false);
            s.recorder.borrow_mut().resume();
        }
        FORK => fork(s, restore),
        _ => {}
    }
    repaint(s);
}

/// Restore the selected frame's recorded sim state into the live app via
/// `restore`, then resume live play from it (a new timeline branch). A no-op with
/// no selection, no recorded bytes, or no restore hook.
fn fork(s: &Shared, restore: &Option<RestoreHook>) {
    let bytes = {
        let recorder = s.recorder.borrow();
        recorder
            .selected_frame()
            .and_then(|frame| recorder.frame(frame).ok())
            .map(|capture| capture.state_bytes().to_vec())
    };
    match (restore, bytes) {
        (Some(restore), Some(bytes)) => {
            restore(&bytes);
            s.reverse.set(false);
            s.active.set(true);
            s.recorder.borrow_mut().resume();
        }
        _ => {}
    }
}

/// Wire pointer-drag on the header into the panel's interface drag model, so the
/// overlay can be dragged anywhere (clamped to the viewport).
fn install_drag(window: &web_sys::Window, s: &Shared) {
    let down = {
        let shared = s.clone();
        Closure::<dyn FnMut(PointerEvent)>::new(move |e: PointerEvent| {
            shared
                .interface
                .borrow_mut()
                .drag_begin(shared.panel, e.client_x(), e.client_y());
        })
    };
    let _ = s
        .nodes
        .header
        .add_event_listener_with_callback("pointerdown", down.as_ref().unchecked_ref());
    down.forget();

    let mv = {
        let shared = s.clone();
        let window = window.clone();
        Closure::<dyn FnMut(PointerEvent)>::new(move |e: PointerEvent| {
            let dragging = shared.interface.borrow().is_dragging(shared.panel);
            dragging.then(|| {
                let max_x = viewport(&window, true);
                let max_y = viewport(&window, false);
                shared.interface.borrow_mut().drag_update(
                    shared.panel,
                    e.client_x(),
                    e.client_y(),
                    max_x,
                    max_y,
                );
                repaint(&shared);
            });
        })
    };
    let _ = window.add_event_listener_with_callback("pointermove", mv.as_ref().unchecked_ref());
    mv.forget();

    let up = {
        let shared = s.clone();
        Closure::<dyn FnMut(PointerEvent)>::new(move |_e: PointerEvent| {
            shared.interface.borrow_mut().drag_end(shared.panel);
        })
    };
    let _ = window.add_event_listener_with_callback("pointerup", up.as_ref().unchecked_ref());
    up.forget();
}

/// The viewport extent in CSS pixels: width when `is_width`, else height. Used as
/// the drag clamp bound; falls back to a large value if unreadable.
fn viewport(window: &web_sys::Window, is_width: bool) -> i32 {
    let measured = if is_width {
        window.inner_width()
    } else {
        window.inner_height()
    };
    measured
        .ok()
        .and_then(|v| v.as_f64())
        .map(|n| n as i32)
        .unwrap_or(4096)
}

/// Wire the focus/visibility listeners that gate recording. Recording **pauses**
/// when the game loses focus — Escape, window blur, or the tab being hidden — and
/// **resumes** on return: window focus, the tab becoming visible, or a click back
/// into the page (which also covers re-engaging an FPS pointer-lock). Each handler
/// repaints so `LIVE`/`PAUSED` stays current. The handlers do not consume the
/// events, so each game's own input handling is unaffected.
fn install_focus_listeners(window: &web_sys::Window, document: &web_sys::Document, s: &Shared) {
    // Escape pauses recording (the key is observed, not consumed).
    let on_key = {
        let shared = s.clone();
        Closure::<dyn FnMut(KeyboardEvent)>::new(move |e: KeyboardEvent| {
            (e.key() == "Escape").then(|| {
                shared.active.set(false);
                repaint(&shared);
            });
        })
    };
    let _ = window.add_event_listener_with_callback("keydown", on_key.as_ref().unchecked_ref());
    on_key.forget();

    // Window blur pauses; focus resumes; a click back in resumes.
    add_toggle(window, "blur", s, false);
    add_toggle(window, "focus", s, true);
    add_toggle(document, "pointerdown", s, true);

    // Tab visibility: hidden pauses, visible resumes.
    let on_visibility = {
        let shared = s.clone();
        let doc = document.clone();
        Closure::<dyn FnMut()>::new(move || {
            shared.active.set(!doc.hidden());
            repaint(&shared);
        })
    };
    let _ = document.add_event_listener_with_callback(
        "visibilitychange",
        on_visibility.as_ref().unchecked_ref(),
    );
    on_visibility.forget();
}

/// Add a listener that sets `active` to a fixed `value` on `event`, then repaints.
fn add_toggle<T: AsRef<EventTarget>>(target: &T, event: &str, s: &Shared, value: bool) {
    let shared = s.clone();
    let cb = Closure::<dyn FnMut()>::new(move || {
        shared.active.set(value);
        repaint(&shared);
    });
    let _ = target
        .as_ref()
        .add_event_listener_with_callback(event, cb.as_ref().unchecked_ref());
    cb.forget();
}

/// Reflect the `visible` flag onto the panel's DOM: add the `--hidden` class when
/// hidden, remove it when shown. `render` only ever writes the `style` attribute
/// (position/width), never the class, so this class swap survives every repaint.
fn apply_visibility(s: &Shared) {
    let class = s
        .visible
        .get()
        .then_some("axiom-scrub")
        .unwrap_or("axiom-scrub axiom-scrub--hidden");
    s.nodes.root.set_class_name(class);
}

/// Flip the overlay's visibility and reflect it onto the DOM. Shared by the F2
/// hotkey and the `axiom:scrubber-toggle` signal the debug console dispatches.
fn toggle_visibility(s: &Shared) {
    s.visible.set(!s.visible.get());
    apply_visibility(s);
}

/// Wire the two ways to toggle the overlay: the **F2** hotkey (observed on
/// `window`, never consumed, so game input is unaffected) and the
/// `axiom:scrubber-toggle` custom event the debug overlay's `scrubber` command
/// dispatches to reach the scrubber across the module boundary (they share no
/// interface instance).
fn install_visibility_toggle(window: &web_sys::Window, s: &Shared) {
    let on_key = {
        let shared = s.clone();
        Closure::<dyn FnMut(KeyboardEvent)>::new(move |e: KeyboardEvent| {
            (e.key() == "F2").then(|| toggle_visibility(&shared));
        })
    };
    let _ = window.add_event_listener_with_callback("keydown", on_key.as_ref().unchecked_ref());
    on_key.forget();

    let on_signal = {
        let shared = s.clone();
        Closure::<dyn FnMut()>::new(move || toggle_visibility(&shared))
    };
    let _ = window.add_event_listener_with_callback(
        "axiom:scrubber-toggle",
        on_signal.as_ref().unchecked_ref(),
    );
    on_signal.forget();
}

/// Encode the present arguments into the recorder's opaque render bytes.
fn encode(clear: [f32; 4], lights: &[Light], light_vp: [f32; 16], batches: &[Batch]) -> Vec<u8> {
    let mut bytes = Vec::new();
    clear.iter().for_each(|f| put_f32(&mut bytes, *f));
    light_vp.iter().for_each(|f| put_f32(&mut bytes, *f));
    put_u32(&mut bytes, lights.len() as u32);
    lights.iter().for_each(|(index, dir, colour, intensity)| {
        put_u32(&mut bytes, *index);
        dir.iter().for_each(|f| put_f32(&mut bytes, *f));
        colour.iter().for_each(|f| put_f32(&mut bytes, *f));
        put_f32(&mut bytes, *intensity);
    });
    put_u32(&mut bytes, batches.len() as u32);
    batches
        .iter()
        .for_each(|(mesh, material, instances, count)| {
            put_u64(&mut bytes, *mesh);
            put_u64(&mut bytes, *material);
            put_u32(&mut bytes, *count);
            put_u32(&mut bytes, instances.len() as u32);
            instances.iter().for_each(|f| put_f32(&mut bytes, *f));
        });
    bytes
}

/// Decode present arguments produced by [`encode`]. Returns `None` on a
/// truncated/unreadable payload (never produced by `encode`), so the caller can
/// safely fall back to presenting nothing.
fn decode(bytes: &[u8]) -> Option<PresentArgs> {
    let mut cur = Cursor { bytes, at: 0 };
    let clear = [cur.f32()?, cur.f32()?, cur.f32()?, cur.f32()?];
    let mut light_vp = [0.0_f32; 16];
    for slot in light_vp.iter_mut() {
        *slot = cur.f32()?;
    }
    let light_count = cur.u32()? as usize;
    let mut lights = Vec::with_capacity(light_count);
    for _ in 0..light_count {
        let index = cur.u32()?;
        let dir = [cur.f32()?, cur.f32()?, cur.f32()?];
        let colour = [cur.f32()?, cur.f32()?, cur.f32()?];
        lights.push((index, dir, colour, cur.f32()?));
    }
    let batch_count = cur.u32()? as usize;
    let mut batches = Vec::with_capacity(batch_count);
    for _ in 0..batch_count {
        let mesh = cur.u64()?;
        let material = cur.u64()?;
        let count = cur.u32()?;
        let instance_len = cur.u32()? as usize;
        let mut instances = Vec::with_capacity(instance_len);
        for _ in 0..instance_len {
            instances.push(cur.f32()?);
        }
        batches.push((mesh, material, instances, count));
    }
    Some((clear, lights, light_vp, batches))
}

fn put_f32(bytes: &mut Vec<u8>, value: f32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}
fn put_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}
fn put_u64(bytes: &mut Vec<u8>, value: u64) {
    bytes.extend_from_slice(&value.to_le_bytes());
}
fn put_i32(bytes: &mut Vec<u8>, value: i32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}
fn put_u8(bytes: &mut Vec<u8>, value: u8) {
    bytes.push(value);
}

// This is a *one-way* read of the host's 2D draw contract into deterministic
// bytes: it drives the recorder's memory/hash/eviction bookkeeping only — the
// frame replayed while scrubbing comes from the clone store, not from decoding
// these bytes (see the module docs for why this module can read the contract but
// not rebuild it). Reading needs no `axiom-math` name: vectors are read through
// their public `x`/`y` fields and the baked transform through `as_cols_array`,
// so nothing here forces a cross-layer import.

/// Encode the 4 colour channels of an [`axiom_host::Rgba`].
fn put_rgba(bytes: &mut Vec<u8>, color: axiom_host::Rgba) {
    let [r, g, b, a] = color.channels();
    put_f32(bytes, r);
    put_f32(bytes, g);
    put_f32(bytes, b);
    put_f32(bytes, a);
}

/// Encode an [`axiom_host::Rect`] as `min.xy` then `size.xy`.
fn put_rect(bytes: &mut Vec<u8>, rect: axiom_host::Rect) {
    put_f32(bytes, rect.min.x);
    put_f32(bytes, rect.min.y);
    put_f32(bytes, rect.size.x);
    put_f32(bytes, rect.size.y);
}

/// Encode the present arguments of a 2D frame into the recorder's opaque render
/// bytes: the clear colour followed by the full layer-sorted list.
fn encode_2d(clear: [f32; 4], list: &axiom_host::Draw2dList) -> Vec<u8> {
    let mut bytes = Vec::new();
    clear.iter().for_each(|f| put_f32(&mut bytes, *f));
    encode_list(&mut bytes, list);
    bytes
}

/// Encode a [`axiom_host::Draw2dList`]: camera, paint table, commands, and each
/// off-screen render target (dimensions + its routed commands). Render targets
/// only ever carry routed commands (the builder has no way to nest paints / a
/// camera / sub-targets into one), so encoding their command lists is faithful to
/// anything a list can actually hold.
fn encode_list(bytes: &mut Vec<u8>, list: &axiom_host::Draw2dList) {
    match list.camera() {
        Some(camera) => {
            put_u8(bytes, 1);
            put_f32(bytes, camera.center.x);
            put_f32(bytes, camera.center.y);
            put_f32(bytes, camera.zoom.get());
        }
        None => put_u8(bytes, 0),
    }

    let paint_count = list.paint_count() as u32;
    put_u32(bytes, paint_count);
    (0..paint_count).for_each(|raw| {
        let id = axiom_host::PaintId::from_raw(raw);
        match list.paint_linear(id) {
            Some((from, to)) => {
                put_u8(bytes, 0);
                put_f32(bytes, from.x);
                put_f32(bytes, from.y);
                put_f32(bytes, to.x);
                put_f32(bytes, to.y);
            }
            None => {
                put_u8(bytes, 1);
                // A paint is linear xor radial; if not linear it is radial. The
                // `unwrap_or` keeps this total without a panic on the impossible
                // arm (a registered id is always one or the other).
                let (center, radius) = list
                    .paint_radial(id)
                    .map(|(c, r)| (c.x, c.y, r.get()))
                    .map(|(x, y, r)| ((x, y), r))
                    .unwrap_or(((0.0, 0.0), 0.0));
                put_f32(bytes, center.0);
                put_f32(bytes, center.1);
                put_f32(bytes, radius);
            }
        }
        let stops = list.paint_stops(id).unwrap_or_default();
        put_u32(bytes, stops.len() as u32);
        stops.iter().for_each(|stop| {
            put_f32(bytes, stop.offset.get());
            put_rgba(bytes, stop.color);
        });
    });

    put_u32(bytes, list.commands().len() as u32);
    list.commands()
        .iter()
        .for_each(|command| encode_command(bytes, command));

    let target_count = list.render_target_count() as u32;
    put_u32(bytes, target_count);
    (0..target_count).for_each(|raw| {
        let id = axiom_host::RenderTargetId::from_raw(raw);
        let (width, height) = list.target_dimensions(id).unwrap_or((0, 0));
        put_u32(bytes, width);
        put_u32(bytes, height);
        let commands = list.target_commands(id).unwrap_or(&[]);
        put_u32(bytes, commands.len() as u32);
        commands
            .iter()
            .for_each(|command| encode_command(bytes, command));
    });
}

/// Encode one [`axiom_host::Draw2dCommand`]: its kind, submit index, baked
/// transform, resolved common attributes (layer / alpha / shadow), fill, and the
/// one geometry payload its kind selects. Every `as_*` accessor returns `Some`
/// only for the matching kind, so exactly one geometry block is emitted.
fn encode_command(bytes: &mut Vec<u8>, command: &axiom_host::Draw2dCommand) {
    put_u32(bytes, command.kind_code());
    put_u32(bytes, command.submission_index());
    command
        .transform()
        .as_cols_array()
        .iter()
        .for_each(|f| put_f32(bytes, *f));
    put_i32(bytes, command.layer());
    put_f32(bytes, command.alpha().get());
    match command.shadow() {
        Some(shadow) => {
            put_u8(bytes, 1);
            put_rgba(bytes, shadow.color);
            put_f32(bytes, shadow.blur.get());
        }
        None => put_u8(bytes, 0),
    }
    match command.fill() {
        Some(fill) => {
            put_u8(bytes, 1);
            match fill.fill_color {
                Some(color) => {
                    put_u8(bytes, 1);
                    put_rgba(bytes, color);
                }
                None => put_u8(bytes, 0),
            }
            match fill.fill_paint {
                Some(paint) => {
                    put_u8(bytes, 1);
                    put_u32(bytes, paint.raw());
                }
                None => put_u8(bytes, 0),
            }
            match fill.stroke {
                Some(stroke) => {
                    put_u8(bytes, 1);
                    put_rgba(bytes, stroke.color);
                    put_f32(bytes, stroke.width.get());
                }
                None => put_u8(bytes, 0),
            }
        }
        None => put_u8(bytes, 0),
    }

    command
        .as_rect()
        .into_iter()
        .for_each(|r| put_rect(bytes, r));
    command
        .as_circle()
        .into_iter()
        .for_each(|(center, radius)| {
            put_f32(bytes, center.x);
            put_f32(bytes, center.y);
            put_f32(bytes, radius.get());
        });
    command
        .as_ellipse()
        .into_iter()
        .for_each(|(center, rx, ry, rotation)| {
            put_f32(bytes, center.x);
            put_f32(bytes, center.y);
            put_f32(bytes, rx.get());
            put_f32(bytes, ry.get());
            put_f32(bytes, rotation.get());
        });
    command
        .as_line()
        .into_iter()
        .for_each(|(a, b, color, width)| {
            put_f32(bytes, a.x);
            put_f32(bytes, a.y);
            put_f32(bytes, b.x);
            put_f32(bytes, b.y);
            put_rgba(bytes, color);
            put_f32(bytes, width.get());
        });
    command.as_path().into_iter().for_each(|(points, closed)| {
        put_u32(bytes, points.len() as u32);
        points.iter().for_each(|p| {
            put_f32(bytes, p.x);
            put_f32(bytes, p.y);
        });
        put_u8(bytes, u8::from(closed));
    });
    command.as_sprite().into_iter().for_each(|(texture, opts)| {
        put_u64(bytes, texture.raw());
        put_rect(bytes, opts.source);
        put_f32(bytes, opts.anchor.x);
        put_f32(bytes, opts.anchor.y);
        put_rgba(bytes, opts.tint);
        put_u8(bytes, u8::from(opts.flip_x));
        put_u8(bytes, u8::from(opts.flip_y));
    });
    command.as_text().into_iter().for_each(|(run, opts)| {
        put_u32(bytes, run.glyphs.len() as u32);
        run.glyphs.iter().for_each(|glyph| {
            put_rect(bytes, glyph.source);
            put_f32(bytes, glyph.advance.get());
        });
        put_f32(bytes, run.line_height.get());
        put_u64(bytes, opts.font.raw());
        put_rgba(bytes, opts.color);
        put_u8(bytes, opts.align.raw());
    });
    command
        .as_particle()
        .into_iter()
        .for_each(|(center, size, color)| {
            put_f32(bytes, center.x);
            put_f32(bytes, center.y);
            put_f32(bytes, size.get());
            put_rgba(bytes, color);
        });
}

/// A tiny bounds-checked little-endian reader over the recorded payload.
struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Cursor<'_> {
    fn take<const N: usize>(&mut self) -> Option<[u8; N]> {
        let end = self.at + N;
        let slice = self.bytes.get(self.at..end)?;
        self.at = end;
        slice.try_into().ok()
    }
    fn f32(&mut self) -> Option<f32> {
        self.take::<4>().map(f32::from_le_bytes)
    }
    fn u32(&mut self) -> Option<u32> {
        self.take::<4>().map(u32::from_le_bytes)
    }
    fn u64(&mut self) -> Option<u64> {
        self.take::<8>().map(u64::from_le_bytes)
    }
}
