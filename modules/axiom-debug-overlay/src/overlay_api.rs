//! [`DebugOverlayApi`] — the module's single public facade.
//!
//! It owns the pure [`OverlayState`] (shared with the wasm DOM listeners via
//! `Rc<RefCell<_>>`) and exposes the overlay's whole behavior as primitive
//! values: booleans, integers, `&str`, and `(label, value)` / `(ok, command,
//! message)` tuples — no internal type and no naked float ever crosses the
//! boundary. The overlay's generic windowing (visibility, pin, focus, drag, the
//! console, the draw list) lives one layer down in [`axiom_interface`]; this
//! facade composes it and adds the debug-specific behavior. Every mutator
//! repaints; on native [`Self::repaint`] is a no-op, so the same calls are
//! exercised by ordinary tests, and on `wasm32` it syncs the DOM. The
//! `mount`/`unmount` DOM methods are `wasm32`-only and delegate to
//! [`crate::dom_binding`].

use std::cell::RefCell;
use std::rc::Rc;

use axiom_interface::InterfaceInputEvent;

use crate::overlay_state::OverlayState;

/// A developer debug overlay + command console for the browser engine surface.
#[derive(Debug)]
pub struct DebugOverlayApi {
    state: Rc<RefCell<OverlayState>>,
    #[cfg(target_arch = "wasm32")]
    binding: Option<crate::dom_binding::Binding>,
}

impl Default for DebugOverlayApi {
    fn default() -> Self {
        DebugOverlayApi::new()
    }
}

impl DebugOverlayApi {
    /// A fresh overlay: hidden, unpinned, normal density, placeholder diagnostics.
    pub fn new() -> Self {
        DebugOverlayApi {
            state: Rc::new(RefCell::new(OverlayState::new())),
            #[cfg(target_arch = "wasm32")]
            binding: None,
        }
    }

    pub fn is_visible(&self) -> bool {
        self.state.borrow().is_visible()
    }

    pub fn is_pinned(&self) -> bool {
        self.state.borrow().is_pinned()
    }

    pub fn is_console_focused(&self) -> bool {
        self.state.borrow().is_console_focused()
    }

    pub fn density_label(&self) -> &'static str {
        self.state.borrow().density_label()
    }

    pub fn command_history_count(&self) -> usize {
        self.state.borrow().command_history_count()
    }

    pub fn header_status(&self) -> String {
        self.state.borrow().header_status()
    }

    /// The labelled rows to render at the current density: `(label, value)`.
    pub fn visible_rows(&self) -> Vec<(String, String)> {
        self.state.borrow().rows()
    }

    /// The recent command results above the input: `(ok, command, message)`.
    pub fn recent_results(&self) -> Vec<(bool, String, String)> {
        self.state.borrow().recent_results()
    }

    pub fn toggle(&self) {
        self.state.borrow_mut().toggle();
        self.repaint();
    }

    pub fn show(&self) {
        self.state.borrow_mut().show();
        self.repaint();
    }

    pub fn hide(&self) {
        self.state.borrow_mut().hide();
        self.repaint();
    }

    pub fn cycle_density(&self) {
        self.state.borrow_mut().cycle_density();
        self.repaint();
    }

    /// Set density by label (`compact`/`normal`/`verbose`); unknown is ignored.
    pub fn set_density(&self, label: &str) {
        self.state.borrow_mut().set_density_label(label);
        self.repaint();
    }

    pub fn pin(&self) {
        self.state.borrow_mut().pin();
        self.repaint();
    }

    pub fn unpin(&self) {
        self.state.borrow_mut().unpin();
        self.repaint();
    }

    pub fn toggle_pin(&self) {
        self.state.borrow_mut().toggle_pin();
        self.repaint();
    }

    /// Open the overlay and focus the console (focuses the real input on wasm).
    pub fn focus_console(&self) {
        self.state.borrow_mut().focus_console();
        self.repaint();
        #[cfg(target_arch = "wasm32")]
        self.binding
            .as_ref()
            .into_iter()
            .for_each(crate::dom_binding::Binding::focus_input);
    }

    pub fn blur_console(&self) {
        self.state.borrow_mut().blur_console();
        self.repaint();
    }

    /// Classify and apply a physical-Backquote chord; returns whether it was
    /// handled (so the caller `preventDefault`s only handled chords). The generic
    /// "does this chord route as a global hotkey" lives in [`InterfaceInputEvent`];
    /// the modifier→action binding lives in [`crate::backquote`].
    pub fn handle_backquote(
        &self,
        shift: bool,
        ctrl: bool,
        alt: bool,
        meta: bool,
        in_text_field: bool,
        console_owns_focus: bool,
    ) -> bool {
        let chord = InterfaceInputEvent {
            shift,
            ctrl,
            alt,
            meta,
            in_text_field,
            console_focus: console_owns_focus,
        };
        let action = self.state.borrow_mut().apply_key("Backquote", chord);
        self.repaint();
        action.is_some()
    }

    /// Submit a raw console line through the overlay's command table (parse →
    /// dispatch → record → echo).
    pub fn console_submit(&self, raw: &str) {
        self.state.borrow_mut().submit_command(raw);
        self.repaint();
    }

    /// ArrowUp: recall an older command (the string to place in the input).
    pub fn console_history_prev(&self) -> Option<String> {
        let recalled = self.state.borrow_mut().history_prev();
        self.repaint();
        recalled
    }

    /// ArrowDown: recall a newer command (or the empty live line).
    pub fn console_history_next(&self) -> Option<String> {
        let recalled = self.state.borrow_mut().history_next();
        self.repaint();
        recalled
    }

    /// Escape: blur the console, keep the overlay open.
    pub fn console_dismiss(&self) {
        self.state.borrow_mut().blur_console();
        self.repaint();
    }
}

