//! [`InterfaceApi`] — the layer's single behavioral facade.
//!
//! It owns the [`InterfaceState`] (panels + focus) and exposes the whole
//! interface as primitive operations on [`PanelId`]s. Panels are identified by
//! the kernel's `HandleId` ([`Self::add_panel`] mints one), so the dependency on
//! `axiom-kernel` is genuine. Diagnostics/content cross in as primitives; the
//! layer emits a neutral [`crate::InterfaceDrawList`] and owns no renderer.

use axiom_kernel::HandleId;

use crate::draw_list::InterfaceDrawList;
use crate::input_event::{classify_console_key, ConsoleKey};
use crate::interface_command::CommandOutcome;
use crate::interface_state::{InterfaceState, RECENT_RESULTS};
use crate::panel::Panel;
use crate::panel_id::PanelId;

/// The deterministic, renderer-/platform-neutral interface surface.
#[derive(Debug)]
pub struct InterfaceApi {
    state: InterfaceState,
}

impl Default for InterfaceApi {
    fn default() -> Self {
        InterfaceApi::new()
    }
}

impl InterfaceApi {
    /// A fresh interface with no panels.
    pub fn new() -> Self {
        InterfaceApi {
            state: InterfaceState::new(),
        }
    }

    /// Add a panel, minting a fresh kernel-handle identity for it.
    pub fn add_panel(&mut self) -> PanelId {
        let id = PanelId::from_handle(HandleId::from_raw(self.state.next_raw()));
        self.state.insert_panel(id);
        id
    }

    // --- visibility ---------------------------------------------------------

    pub fn is_visible(&self, panel: PanelId) -> bool {
        self.state
            .panel(panel)
            .map(Panel::is_visible)
            .unwrap_or(false)
    }

    pub fn show(&mut self, panel: PanelId) {
        self.state
            .panel_mut(panel)
            .into_iter()
            .for_each(Panel::show);
    }

    pub fn hide(&mut self, panel: PanelId) {
        self.state
            .panel_mut(panel)
            .into_iter()
            .for_each(Panel::hide);
    }

    pub fn toggle(&mut self, panel: PanelId) {
        self.state
            .panel_mut(panel)
            .into_iter()
            .for_each(Panel::toggle);
    }

    // --- pin ----------------------------------------------------------------

    pub fn is_pinned(&self, panel: PanelId) -> bool {
        self.state
            .panel(panel)
            .map(Panel::is_pinned)
            .unwrap_or(false)
    }

    pub fn pin(&mut self, panel: PanelId) {
        self.state.panel_mut(panel).into_iter().for_each(Panel::pin);
    }

    pub fn unpin(&mut self, panel: PanelId) {
        self.state
            .panel_mut(panel)
            .into_iter()
            .for_each(Panel::unpin);
    }

    pub fn toggle_pin(&mut self, panel: PanelId) {
        self.state
            .panel_mut(panel)
            .into_iter()
            .for_each(Panel::toggle_pin);
    }

    // --- focus --------------------------------------------------------------

    /// Show the panel and give its console focus (transferring focus to it).
    pub fn focus_console(&mut self, panel: PanelId) {
        self.state
            .panel_mut(panel)
            .into_iter()
            .for_each(Panel::show);
        self.state.focus_mut().focus(panel);
    }

    pub fn blur_console(&mut self, panel: PanelId) {
        self.state.focus_mut().blur(panel);
    }

    pub fn is_console_focused(&self, panel: PanelId) -> bool {
        self.state.focus().is_focused(panel)
    }

    // --- layout / drag ------------------------------------------------------

    pub fn panel_position(&self, panel: PanelId) -> (i32, i32) {
        self.state
            .panel(panel)
            .map(Panel::position)
            .unwrap_or((0, 0))
    }

    pub fn is_dragging(&self, panel: PanelId) -> bool {
        self.state
            .panel(panel)
            .map(Panel::is_dragging)
            .unwrap_or(false)
    }

    pub fn drag_begin(&mut self, panel: PanelId, pointer_x: i32, pointer_y: i32) {
        self.state
            .panel_mut(panel)
            .into_iter()
            .for_each(|p| p.drag_begin(pointer_x, pointer_y));
    }

    pub fn drag_update(
        &mut self,
        panel: PanelId,
        pointer_x: i32,
        pointer_y: i32,
        max_x: i32,
        max_y: i32,
    ) {
        self.state
            .panel_mut(panel)
            .into_iter()
            .for_each(|p| p.drag_update(pointer_x, pointer_y, max_x, max_y));
    }

