//! The DOM controller — the thin `wasm32` edge that projects the pure
//! [`OverlayState`] onto real DOM nodes and wires the browser keyboard events
//! back into it.
//!
//! This file owns *everything* that touches the browser: element creation, the
//! injected stylesheet, the window/console keydown listeners, and focus. It
//! holds no decision logic of its own — every "what should happen" is delegated
//! to the pure modules ([`classify`], [`CommandRegistry`], [`OverlayState`]),
//! which are unit-tested on native. The controller is verified in a real browser
//! (see `DEBUG_OVERLAY.md`), so it is kept deliberately mechanical.
//!
//! Invariants the controller guarantees:
//! * **Mount once, update many.** [`Self::mount`] is idempotent — a second call
//!   while mounted does nothing, so it never duplicates nodes or listeners.
//! * **Clean unmount.** [`Self::unmount`] removes the listeners, the overlay
//!   nodes, and the injected style.
//! * **Doesn't block the game.** The overlay container is `pointer-events: none`;
//!   only the console input opts back in, so clicks pass through to the canvas.

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{Document, Element, HtmlInputElement, KeyboardEvent, Window};

use crate::browser_diagnostics::BrowserDiagnosticsSnapshot;
use crate::browser_keyboard_shortcut::{
    classify, classify_console_key, ConsoleKey, KeyChord, OverlayShortcut,
};
use crate::debug_command_registry::CommandRegistry;
use crate::debug_overlay_density::OverlayDensity;
use crate::debug_overlay_state::OverlayState;

/// The overlay root element id.
const OVERLAY_ID: &str = "axiom-debug-overlay";
/// The injected stylesheet's id (so injection is idempotent).
const STYLE_ID: &str = "axiom-dbg-style";
/// The console input's id (so the keyboard classifier can recognise "our console
/// owns focus").
const CONSOLE_INPUT_ID: &str = "axiom-dbg-console-input";
/// How many recent results the controller paints (the state caps it too).
const RESULT_LINES: usize = 5;

/// The overlay's own stylesheet: monospace, square corners, high contrast,
/// compact, top-left, click-through except the console input. No rounded
/// corners, no pills, no animation.
const OVERLAY_CSS: &str = r#"
#axiom-debug-overlay {
  position: fixed; top: 0; left: 0; margin: 8px; z-index: 2147483000;
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
}
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
"#;

/// Handles to the mounted DOM nodes the controller updates in place.
#[derive(Debug)]
struct OverlayDom {
    /// Where `root` was appended (for removal on unmount).
    parent: Element,
    root: Element,
    header_status: Element,
    rows: Element,
    results: Element,
    input: HtmlInputElement,
    style: Element,
}

/// The shared mutable interior: the pure state plus its DOM projection (when
/// mounted). Shared (via `Rc<RefCell<_>>`) between the controller's methods and
/// the event-listener closures.
#[derive(Debug)]
struct Inner {
    state: OverlayState,
    dom: Option<OverlayDom>,
}

impl Inner {
    /// Project the current state onto the DOM. Cheap and safe to call every
    /// frame; a no-op (beyond hiding) while the overlay is hidden.
    fn sync(&self) {
        let Some(dom) = &self.dom else { return };
        let visible = self.state.is_visible();
        set_hidden(&dom.root, !visible);
        if !visible {
            return;
        }
        dom.root.set_class_name(&format!(
            "axiom-dbg-overlay axiom-dbg--{}",
            self.state.density().label()
        ));
        dom.header_status
            .set_text_content(Some(&self.state.header_status()));
        render_rows(&dom.rows, &self.state);
        render_results(&dom.results, &self.state);
    }
}

/// The public integration API. Construct one, [`mount`](Self::mount) it, then
/// drive it with the toggle/density/pin/console methods and feed it host
/// diagnostics via [`update_diagnostics`](Self::update_diagnostics).
#[derive(Debug)]
pub struct DebugOverlayController {
    inner: Rc<RefCell<Inner>>,
    window_keydown: Option<Closure<dyn FnMut(KeyboardEvent)>>,
    input_keydown: Option<Closure<dyn FnMut(KeyboardEvent)>>,
}

impl Default for DebugOverlayController {
    fn default() -> Self {
        DebugOverlayController::new()
    }
}

impl DebugOverlayController {
    pub fn new() -> Self {
        DebugOverlayController {
            inner: Rc::new(RefCell::new(Inner {
                state: OverlayState::new(),
                dom: None,
            })),
            window_keydown: None,
            input_keydown: None,
        }
    }

    /// Mount the overlay into `parent` and install the keyboard listeners.
    /// Idempotent: calling it again while mounted does nothing (no duplicate
    /// nodes or listeners).
    pub fn mount(&mut self, parent: &Element) {
        if self.inner.borrow().dom.is_some() {
            return;
        }
        let document = document();
        inject_style(&document);
        let dom = build_dom(&document, parent);
        self.install_input_keydown(&dom.input);
        self.inner.borrow_mut().dom = Some(dom);
        self.install_window_keydown();
        self.inner.borrow().sync();
    }