// Diagnostics ingestion + DOM mounting, in a second `impl` block so neither
// block exceeds the engine's per-impl item budget (a structural split, not a
// behavioural one — the surface is unchanged).
impl DebugOverlayApi {
    /// Live per-frame counters. Timing is integer-encoded: `fps_milli` is
    /// fps × 1000, `frame_time_micros` is the frame time in microseconds.
    pub fn set_frame(
        &self,
        frame_index: u64,
        tick: u64,
        sim_ticks: u32,
        fps_milli: u32,
        frame_time_micros: u32,
    ) {
        self.state.borrow_mut().set_frame(
            frame_index,
            tick,
            sim_ticks,
            fps_milli,
            frame_time_micros,
        );
        self.repaint();
    }

    /// The subsystem owners / live backends.
    pub fn set_backends(
        &self,
        renderer: &str,
        canvas_owner: &str,
        sim_owner: &str,
        storage: &str,
        audio: &str,
        network: &str,
    ) {
        self.state.borrow_mut().set_backends(
            renderer,
            canvas_owner,
            sim_owner,
            storage,
            audio,
            network,
        );
        self.repaint();
    }

    /// GPU/canvas/worker counters.
    pub fn set_counters(
        &self,
        webgpu_submissions: u64,
        canvas2d_frames: u64,
        worker_in: u64,
        worker_out: u64,
    ) {
        self.state.borrow_mut().set_counters(
            webgpu_submissions,
            canvas2d_frames,
            worker_in,
            worker_out,
        );
        self.repaint();
    }

    /// Render-fallback status.
    pub fn set_fallback(&self, count: u32, reason: &str) {
        self.state.borrow_mut().set_fallback(count, reason);
        self.repaint();
    }

    /// Document visibility state (e.g. `visible`/`hidden`).
    pub fn set_visibility(&self, visibility_state: &str) {
        self.state.borrow_mut().set_visibility(visibility_state);
        self.repaint();
    }

    /// Replace the app-specific read-out rows shown below the engine diagnostics:
    /// `(label, value)` pairs the app formats itself (e.g. a game's player pose, as
    /// `("pos", "1.0 8.0")`). The overlay never interprets them, so any app can
    /// surface its own state without widening this API.
    pub fn set_app_rows(&self, rows: &[(String, String)]) {
        self.state.borrow_mut().set_app_rows(rows);
        self.repaint();
    }

    /// Mount the overlay into `parent` and install the keyboard listeners.
    /// Idempotent — a second call while mounted does nothing.
    #[cfg(target_arch = "wasm32")]
    pub fn mount(&mut self, parent: &web_sys::Element) {
        self.binding.is_none().then(|| {
            let binding = crate::dom_binding::mount(&self.state, parent);
            binding.sync(&self.state.borrow());
            self.binding = Some(binding);
        });
    }

    /// Mount into `document.body`.
    #[cfg(target_arch = "wasm32")]
    pub fn mount_to_body(&mut self) {
        crate::dom_binding::body()
            .into_iter()
            .for_each(|body| self.mount(&body));
    }

    /// Remove the overlay nodes, the injected style, and the listeners.
    #[cfg(target_arch = "wasm32")]
    pub fn unmount(&mut self) {
        self.binding
            .take()
            .into_iter()
            .for_each(crate::dom_binding::Binding::unmount);
    }

