//! The DOM presentation arm — the thin `wasm32` edge that **renders the neutral
//! [`axiom_interface::InterfaceDrawList`]** onto real DOM nodes and wires the
//! browser keyboard/pointer events back into the pure [`OverlayState`]. Compiled
//! only for `wasm32`, behind the facade, so it never enters the native build, the
//! coverage gate, or the branchless lint (exactly like windowing's live arm). It
//! holds no decision logic: "what to draw" is the draw list, and every "what
//! should happen" is delegated to the pure core ([`OverlayState::apply_key`],
//! [`OverlayState::submit_command`], [`OverlayState::apply_shortcut`]).

use std::cell::RefCell;
use std::rc::Rc;

use axiom_interface::{InterfaceDrawItem, InterfaceInputEvent};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{Document, Element, HtmlInputElement, KeyboardEvent, PointerEvent, Window};

use crate::overlay_state::OverlayState;

const OVERLAY_ID: &str = "axiom-debug-overlay";
const STYLE_ID: &str = "axiom-dbg-style";
const CONSOLE_INPUT_ID: &str = "axiom-dbg-console-input";

/// The overlay's own stylesheet: monospace, square corners, high contrast,
/// compact, top-left, click-through except the console input. Width is driven by
/// the density class so it survives a drag (which only rewrites left/top).
const OVERLAY_CSS: &str = r#"
#axiom-debug-overlay {
  position: fixed; top: 0; left: 0; z-index: 2147483000;
  box-sizing: border-box; width: 360px; max-width: calc(100vw - 16px);
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, "Liberation Mono", monospace;
  font-size: 12px; line-height: 1.45; color: #d8f6ec;
  background: rgba(6, 10, 13, 0.86); border: 1px solid #00e6a8; border-radius: 0;
  pointer-events: none; -webkit-user-select: none; user-select: none;
}
#axiom-debug-overlay[hidden] { display: none; }
#axiom-debug-overlay.axiom-dbg--verbose { width: 460px; }
.axiom-dbg-header {
  display: flex; justify-content: space-between; align-items: baseline; gap: 8px;
  padding: 5px 8px; background: #00e6a8; color: #00130d;
  font-weight: 700; text-transform: uppercase; letter-spacing: 0.05em;
  cursor: move; pointer-events: auto; touch-action: none;
}
.axiom-dbg-header:active { cursor: grabbing; }
.axiom-dbg-title { white-space: nowrap; }
.axiom-dbg-status { font-weight: 600; text-transform: none; letter-spacing: 0; opacity: 0.85; }
.axiom-dbg-rows { display: grid; grid-template-columns: auto 1fr; gap: 1px 10px; padding: 5px 8px; }
.axiom-dbg-key { color: #6fbfae; white-space: nowrap; }
.axiom-dbg-val { color: #eafff8; text-align: right; word-break: break-word; }
.axiom-dbg-console { border-top: 1px solid #0f3d33; padding: 5px 8px 7px; }
.axiom-dbg-results {
  display: flex; flex-direction: column; gap: 1px; margin-bottom: 5px;
  max-height: 104px; overflow: auto;
}
.axiom-dbg-result { white-space: pre-wrap; word-break: break-word; }
.axiom-dbg-result.ok { color: #bfe9dc; }
.axiom-dbg-result.err { color: #ff9a9a; }
.axiom-dbg-inputrow { display: flex; align-items: center; gap: 5px; }
.axiom-dbg-prompt { color: #00e6a8; font-weight: 700; }
.axiom-dbg-input {
  flex: 1 1 auto; min-width: 0; pointer-events: auto; font: inherit; color: #eafff8;
  background: #04120f; border: 1px solid #0f3d33; border-radius: 0; padding: 2px 5px; outline: none;
}
.axiom-dbg-input:focus { border-color: #00e6a8; background: #06201a; }
/* Click-to-copy: rows and console result lines are clickable (the overlay is
   otherwise click-through). Hover highlights the target; the cursor signals it. */
.axiom-dbg-key, .axiom-dbg-val, .axiom-dbg-result {
  pointer-events: auto; cursor: pointer;
}
.axiom-dbg-rows > :hover, .axiom-dbg-result:hover { background: rgba(0, 230, 168, 0.18); }
"#;

/// The mounted DOM node handles. Every field is a cheap web_sys clone of the same
/// underlying node, so the event listeners can hold their own clones.
#[derive(Debug, Clone)]
struct Nodes {
    parent: Element,
    root: Element,
    /// The title bar — the drag handle.
    header: Element,
    header_title: Element,
    header_status: Element,
    rows: Element,
    results: Element,
    prompt: Element,
    input: HtmlInputElement,
    style: Element,
}

/// A mounted overlay: the DOM nodes plus the live keyboard/pointer listeners (kept
/// alive here, removed on [`Binding::unmount`]).
#[derive(Debug)]
pub(crate) struct Binding {
    nodes: Nodes,
    window_keydown: Closure<dyn FnMut(KeyboardEvent)>,
    input_keydown: Closure<dyn FnMut(KeyboardEvent)>,
    pointer_down: Closure<dyn FnMut(PointerEvent)>,
    pointer_move: Closure<dyn FnMut(PointerEvent)>,
    pointer_up: Closure<dyn FnMut(PointerEvent)>,
    copy_click: Closure<dyn FnMut(web_sys::Event)>,
}

impl Binding {
    /// Repaint the DOM from the current state's draw list.
    pub(crate) fn sync(&self, state: &OverlayState) {
        sync_nodes(&self.nodes, state);
    }

    /// Focus the console input element.
    pub(crate) fn focus_input(&self) {
        let _ = self.nodes.input.focus();
    }

    /// Remove the listeners, the overlay nodes, and the injected style.
    pub(crate) fn unmount(self) {
        let _ = window().remove_event_listener_with_callback(
            "keydown",
            self.window_keydown.as_ref().unchecked_ref(),
        );
        let _ = self.nodes.input.remove_event_listener_with_callback(
            "keydown",
            self.input_keydown.as_ref().unchecked_ref(),
        );
        let header = &self.nodes.header;
        let _ = header.remove_event_listener_with_callback(
            "pointerdown",
            self.pointer_down.as_ref().unchecked_ref(),
        );
        let _ = header.remove_event_listener_with_callback(
            "pointermove",
            self.pointer_move.as_ref().unchecked_ref(),
        );
        let _ = header.remove_event_listener_with_callback(
            "pointerup",
            self.pointer_up.as_ref().unchecked_ref(),
        );
        let _ = self
            .nodes
            .root
            .remove_event_listener_with_callback("click", self.copy_click.as_ref().unchecked_ref());
        let _ = self.nodes.parent.remove_child(&self.nodes.root);
        if let Some(style_parent) = self.nodes.style.parent_node() {
            let _ = style_parent.remove_child(&self.nodes.style);
        }
    }
}

/// `document.body` as an `Element`, if present.
pub(crate) fn body() -> Option<Element> {
    document().body().map(|body| body.unchecked_into())
}

/// Build the overlay DOM under `parent`, install the listeners (sharing `state`),
/// and return the [`Binding`]. Starts hidden; the caller syncs.
pub(crate) fn mount(state: &Rc<RefCell<OverlayState>>, parent: &Element) -> Binding {
    let document = document();
    inject_style(&document);
    let nodes = build_dom(&document, parent);
    let window_keydown = install_window_keydown(state, &nodes);
    let input_keydown = install_input_keydown(state, &nodes);
    let (pointer_down, pointer_move, pointer_up) = install_drag(state, &nodes);
    let copy_click = install_copy_click(state, &nodes);
    Binding {
        nodes,
        window_keydown,
        input_keydown,
        pointer_down,
        pointer_move,
        pointer_up,
        copy_click,
    }
}

fn install_window_keydown(
    state: &Rc<RefCell<OverlayState>>,
    nodes: &Nodes,
) -> Closure<dyn FnMut(KeyboardEvent)> {
    let state = state.clone();
    let nodes = nodes.clone();
    let callback = Closure::<dyn FnMut(KeyboardEvent)>::new(move |event: KeyboardEvent| {
        let (in_text_field, console_focus) = active_focus();
        let chord = InterfaceInputEvent {
            shift: event.shift_key(),
            ctrl: event.ctrl_key(),
            alt: event.alt_key(),
            meta: event.meta_key(),
            in_text_field,
            console_focus,
        };
        // The overlay's keymap resolves the physical key code; an unbound key
        // yields `None` and this is a no-op. The bound chords (Backquote +
        // modifiers) apply their action and are consumed.
        let action = state.borrow_mut().apply_key(&event.code(), chord);
        if let Some(_action) = action {
            event.prevent_default();
            // Reflect the pure model's console-focus onto the real input: opening
            // the overlay (or the explicit focus chord) lands the caret in the
            // box; closing it releases focus. The model owns the decision.
            if state.borrow().is_console_focused() {
                let _ = nodes.input.focus();
            } else {
                let _ = nodes.input.blur();
            }
            sync_nodes(&nodes, &state.borrow());
        }
    });
    window()
        .add_event_listener_with_callback("keydown", callback.as_ref().unchecked_ref())
        .expect("window keydown listener installs");
    callback
}

fn install_input_keydown(
    state: &Rc<RefCell<OverlayState>>,
    nodes: &Nodes,
) -> Closure<dyn FnMut(KeyboardEvent)> {
    // A separate handle to attach the listener to, since `nodes` itself is moved
    // into the closure below.
    let input = nodes.input.clone();
    let state = state.clone();
    let nodes = nodes.clone();
    // The overlay owns this binding: which physical key triggers which console
    // action (submit/dismiss/recall) is the consumer's policy, mirroring how the
    // Backquote binding stays in the overlay. The console *model* is the layer's.
    let callback = Closure::<dyn FnMut(KeyboardEvent)>::new(move |event: KeyboardEvent| {
        let mut handled = true;
        match event.key().as_str() {
            "Enter" => {
                let raw = nodes.input.value();
                state.borrow_mut().submit_command(&raw);
                // A `copy` command queues clipboard text; flush it here, inside
                // the keydown — the user gesture `navigator.clipboard` requires.
                flush_clipboard(&state);
                nodes.input.set_value("");
            }
            "Escape" => {
                state.borrow_mut().blur_console();
                let _ = nodes.input.blur();
            }
            "ArrowUp" => {
                if let Some(text) = state.borrow_mut().history_prev() {
                    nodes.input.set_value(&text);
                }
            }
            "ArrowDown" => {
                if let Some(text) = state.borrow_mut().history_next() {
                    nodes.input.set_value(&text);
                }
            }
            _ => handled = false,
        }
        if handled {
            event.prevent_default();
            sync_nodes(&nodes, &state.borrow());
        }
    });
    input
        .add_event_listener_with_callback("keydown", callback.as_ref().unchecked_ref())
        .expect("console keydown listener installs");
    callback
}

/// Inspect `document.activeElement`: is it a text-entry element, and is it our
/// own console input?
fn active_focus() -> (bool, bool) {
    document()
        .active_element()
        .map(|element| {
            let console_owns_focus = element.id() == CONSOLE_INPUT_ID;
            let tag = element.tag_name().to_ascii_lowercase();
            let editable = element
                .dyn_ref::<web_sys::HtmlElement>()
                .map(web_sys::HtmlElement::is_content_editable)
                .unwrap_or(false);
            let is_text = tag == "input" || tag == "textarea" || editable;
            (is_text, console_owns_focus)
        })
        .unwrap_or((false, false))
}

/// Render the neutral draw list into the fixed DOM skeleton. The list is empty
/// when the panel is hidden, so an empty list hides the root.
fn sync_nodes(nodes: &Nodes, state: &OverlayState) {
    let visible = state.is_visible();
    set_hidden(&nodes.root, !visible);
    if !visible {
        return;
    }
    // Density class drives the CSS width (so it survives a left/top-only drag).
    nodes.root.set_class_name(&format!(
        "axiom-dbg-overlay axiom-dbg--{}",
        state.density_label()
    ));
    // Collect specs, then RECONCILE the DOM in place (reuse existing nodes)
    // instead of clear-and-rebuild: rebuilding would break click-to-copy (a
    // mousedown/mouseup on a replaced node fires no `click`) and churn the DOM
    // 60×/sec for nothing.
    let mut row_cells: Vec<ChildSpec> = Vec::new();
    let mut result_lines: Vec<ChildSpec> = Vec::new();
    for item in state.draw_list().items() {
        match item {
            InterfaceDrawItem::Panel { x, y, .. } => {
                let _ = nodes
                    .root
                    .set_attribute("style", &format!("left:{x}px;top:{y}px;"));
            }
            InterfaceDrawItem::Header { primary, secondary } => {
                nodes.header_title.set_text_content(Some(primary));
                nodes.header_status.set_text_content(Some(secondary));
            }
            InterfaceDrawItem::Row { label, value } => {
                // Both cells carry the whole row's copy text, so clicking either
                // the key or the value copies "label: value".
                let copy = format!("{label}: {value}");
                row_cells.push(ChildSpec::copyable("axiom-dbg-key", label, &copy));
                row_cells.push(ChildSpec::copyable("axiom-dbg-val", value, &copy));
            }
            InterfaceDrawItem::ConsoleLine { ok, text } => {
                let class = if *ok {
                    "axiom-dbg-result ok"
                } else {
                    "axiom-dbg-result err"
                };
                result_lines.push(ChildSpec::copyable(class, text, text));
            }
            InterfaceDrawItem::ConsoleInput { prompt, .. } => {
                nodes.prompt.set_text_content(Some(prompt));
            }
            // The overlay never sets panel actions, so it emits no buttons; the
            // arm exists only to keep the match exhaustive.
            InterfaceDrawItem::Button { .. } => {}
        }
    }
    let document = document();
    reconcile_children(&document, &nodes.rows, &row_cells);
    reconcile_children(&document, &nodes.results, &result_lines);
}

/// One desired child `div`: its class, text, and the click-to-copy payload.
struct ChildSpec {
    class: String,
    text: String,
    copy: String,
}

impl ChildSpec {
    fn copyable(class: &str, text: &str, copy: &str) -> Self {
        ChildSpec {
            class: class.to_string(),
            text: text.to_string(),
            copy: copy.to_string(),
        }
    }
}

/// Update `container`'s children to match `specs`, reusing existing element nodes
/// by position (so node identity is stable across the per-frame repaint) and only
/// creating/removing nodes when the count changes.
fn reconcile_children(document: &Document, container: &Element, specs: &[ChildSpec]) {
    let existing = container.children();
    for (index, spec) in specs.iter().enumerate() {
        let element = existing.item(index as u32).unwrap_or_else(|| {
            let created = make_div(document, &spec.class, "");
            let _ = container.append_child(&created);
            created
        });
        element.set_class_name(&spec.class);
        element.set_text_content(Some(&spec.text));
        set_copy(&element, &spec.copy);
    }
    // Drop any trailing nodes left over from a longer previous render.
    while existing.length() as usize > specs.len() {
        match container.last_element_child() {
            Some(extra) => {
                let _ = container.remove_child(&extra);
            }
            None => break,
        }
    }
}

fn build_dom(document: &Document, parent: &Element) -> Nodes {
    // Idempotent at the DOM level: drop any pre-existing overlay root before
    // building a fresh one (mirrors `inject_style`'s dedup), so a repeated
    // `start()` (wasm2js fallback re-init, HMR reload) never stacks two overlays.
    remove_existing(document, OVERLAY_ID);
    let root = document.create_element("div").expect("create overlay root");
    root.set_id(OVERLAY_ID);
    root.set_class_name("axiom-dbg-overlay axiom-dbg--normal");
    set_hidden(&root, true);

    let header = make_div(document, "axiom-dbg-header", "");
    let header_title = make_div(document, "axiom-dbg-title", "Axiom Debug Overlay");
    let _ = header.append_child(&header_title);
    let header_status = make_div(document, "axiom-dbg-status", "normal");
    let _ = header.append_child(&header_status);

    let rows = make_div(document, "axiom-dbg-rows", "");

    let console = make_div(document, "axiom-dbg-console", "");
    let results = make_div(document, "axiom-dbg-results", "");
    let inputrow = make_div(document, "axiom-dbg-inputrow", "");
    let prompt = make_div(document, "axiom-dbg-prompt", ">");
    let _ = inputrow.append_child(&prompt);
    let input = make_console_input(document);
    let _ = inputrow.append_child(&input);
    let _ = console.append_child(&results);
    let _ = console.append_child(&inputrow);

    let _ = root.append_child(&header);
    let _ = root.append_child(&rows);
    let _ = root.append_child(&console);
    let _ = parent.append_child(&root);

    let style = document
        .get_element_by_id(STYLE_ID)
        .expect("style injected before build");

    Nodes {
        parent: parent.clone(),
        root,
        header,
        header_title,
        header_status,
        rows,
        results,
        prompt,
        input,
        style,
    }
}

fn make_console_input(document: &Document) -> HtmlInputElement {
    let element = document
        .create_element("input")
        .expect("create console input");
    element.set_id(CONSOLE_INPUT_ID);
    element.set_class_name("axiom-dbg-input");
    let _ = element.set_attribute("type", "text");
    let _ = element.set_attribute("autocomplete", "off");
    let _ = element.set_attribute("autocapitalize", "off");
    let _ = element.set_attribute("spellcheck", "false");
    let _ = element.set_attribute("placeholder", "type `help`");
    let _ = element.set_attribute("aria-label", "Axiom debug console");
    element.dyn_into().expect("input is an HtmlInputElement")
}

fn inject_style(document: &Document) {
    if document.get_element_by_id(STYLE_ID).is_some() {
        return;
    }
    let style = document.create_element("style").expect("create style");
    style.set_id(STYLE_ID);
    style.set_text_content(Some(OVERLAY_CSS));
    if let Some(head) = document.head() {
        let _ = head.append_child(&style);
    }
}

/// Detach any element with `id` already in the document (and its parent), so a
/// rebuild starts from a clean slate. Used to keep the overlay mount idempotent.
fn remove_existing(document: &Document, id: &str) {
    if let Some(existing) = document.get_element_by_id(id) {
        if let Some(parent) = existing.parent_node() {
            let _ = parent.remove_child(&existing);
        }
    }
}

fn make_div(document: &Document, class: &str, text: &str) -> Element {
    let element = document.create_element("div").expect("create div");
    element.set_class_name(class);
    if !text.is_empty() {
        element.set_text_content(Some(text));
    }
    element
}

/// Mark `element` as click-to-copy: a `data-copy` payload the delegated click
/// listener reads, plus a hover title affordance.
fn set_copy(element: &Element, text: &str) {
    let _ = element.set_attribute("data-copy", text);
    let _ = element.set_attribute("title", "click to copy");
}

fn set_hidden(element: &Element, hidden: bool) {
    if hidden {
        let _ = element.set_attribute("hidden", "");
    } else {
        let _ = element.remove_attribute("hidden");
    }
}

/// Install the drag-to-move handlers on the header (the title bar): pointerdown
/// grabs + captures the pointer, pointermove moves the window, pointerup
/// releases. Pointer capture means the move/up fire even when the pointer leaves
/// the header mid-drag, and it works for mouse, touch, and pen alike.
fn install_drag(
    state: &Rc<RefCell<OverlayState>>,
    nodes: &Nodes,
) -> (
    Closure<dyn FnMut(PointerEvent)>,
    Closure<dyn FnMut(PointerEvent)>,
    Closure<dyn FnMut(PointerEvent)>,
) {
    let header = nodes.header.clone();

    let down = {
        let state = state.clone();
        let header = header.clone();
        Closure::<dyn FnMut(PointerEvent)>::new(move |event: PointerEvent| {
            event.prevent_default();
            let _ = header.set_pointer_capture(event.pointer_id());
            state
                .borrow_mut()
                .drag_begin(event.client_x(), event.client_y());
        })
    };

    let dragging = {
        let state = state.clone();
        let nodes = nodes.clone();
        Closure::<dyn FnMut(PointerEvent)>::new(move |event: PointerEvent| {
            if state.borrow().is_dragging() {
                let (max_x, max_y) = drag_bounds(&nodes.root);
                state
                    .borrow_mut()
                    .drag_update(event.client_x(), event.client_y(), max_x, max_y);
                apply_position(&nodes.root, &state.borrow());
            }
        })
    };

    let up = {
        let state = state.clone();
        let header = header.clone();
        Closure::<dyn FnMut(PointerEvent)>::new(move |event: PointerEvent| {
            let _ = header.release_pointer_capture(event.pointer_id());
            state.borrow_mut().drag_end();
        })
    };

    let _ = header.add_event_listener_with_callback("pointerdown", down.as_ref().unchecked_ref());
    let _ =
        header.add_event_listener_with_callback("pointermove", dragging.as_ref().unchecked_ref());
    let _ = header.add_event_listener_with_callback("pointerup", up.as_ref().unchecked_ref());
    (down, dragging, up)
}

/// The max top-left the window may take so it stays on screen: viewport minus the
/// overlay's own size.
fn drag_bounds(root: &Element) -> (i32, i32) {
    let window = window();
    let viewport_w = window
        .inner_width()
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0) as i32;
    let viewport_h = window
        .inner_height()
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0) as i32;
    (
        viewport_w - root.client_width(),
        viewport_h - root.client_height(),
    )
}

/// Write the current panel position onto the overlay root as inline `left`/`top`
/// (width stays on the density class, so a drag never resets it).
fn apply_position(root: &Element, state: &OverlayState) {
    let (x, y) = state.position();
    let _ = root.set_attribute("style", &format!("left:{x}px;top:{y}px;"));
}

/// One delegated `click` listener on the overlay root: a click on any element
/// carrying a `data-copy` payload (tagged by [`set_copy`]) queues that text and
/// flushes it to the clipboard. Click is a user gesture, so
/// `navigator.clipboard` is permitted here; other clicks are no-ops.
fn install_copy_click(
    state: &Rc<RefCell<OverlayState>>,
    nodes: &Nodes,
) -> Closure<dyn FnMut(web_sys::Event)> {
    let state = state.clone();
    let callback = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
        let copied = event
            .target()
            .and_then(|target| target.dyn_into::<Element>().ok())
            .and_then(|element| element.closest("[data-copy]").ok().flatten())
            .and_then(|element| element.get_attribute("data-copy"));
        if let Some(text) = copied {
            state.borrow_mut().request_clipboard(text);
            flush_clipboard(&state);
        }
    });
    let _ = nodes
        .root
        .add_event_listener_with_callback("click", callback.as_ref().unchecked_ref());
    callback
}

/// Drain the overlay's pending clipboard requests and write each to the system
/// clipboard. Must be called from inside a user gesture. The returned Promise
/// is intentionally dropped — a failure (e.g. an insecure context) is silently
/// ignored so it never disrupts the console.
fn flush_clipboard(state: &Rc<RefCell<OverlayState>>) {
    let requests = state.borrow_mut().take_clipboard_requests();
    let clipboard = window().navigator().clipboard();
    for text in &requests {
        let _ = clipboard.write_text(text);
    }
}

fn window() -> Window {
    web_sys::window().expect("a browser window")
}

fn document() -> Document {
    window().document().expect("a document")
}