    pub fn drag_end(&mut self, panel: PanelId) {
        self.state
            .panel_mut(panel)
            .into_iter()
            .for_each(Panel::drag_end);
    }

    pub fn set_panel_width(&mut self, panel: PanelId, width: i32) {
        self.state
            .panel_mut(panel)
            .into_iter()
            .for_each(|p| p.set_width(width));
    }
}

impl InterfaceApi {
    // --- console model ------------------------------------------------------

    pub fn console_record(&mut self, panel: PanelId, command: &str) {
        self.state
            .panel_mut(panel)
            .into_iter()
            .for_each(|p| p.console_mut().record(command));
    }

    pub fn console_history_len(&self, panel: PanelId) -> usize {
        self.state
            .panel(panel)
            .map(|p| p.console().history_len())
            .unwrap_or(0)
    }

    /// The recent command history, oldest first (e.g. for a history preview).
    pub fn console_recent_history(&self, panel: PanelId, max: usize) -> Vec<String> {
        self.state
            .panel(panel)
            .map(|p| p.console().recent_history(max).to_vec())
            .unwrap_or_default()
    }

    pub fn console_recall_prev(&mut self, panel: PanelId) -> Option<String> {
        self.state
            .panel_mut(panel)
            .and_then(|p| p.console_mut().recall_prev())
    }

    pub fn console_recall_next(&mut self, panel: PanelId) -> Option<String> {
        self.state
            .panel_mut(panel)
            .and_then(|p| p.console_mut().recall_next())
    }

    /// Apply a console navigation/dismiss key (Escape / arrows). Returns the
    /// recalled command to place in the input, or `None`. `Enter` (submit) yields
    /// `None` — submitting is the consumer's job, since it owns the commands.
    pub fn console_navigate(&mut self, panel: PanelId, key: &str) -> Option<String> {
        classify_console_key(key).and_then(|console_key| self.apply_console_key(panel, console_key))
    }

    pub fn console_append_result(
        &mut self,
        panel: PanelId,
        ok: bool,
        command: &str,
        message: &str,
    ) {
        let outcome = CommandOutcome::new(ok, command, message);
        self.state
            .panel_mut(panel)
            .into_iter()
            .for_each(|p| p.console_mut().append_result(outcome.clone()));
    }

    pub fn console_clear_results(&mut self, panel: PanelId) {
        self.state
            .panel_mut(panel)
            .into_iter()
            .for_each(|p| p.console_mut().clear_results());
    }