    #[cfg(target_arch = "wasm32")]
    fn repaint(&self) {
        self.binding
            .as_ref()
            .into_iter()
            .for_each(|binding| binding.sync(&self.state.borrow()));
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn repaint(&self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_starts_hidden_unpinned_normal() {
        let api = DebugOverlayApi::new();
        assert!(!api.is_visible());
        assert!(!api.is_pinned());
        assert!(!api.is_console_focused());
        assert_eq!(api.density_label(), "normal");
        assert_eq!(api.command_history_count(), 0);
        assert_eq!(DebugOverlayApi::default().density_label(), "normal");
    }

    #[test]
    fn direct_ops_drive_visibility_pin_and_density() {
        let api = DebugOverlayApi::new();
        api.show();
        assert!(api.is_visible());
        api.hide();
        assert!(!api.is_visible());
        api.toggle();
        assert!(api.is_visible());
        api.cycle_density();
        assert_eq!(api.density_label(), "verbose");
        api.set_density("compact");
        assert_eq!(api.density_label(), "compact");
        api.set_density("nonsense");
        assert_eq!(api.density_label(), "compact");
        api.pin();
        assert!(api.is_pinned() && api.is_visible());
        api.toggle_pin();
        assert!(!api.is_pinned());
        api.unpin();
        assert!(!api.is_pinned());
    }

    #[test]
    fn focus_and_blur_console_and_header_status() {
        let api = DebugOverlayApi::new();
        api.focus_console();
        assert!(api.is_visible() && api.is_console_focused());
        assert_eq!(api.header_status(), "normal");
        api.pin();
        assert_eq!(api.header_status(), "normal • pinned");
        api.blur_console();
        assert!(!api.is_console_focused());
    }

    #[test]
    fn handle_backquote_classifies_applies_and_reports_handled() {
        let api = DebugOverlayApi::new();
        assert!(api.handle_backquote(false, false, false, false, false, false));
        assert!(api.is_visible());
        assert!(api.handle_backquote(true, false, false, false, false, false)); // shift -> density
        assert_eq!(api.density_label(), "verbose");
        assert!(api.handle_backquote(false, true, false, false, false, false)); // ctrl -> pin
        assert!(api.is_pinned());
        assert!(api.handle_backquote(false, false, true, false, false, false)); // alt -> console
        assert!(api.is_console_focused());
        // Not handled: a held meta key, and a focused non-console text field.
        assert!(!api.handle_backquote(false, false, false, true, false, false));
        assert!(!api.handle_backquote(false, false, false, false, true, false));
    }

    #[test]
    fn console_submit_routes_through_the_table() {
        let api = DebugOverlayApi::new();
        api.console_submit("overlay.compact");
        assert_eq!(api.density_label(), "compact");
        api.console_submit("help");
        assert_eq!(api.command_history_count(), 2);
        let rows = api.recent_results();
        assert!(rows.iter().any(|(ok, cmd, _)| *ok && cmd == "help"));
        // Empty input is ignored entirely.
        api.console_submit("   ");
        assert_eq!(api.command_history_count(), 2);
    }

    #[test]
    fn console_history_navigation_and_dismiss() {
        let api = DebugOverlayApi::new();
        api.console_submit("overlay.pin");
        api.console_submit("help");
        assert_eq!(api.console_history_prev(), Some("help".to_string()));
        assert_eq!(api.console_history_prev(), Some("overlay.pin".to_string()));
        assert_eq!(api.console_history_next(), Some("help".to_string()));
        assert_eq!(api.console_history_next(), Some(String::new()));
        assert_eq!(api.console_history_next(), None);
        api.focus_console();
        api.console_dismiss();
        assert!(!api.is_console_focused());
    }

    #[test]
    fn diagnostics_setters_are_reflected_in_the_rows() {
        let api = DebugOverlayApi::new();
        api.set_frame(120, 119, 2, 59_940, 16_680);
        api.set_backends(
            "webgl2",
            "axiom-windowing",
            "axiom-runtime",
            "none",
            "none",
            "none",
        );
        api.set_counters(7, 0, 1, 2);
        api.set_fallback(1, "webgpu device failed");
        api.set_visibility("visible");
        api.set_density("normal");

        let value = |label: &str| {
            api.visible_rows()
                .into_iter()
                .find(|(l, _)| l == label)
                .map(|(_, v)| v)
                .unwrap_or_default()
        };
        assert_eq!(value("frame"), "120");
        assert_eq!(value("fps"), "59.9");
        assert_eq!(value("frame ms"), "16.68");
        assert_eq!(value("renderer"), "webgl2");
        assert_eq!(value("fallbacks"), "1");
        assert_eq!(value("worker msgs"), "1 / 2");
        assert_eq!(value("visibility"), "visible");

        // backend.report reflects the pushed backends.
        api.console_submit("backend.report");
        assert!(api
            .recent_results()
            .iter()
            .any(|(_, _, msg)| msg.contains("renderer: webgl2")));
    }

    #[test]
    fn app_rows_surface_through_the_api_and_replace() {
        let api = DebugOverlayApi::new();
        api.set_density("normal");
        api.set_app_rows(&[
            ("pos".to_string(), "1.0 8.0".to_string()),
            ("look".to_string(), "0.00 0.00".to_string()),
        ]);
        let value = |label: &str| {
            api.visible_rows()
                .into_iter()
                .find(|(l, _)| l == label)
                .map(|(_, v)| v)
        };
        assert_eq!(value("pos"), Some("1.0 8.0".to_string()));
        assert_eq!(value("look"), Some("0.00 0.00".to_string()));
        // A later push replaces the rows wholesale.
        api.set_app_rows(&[("pos".to_string(), "2.0 7.0".to_string())]);
        assert_eq!(value("pos"), Some("2.0 7.0".to_string()));
        assert_eq!(value("look"), None);
    }
}