    /// Mount into `document.body`.
    pub fn mount_to_body(&mut self) {
        if let Some(body) = document().body() {
            self.mount(&body);
        }
    }

    /// Remove the listeners, the overlay nodes, and the injected style.
    pub fn unmount(&mut self) {
        let window_cb = self.window_keydown.take();
        let input_cb = self.input_keydown.take();
        if let Some(cb) = &window_cb {
            let _ = window()
                .remove_event_listener_with_callback("keydown", cb.as_ref().unchecked_ref());
        }
        let mut inner = self.inner.borrow_mut();
        if let Some(dom) = inner.dom.take() {
            if let Some(cb) = &input_cb {
                let _ = dom
                    .input
                    .remove_event_listener_with_callback("keydown", cb.as_ref().unchecked_ref());
            }
            let _ = dom.parent.remove_child(&dom.root);
            if let Some(style_parent) = dom.style.parent_node() {
                let _ = style_parent.remove_child(&dom.style);
            }
        }
    }

    // --- integration API (interior-mutable; safe to call after `mount`) ------

    pub fn toggle(&self) {
        self.mutate(OverlayState::toggle);
    }

    pub fn show(&self) {
        self.mutate(OverlayState::show);
    }

    pub fn hide(&self) {
        self.mutate(OverlayState::hide);
    }

    pub fn is_visible(&self) -> bool {
        self.inner.borrow().state.is_visible()
    }

    pub fn cycle_density(&self) {
        self.mutate(OverlayState::cycle_density);
    }

    pub fn set_density(&self, density: OverlayDensity) {
        self.mutate(move |state| state.set_density(density));
    }

    pub fn pin(&self) {
        self.mutate(OverlayState::pin);
    }

    pub fn unpin(&self) {
        self.mutate(OverlayState::unpin);
    }

    pub fn is_pinned(&self) -> bool {
        self.inner.borrow().state.is_pinned()
    }

    /// Open the overlay, focus the console state, and focus the real input.
    pub fn focus_console(&self) {
        let mut inner = self.inner.borrow_mut();
        inner.state.focus_console();
        if let Some(dom) = &inner.dom {
            let _ = dom.input.focus();
        }
        inner.sync();
    }

    /// Feed the host's latest diagnostics in and repaint.
    pub fn update_diagnostics(&self, snapshot: BrowserDiagnosticsSnapshot) {
        let mut inner = self.inner.borrow_mut();
        inner.state.update_diagnostics(snapshot);
        inner.sync();
    }

    // --- internals ----------------------------------------------------------

    /// Mutate the pure state through `f`, then repaint.
    fn mutate(&self, f: impl FnOnce(&mut OverlayState)) {
        let mut inner = self.inner.borrow_mut();
        f(&mut inner.state);
        inner.sync();
    }

    fn install_window_keydown(&mut self) {
        let inner = self.inner.clone();
        let cb = Closure::<dyn FnMut(KeyboardEvent)>::new(move |event: KeyboardEvent| {
            if let Some(shortcut) = classify(chord_from_event(&event)) {
                // Only prevent default for shortcuts we actually handle.
                event.prevent_default();
                handle_shortcut(&inner, shortcut);
            }
        });
        let _ = window().add_event_listener_with_callback("keydown", cb.as_ref().unchecked_ref());
        self.window_keydown = Some(cb);
    }

    fn install_input_keydown(&mut self, input: &HtmlInputElement) {
        let inner = self.inner.clone();
        let cb = Closure::<dyn FnMut(KeyboardEvent)>::new(move |event: KeyboardEvent| {
            if let Some(action) = classify_console_key(&event.key()) {
                event.prevent_default();
                handle_console_key(&inner, action);
            }
        });
        let _ = input.add_event_listener_with_callback("keydown", cb.as_ref().unchecked_ref());
        self.input_keydown = Some(cb);
    }
}

/// Apply a classified overlay shortcut to the shared state, then repaint.
fn handle_shortcut(inner: &Rc<RefCell<Inner>>, shortcut: OverlayShortcut) {
    let mut guard = inner.borrow_mut();
    match shortcut {
        OverlayShortcut::ToggleOverlay => guard.state.toggle(),
        OverlayShortcut::CycleDensity => guard.state.cycle_density(),
        OverlayShortcut::TogglePinned => guard.state.toggle_pin(),
        OverlayShortcut::FocusConsole => {
            guard.state.focus_console();
            if let Some(dom) = &guard.dom {
                let _ = dom.input.focus();
            }
        }
    }
    guard.sync();
}