    /// The recent console results: `(ok, command, message)`.
    pub fn console_recent_results(&self, panel: PanelId) -> Vec<(bool, String, String)> {
        self.state
            .panel(panel)
            .map(|p| {
                p.console()
                    .recent_results(RECENT_RESULTS)
                    .iter()
                    .map(|o| {
                        (
                            o.succeeded(),
                            o.command().to_string(),
                            o.message().to_string(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    // --- content ------------------------------------------------------------

    pub fn set_panel_header(&mut self, panel: PanelId, primary: &str, secondary: &str) {
        self.state
            .panel_mut(panel)
            .into_iter()
            .for_each(|p| p.set_header(primary, secondary));
    }

    pub fn set_panel_rows(&mut self, panel: PanelId, rows: &[(String, String)]) {
        self.state
            .panel_mut(panel)
            .into_iter()
            .for_each(|p| p.set_rows(rows));
    }

    pub fn panel_rows(&self, panel: PanelId) -> Vec<(String, String)> {
        self.state
            .panel(panel)
            .map(|p| p.rows().to_vec())
            .unwrap_or_default()
    }

    // --- draw ---------------------------------------------------------------

    pub fn draw_list(&self, panel: PanelId) -> InterfaceDrawList {
        self.state.draw_list(panel)
    }

    // --- clipboard ----------------------------------------------------------

    /// Queue `text` to be copied to the platform clipboard. Like the draw list,
    /// this is a renderer-/platform-neutral *output channel*: the layer only
    /// records the request as data and never touches a browser API, so any
    /// interface can ask for a copy. A platform host realizes it by draining
    /// [`Self::take_clipboard_requests`] and performing the actual copy.
    pub fn request_clipboard(&mut self, text: String) {
        self.state.request_clipboard(text);
    }

    /// Drain every pending clipboard request in order, clearing the queue. The
    /// platform host calls this and writes each string to the real clipboard
    /// (on a single-slot system clipboard, the last one wins).
    pub fn take_clipboard_requests(&mut self) -> Vec<String> {
        self.state.take_clipboard_requests()
    }

    // --- console-key dispatch (branchless table over the discriminant) ------

    fn apply_console_key(&mut self, panel: PanelId, key: ConsoleKey) -> Option<String> {
        const OPS: [fn(&mut InterfaceApi, PanelId) -> Option<String>; 4] = [
            InterfaceApi::console_key_submit,
            InterfaceApi::console_key_dismiss,
            InterfaceApi::console_recall_prev,
            InterfaceApi::console_recall_next,
        ];
        OPS[key as usize](self, panel)
    }

    fn console_key_submit(&mut self, _panel: PanelId) -> Option<String> {
        None
    }

    fn console_key_dismiss(&mut self, panel: PanelId) -> Option<String> {
        self.blur_console(panel);
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one_panel() -> (InterfaceApi, PanelId) {
        let mut api = InterfaceApi::new();
        let panel = api.add_panel();
        (api, panel)
    }

    fn missing() -> PanelId {
        PanelId::from_handle(HandleId::from_raw(9999))
    }

    #[test]
    fn add_panel_mints_a_valid_distinct_handle() {
        let mut api = InterfaceApi::new();
        let a = api.add_panel();
        let b = api.add_panel();
        assert!(a.handle().is_valid() && b.handle().is_valid());
        assert_ne!(a, b);
        assert_eq!(a.raw(), 1);
        assert_eq!(b.raw(), 2);
    }

    #[test]
    fn visibility_and_pin_are_deterministic() {
        let (mut api, p) = one_panel();
        assert!(!api.is_visible(p));
        api.show(p);
        assert!(api.is_visible(p));
        api.toggle(p);
        assert!(!api.is_visible(p));
        api.toggle(p);
        assert!(api.is_visible(p));
        api.pin(p);
        assert!(api.is_pinned(p));
        api.toggle(p); // pinned protects
        assert!(api.is_visible(p));
        api.unpin(p);
        api.hide(p);
        api.toggle_pin(p); // unpinned + hidden -> pins and shows
        assert!(api.is_pinned(p) && api.is_visible(p));
        api.toggle_pin(p); // pinned -> unpins, keeps visibility
        assert!(!api.is_pinned(p) && api.is_visible(p));
    }

    #[test]
    fn focus_transfers_and_blurs() {
        let mut api = InterfaceApi::new();
        let (a, b) = (api.add_panel(), api.add_panel());
        api.focus_console(a);
        assert!(api.is_console_focused(a) && api.is_visible(a));
        api.focus_console(b);
        assert!(api.is_console_focused(b) && !api.is_console_focused(a));
        api.blur_console(b);
        assert!(!api.is_console_focused(b));
    }

    #[test]
    fn layout_and_drag_are_deterministic_and_clamped() {
        let (mut api, p) = one_panel();
        assert_eq!(api.panel_position(p), (8, 8));
        assert!(!api.is_dragging(p));
        api.drag_begin(p, 20, 18);
        assert!(api.is_dragging(p));
        api.drag_update(p, 120, 118, 1000, 800);
        assert_eq!(api.panel_position(p), (108, 108));
        api.drag_update(p, 5000, 5000, 1000, 800);
        assert_eq!(api.panel_position(p), (1000, 800));
        api.drag_end(p);
        assert!(!api.is_dragging(p));
        api.set_panel_width(p, 460);
    }

    #[test]
    fn console_model_records_navigates_and_logs() {
        let (mut api, p) = one_panel();
        api.console_record(p, "first");
        api.console_record(p, "second");
        assert_eq!(api.console_history_len(p), 2);
        assert_eq!(api.console_recent_history(p, 1), vec!["second".to_string()]);
        assert_eq!(api.console_recent_history(p, 9).len(), 2);
        assert_eq!(api.console_recall_prev(p), Some("second".to_string()));
        assert_eq!(api.console_recall_prev(p), Some("first".to_string()));
        assert_eq!(api.console_recall_next(p), Some("second".to_string()));
        api.console_append_result(p, true, "help", "ok");
        api.console_append_result(p, false, "nope", "unknown");
        let results = api.console_recent_results(p);
        assert_eq!(results.len(), 2);
        assert!(results[0].0 && !results[1].0);
        api.console_clear_results(p);
        assert!(api.console_recent_results(p).is_empty());
    }

    #[test]
    fn clipboard_requests_queue_in_order_and_drain_once() {
        let mut api = InterfaceApi::new();
        // Nothing queued yet.
        assert!(api.take_clipboard_requests().is_empty());
        api.request_clipboard("alpha".to_string());
        api.request_clipboard("beta".to_string());
        // Drained in request order...
        assert_eq!(
            api.take_clipboard_requests(),
            vec!["alpha".to_string(), "beta".to_string()]
        );
        // ...and draining clears the queue.
        assert!(api.take_clipboard_requests().is_empty());
    }

    #[test]
    fn console_navigate_maps_every_key() {
        let (mut api, p) = one_panel();
        api.console_record(p, "alpha");
        // Submit yields None (the consumer submits).
        assert_eq!(api.console_navigate(p, "Enter"), None);
        // ArrowUp recalls; ArrowDown returns toward the live line.
        assert_eq!(
            api.console_navigate(p, "ArrowUp"),
            Some("alpha".to_string())
        );
        assert_eq!(api.console_navigate(p, "ArrowDown"), Some(String::new()));
        // Ordinary typing is not a console key.
        assert_eq!(api.console_navigate(p, "x"), None);
        // Escape dismisses (blurs) and returns None.
        api.focus_console(p);
        assert_eq!(api.console_navigate(p, "Escape"), None);
        assert!(!api.is_console_focused(p));
    }

    #[test]
    fn content_and_draw_list_are_ordered() {
        let (mut api, p) = one_panel();
        api.show(p);
        api.set_panel_header(p, "Title", "status");
        api.set_panel_rows(
            p,
            &[
                ("a".to_string(), "1".to_string()),
                ("b".to_string(), "2".to_string()),
            ],
        );
        api.console_append_result(p, true, "help", "ok");
        api.focus_console(p);
        assert_eq!(api.panel_rows(p).len(), 2);

        let list = api.draw_list(p);
        let items = list.items();
        use crate::draw_list::InterfaceDrawItem;
        // Panel, Header, 2 Rows, 1 ConsoleLine, ConsoleInput — in that fixed order.
        assert_eq!(items.len(), 6);
        assert_eq!(
            items[0],
            InterfaceDrawItem::Panel {
                x: 8,
                y: 8,
                width: 360,
                height: 0
            }
        );
        assert_eq!(
            items[1],
            InterfaceDrawItem::Header {
                primary: "Title".to_string(),
                secondary: "status".to_string()
            }
        );
        assert_eq!(
            items[2],
            InterfaceDrawItem::Row {
                label: "a".to_string(),
                value: "1".to_string()
            }
        );
        assert_eq!(
            items[4],
            InterfaceDrawItem::ConsoleLine {
                ok: true,
                text: "help: ok".to_string()
            }
        );
        assert_eq!(
            items[5],
            InterfaceDrawItem::ConsoleInput {
                prompt: ">".to_string(),
                focused: true
            }
        );
    }

    #[test]
    fn hidden_or_unknown_panel_draws_nothing() {
        let (mut api, p) = one_panel();
        // Panel exists but is hidden → empty.
        assert!(api.draw_list(p).is_empty());
        // Unknown panel → empty, and queries return defaults.
        assert!(api.draw_list(missing()).is_empty());
        assert!(!api.is_visible(missing()));
        assert!(!api.is_pinned(missing()));
        assert!(!api.is_dragging(missing()));
        assert_eq!(api.panel_position(missing()), (0, 0));
        assert_eq!(api.console_history_len(missing()), 0);
        assert!(api.console_recent_history(missing(), 4).is_empty());
        assert!(api.panel_rows(missing()).is_empty());
        assert!(api.console_recent_results(missing()).is_empty());
        assert_eq!(api.console_recall_prev(missing()), None);
        assert_eq!(api.console_recall_next(missing()), None);
    }

    #[test]
    fn mutating_an_unknown_panel_is_a_safe_noop() {
        let mut api = InterfaceApi::new();
        let ghost = missing();
        // None of these find a panel; all must be no-ops, not panics.
        api.show(ghost);
        api.hide(ghost);
        api.toggle(ghost);
        api.pin(ghost);
        api.unpin(ghost);
        api.toggle_pin(ghost);
        api.drag_begin(ghost, 1, 1);
        api.drag_update(ghost, 2, 2, 10, 10);
        api.drag_end(ghost);
        api.set_panel_width(ghost, 100);
        api.console_record(ghost, "x");
        api.console_append_result(ghost, true, "c", "m");
        api.console_clear_results(ghost);
        api.set_panel_header(ghost, "a", "b");
        api.set_panel_rows(ghost, &[]);
        assert!(api.draw_list(ghost).is_empty());
    }

    #[test]
    fn default_matches_new() {
        assert!(InterfaceApi::default().draw_list(missing()).is_empty());
    }
}