/// Apply a classified console key to the shared state, then repaint.
fn handle_console_key(inner: &Rc<RefCell<Inner>>, action: ConsoleKey) {
    let mut guard = inner.borrow_mut();
    match action {
        ConsoleKey::Submit => {
            let raw = guard
                .dom
                .as_ref()
                .map(|dom| dom.input.value())
                .unwrap_or_default();
            CommandRegistry::standard().execute(&mut guard.state, &raw);
            if let Some(dom) = &guard.dom {
                dom.input.set_value("");
            }
        }
        ConsoleKey::Dismiss => {
            guard.state.blur_console();
            if let Some(dom) = &guard.dom {
                let _ = dom.input.blur();
            }
        }
        ConsoleKey::HistoryPrev => {
            if let Some(text) = guard.state.history_prev() {
                set_input_value(&guard, &text);
            }
        }
        ConsoleKey::HistoryNext => {
            if let Some(text) = guard.state.history_next() {
                set_input_value(&guard, &text);
            }
        }
    }
    guard.sync();
}

fn set_input_value(guard: &Inner, text: &str) {
    if let Some(dom) = &guard.dom {
        dom.input.set_value(text);
    }
}

/// Lift a browser `KeyboardEvent` (plus the document's focus) into a [`KeyChord`].
fn chord_from_event(event: &KeyboardEvent) -> KeyChord {
    let (target_is_text_entry, console_owns_focus) = active_focus();
    KeyChord {
        code_is_backquote: event.code() == "Backquote",
        shift: event.shift_key(),
        ctrl: event.ctrl_key(),
        alt: event.alt_key(),
        meta: event.meta_key(),
        target_is_text_entry,
        console_owns_focus,
    }
}

/// Inspect `document.activeElement`: is it a text-entry element, and is it our
/// own console input?
fn active_focus() -> (bool, bool) {
    document()
        .active_element()
        .map(|el| {
            let console_owns_focus = el.id() == CONSOLE_INPUT_ID;
            let tag = el.tag_name().to_ascii_lowercase();
            let editable = el
                .dyn_ref::<web_sys::HtmlElement>()
                .map(web_sys::HtmlElement::is_content_editable)
                .unwrap_or(false);
            let is_text = tag == "input" || tag == "textarea" || editable;
            (is_text, console_owns_focus)
        })
        .unwrap_or((false, false))
}

fn render_rows(container: &Element, state: &OverlayState) {
    clear_children(container);
    let document = document();
    for row in state.visible_rows() {
        let _ = container.append_child(&make_div(&document, "axiom-dbg-key", &row.label));
        let _ = container.append_child(&make_div(&document, "axiom-dbg-val", &row.value));
    }
}

fn render_results(container: &Element, state: &OverlayState) {
    clear_children(container);
    let document = document();
    for result in state.recent_results().iter().rev().take(RESULT_LINES).rev() {
        let class = if result.ok {
            "axiom-dbg-result ok"
        } else {
            "axiom-dbg-result err"
        };
        let text = format!("{}: {}", result.command, result.message);
        let _ = container.append_child(&make_div(&document, class, &text));
    }
}

fn build_dom(document: &Document, parent: &Element) -> OverlayDom {
    let root = document.create_element("div").expect("create overlay root");
    root.set_id(OVERLAY_ID);
    root.set_class_name("axiom-dbg-overlay axiom-dbg--normal");
    set_hidden(&root, true);

    let header = make_div(document, "axiom-dbg-header", "");
    let _ = header.append_child(&make_div(document, "axiom-dbg-title", "Axiom Debug Overlay"));
    let header_status = make_div(document, "axiom-dbg-status", "normal");
    let _ = header.append_child(&header_status);

    let rows = make_div(document, "axiom-dbg-rows", "");

    let console = make_div(document, "axiom-dbg-console", "");
    let results = make_div(document, "axiom-dbg-results", "");
    let inputrow = make_div(document, "axiom-dbg-inputrow", "");
    let _ = inputrow.append_child(&make_div(document, "axiom-dbg-prompt", ">"));
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
        .expect("style was injected before build");

    OverlayDom {
        parent: parent.clone(),
        root,
        header_status,
        rows,
        results,
        input,
        style,
    }
}

fn make_console_input(document: &Document) -> HtmlInputElement {
    let element = document.create_element("input").expect("create console input");
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
        return; // idempotent
    }
    let style = document.create_element("style").expect("create style");
    style.set_id(STYLE_ID);
    style.set_text_content(Some(OVERLAY_CSS));
    if let Some(head) = document.head() {
        let _ = head.append_child(&style);
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

fn clear_children(element: &Element) {
    while let Some(child) = element.last_child() {
        let _ = element.remove_child(&child);
    }
}

fn set_hidden(element: &Element, hidden: bool) {
    if hidden {
        let _ = element.set_attribute("hidden", "");
    } else {
        let _ = element.remove_attribute("hidden");
    }
}

fn window() -> Window {
    web_sys::window().expect("a browser window")
}

fn document() -> Document {
    window().document().expect("a document")
}
