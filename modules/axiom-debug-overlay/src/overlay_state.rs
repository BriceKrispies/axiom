//! The overlay's debug model — density, the diagnostics read-out, and the
//! debug-specific command coordination — composed on top of an
//! [`axiom_interface::InterfaceApi`] panel that owns all the *generic* windowing:
//! visibility, pin, focus, drag/layout, the console model, and the neutral draw
//! list. This file holds only what is debug-specific; the panel does the rest.
//! Branchless throughout.

use axiom_interface::{CommandTable, InterfaceApi, InterfaceDrawList, PanelId, ParsedCommand};

use crate::backquote::OverlayShortcut;
use crate::diagnostics::Diagnostics;
use crate::overlay_commands::OVERLAY_SPECS;
use crate::overlay_density::OverlayDensity;

/// One density's row builder: renders the overlay's `(label, value)` rows.
type RowBuilder = fn(&OverlayState) -> Vec<(String, String)>;

/// The overlay panel's header title.
const OVERLAY_TITLE: &str = "Axiom Debug Overlay";
/// How many recent commands the verbose history preview shows.
const HISTORY_PREVIEW: usize = 4;
/// Panel widths per density (`compact`/`normal` share one; `verbose` is wider).
const DENSITY_WIDTH: [i32; 3] = [360, 360, 460];

/// The overlay's complete display state: one interface panel plus the
/// debug-specific density and diagnostics.
#[derive(Debug)]
pub(crate) struct OverlayState {
    interface: InterfaceApi,
    panel: PanelId,
    density: OverlayDensity,
    diagnostics: Diagnostics,
    /// App-pushed read-out rows (label, value), shown below the engine diagnostics
    /// in normal/verbose. The overlay stays generic — it never interprets these;
    /// a game (e.g. retro FPS) formats and pushes its own state, like the player pose.
    app_rows: Vec<(String, String)>,
}

impl OverlayState {
    pub(crate) fn new() -> Self {
        let mut interface = InterfaceApi::new();
        let panel = interface.add_panel();
        let mut state = OverlayState {
            interface,
            panel,
            density: OverlayDensity::default(),
            diagnostics: Diagnostics::placeholder(),
            app_rows: Vec::new(),
        };
        state.refresh_panel();
        state
    }

    // --- visibility (delegated to the panel) --------------------------------

    pub(crate) fn is_visible(&self) -> bool {
        self.interface.is_visible(self.panel)
    }

    pub(crate) fn show(&mut self) {
        self.interface.show(self.panel);
        self.refresh_panel();
    }

    pub(crate) fn hide(&mut self) {
        self.interface.hide(self.panel);
        self.refresh_panel();
    }

    pub(crate) fn toggle(&mut self) {
        self.interface.toggle(self.panel);
        self.refresh_panel();
    }

    // --- pin (delegated) ----------------------------------------------------

    pub(crate) fn is_pinned(&self) -> bool {
        self.interface.is_pinned(self.panel)
    }

    pub(crate) fn pin(&mut self) {
        self.interface.pin(self.panel);
        self.refresh_panel();
    }

    pub(crate) fn unpin(&mut self) {
        self.interface.unpin(self.panel);
        self.refresh_panel();
    }

    pub(crate) fn toggle_pin(&mut self) {
        self.interface.toggle_pin(self.panel);
        self.refresh_panel();
    }

    // --- density (debug-specific) -------------------------------------------

    pub(crate) fn density_label(&self) -> &'static str {
        self.density.label()
    }

    pub(crate) fn set_density(&mut self, density: OverlayDensity) {
        self.density = density;
        self.refresh_panel();
    }

    /// Set density from a label; an unknown label leaves it unchanged.
    pub(crate) fn set_density_label(&mut self, label: &str) {
        OverlayDensity::from_label(label)
            .into_iter()
            .for_each(|density| self.density = density);
        self.refresh_panel();
    }

    pub(crate) fn cycle_density(&mut self) {
        self.density = self.density.cycle();
        self.refresh_panel();
    }

    /// Apply a classified Backquote shortcut. Branchless: the fieldless
    /// [`OverlayShortcut`] discriminant indexes a `const` table of state ops (each
    /// of which refreshes the panel), so the facade and the wasm keydown listener
    /// share one dispatch.
    pub(crate) fn apply_shortcut(&mut self, shortcut: OverlayShortcut) {
        const OPS: [fn(&mut OverlayState); 4] = [
            OverlayState::toggle,
            OverlayState::cycle_density,
            OverlayState::toggle_pin,
            OverlayState::focus_console,
        ];
        OPS[shortcut as usize](self);
    }
}

impl OverlayState {
    // --- console (delegated to the panel) -----------------------------------

    pub(crate) fn focus_console(&mut self) {
        self.interface.focus_console(self.panel);
        self.refresh_panel();
    }

    pub(crate) fn blur_console(&mut self) {
        self.interface.blur_console(self.panel);
        self.refresh_panel();
    }

    pub(crate) fn is_console_focused(&self) -> bool {
        self.interface.is_console_focused(self.panel)
    }

    pub(crate) fn command_history_count(&self) -> usize {
        self.interface.console_history_len(self.panel)
    }

    pub(crate) fn history_prev(&mut self) -> Option<String> {
        self.interface.console_recall_prev(self.panel)
    }

    pub(crate) fn history_next(&mut self) -> Option<String> {
        self.interface.console_recall_next(self.panel)
    }

    /// Recent console results: `(ok, command, message)`.
    pub(crate) fn recent_results(&self) -> Vec<(bool, String, String)> {
        self.interface.console_recent_results(self.panel)
    }

    pub(crate) fn clear_console_results(&mut self) {
        self.interface.console_clear_results(self.panel);
    }

    /// Submit a raw console line: parse → record → dispatch through the overlay's
    /// command table → echo the result. An empty line is ignored entirely.
    pub(crate) fn submit_command(&mut self, raw: &str) {
        ParsedCommand::parse(raw)
            .into_iter()
            .for_each(|parsed| self.run_command(&parsed));
    }

    fn run_command(&mut self, parsed: &ParsedCommand) {
        self.interface
            .console_record(self.panel, &parsed.canonical());
        let outcome = CommandTable::new(OVERLAY_SPECS).dispatch(self, parsed);
        // A silent success (empty message, e.g. `clear`) leaves no echo line, so a
        // side-effect-only command doesn't clutter the log; errors always show.
        let echo = !outcome.succeeded() | !outcome.message().is_empty();
        echo.then(|| {
            self.interface.console_append_result(
                self.panel,
                outcome.succeeded(),
                outcome.command(),
                outcome.message(),
            )
        });
        self.refresh_panel();
    }

    // --- diagnostics in (primitives only) -----------------------------------

    pub(crate) fn diagnostics(&self) -> &Diagnostics {
        &self.diagnostics
    }

    pub(crate) fn set_frame(
        &mut self,
        frame_index: u64,
        tick: u64,
        sim_ticks: u32,
        fps_milli: u32,
        frame_time_micros: u32,
    ) {
        self.diagnostics.frame_index = frame_index;
        self.diagnostics.tick = tick;
        self.diagnostics.sim_ticks_this_frame = sim_ticks;
        self.diagnostics.fps_milli = fps_milli;
        self.diagnostics.frame_time_micros = frame_time_micros;
        self.refresh_panel();
    }

    pub(crate) fn set_backends(
        &mut self,
        renderer: &str,
        canvas_owner: &str,
        sim_owner: &str,
        storage: &str,
        audio: &str,
        network: &str,
    ) {
        self.diagnostics.renderer_backend = renderer.to_string();
        self.diagnostics.canvas_owner = canvas_owner.to_string();
        self.diagnostics.simulation_owner = sim_owner.to_string();
        self.diagnostics.storage_backend = storage.to_string();
        self.diagnostics.audio_backend = audio.to_string();
        self.diagnostics.network_backend = network.to_string();
        self.refresh_panel();
    }

    pub(crate) fn set_counters(
        &mut self,
        webgpu_submissions: u64,
        canvas2d_frames: u64,
        worker_in: u64,
        worker_out: u64,
    ) {
        self.diagnostics.webgpu_submissions = webgpu_submissions;
        self.diagnostics.canvas2d_frames = canvas2d_frames;
        self.diagnostics.worker_messages_in = worker_in;
        self.diagnostics.worker_messages_out = worker_out;
        self.refresh_panel();
    }

    pub(crate) fn set_fallback(&mut self, count: u32, reason: &str) {
        self.diagnostics.fallback_count = count;
        self.diagnostics.fallback_reason = reason.to_string();
        self.refresh_panel();
    }

    pub(crate) fn set_visibility(&mut self, visibility_state: &str) {
        self.diagnostics.visibility_state = visibility_state.to_string();
        self.refresh_panel();
    }

    /// Replace the app-pushed read-out rows (see [`Self::app_rows`]). An app calls
    /// this each frame with its own `(label, value)` pairs already formatted to
    /// strings, so no float crosses the boundary and the overlay stays generic.
    pub(crate) fn set_app_rows(&mut self, rows: &[(String, String)]) {
        self.app_rows = rows.to_vec();
        self.refresh_panel();
    }
}

// The presentation surface the wasm `dom_binding` arm reads: drag plumbing driven
// by the header pointer handlers, and the neutral draw list it renders. The native
// lib build has no caller other than the tests below, so this block is
// `allow(dead_code)` off-wasm — exactly as the windowing live arm is structured.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
impl OverlayState {
    pub(crate) fn position(&self) -> (i32, i32) {
        self.interface.panel_position(self.panel)
    }

    pub(crate) fn is_dragging(&self) -> bool {
        self.interface.is_dragging(self.panel)
    }

    pub(crate) fn drag_begin(&mut self, pointer_x: i32, pointer_y: i32) {
        self.interface.drag_begin(self.panel, pointer_x, pointer_y);
    }

    pub(crate) fn drag_update(&mut self, pointer_x: i32, pointer_y: i32, max_x: i32, max_y: i32) {
        self.interface
            .drag_update(self.panel, pointer_x, pointer_y, max_x, max_y);
    }

    pub(crate) fn drag_end(&mut self) {
        self.interface.drag_end(self.panel);
    }

    /// The neutral draw list for the panel (empty when hidden).
    pub(crate) fn draw_list(&self) -> InterfaceDrawList {
        self.interface.draw_list(self.panel)
    }
}

/// The view model — how the current state maps to a neutral header + rows. A
/// distinct concern, in its own `impl` block (which also keeps each block within
/// the engine size budget). Every method here is used on native (by the facade
/// and `refresh_panel`), so it carries no dead-code allowance.
impl OverlayState {
    /// The header's right-hand status: density, plus a pin marker when pinned.
    pub(crate) fn header_status(&self) -> String {
        let pinned = self
            .is_pinned()
            .then(|| format!("{} • pinned", self.density.label()));
        pinned.unwrap_or_else(|| self.density.label().to_string())
    }

    /// Push the current header + rows + density width into the panel, so the
    /// neutral draw list reflects the latest debug state.
    fn refresh_panel(&mut self) {
        let status = self.header_status();
        let rows = self.rows();
        let width = DENSITY_WIDTH[self.density as usize];
        self.interface
            .set_panel_header(self.panel, OVERLAY_TITLE, &status);
        self.interface.set_panel_rows(self.panel, &rows);
        self.interface.set_panel_width(self.panel, width);
    }

    /// The ordered rows to render at the current density. Branchless: the
    /// fieldless density discriminant indexes a `const` table of row builders.
    pub(crate) fn rows(&self) -> Vec<(String, String)> {
        const BUILDERS: [RowBuilder; 3] = [
            OverlayState::compact_rows,
            OverlayState::normal_rows,
            OverlayState::verbose_rows,
        ];
        BUILDERS[self.density as usize](self)
    }

    fn compact_rows(&self) -> Vec<(String, String)> {
        self.diagnostics.compact_rows()
    }

    fn normal_rows(&self) -> Vec<(String, String)> {
        let mut rows = self.diagnostics.core_rows();
        rows.push((
            "cmd history".to_string(),
            self.command_history_count().to_string(),
        ));
        rows.extend(self.app_rows.iter().cloned());
        rows
    }

    fn verbose_rows(&self) -> Vec<(String, String)> {
        let mut rows = self.normal_rows();
        rows.push((
            "backend select".to_string(),
            self.diagnostics.backend_select_text(),
        ));
        rows.push(("overlay state".to_string(), self.debug_state_text()));
        rows.push(("history".to_string(), self.history_preview()));
        rows
    }

    fn debug_state_text(&self) -> String {
        format!(
            "density={} pinned={} visible={} console={}",
            self.density.label(),
            self.is_pinned(),
            self.is_visible(),
            self.is_console_focused(),
        )
    }

    fn history_preview(&self) -> String {
        let preview = self
            .interface
            .console_recent_history(self.panel, HISTORY_PREVIEW);
        let none = preview.is_empty().then(|| "(none)".to_string());
        none.unwrap_or_else(|| preview.join(" | "))
    }
}

/// Clipboard plumbing — delegated to the interface layer's neutral outbox.
impl OverlayState {
    /// Queue `text` for the platform clipboard. The pure core only records the
    /// request as data; a platform host drains and performs the real copy.
    pub(crate) fn request_clipboard(&mut self, text: String) {
        self.interface.request_clipboard(text);
    }

    /// Drain the pending clipboard requests. Only the wasm DOM arm needs this
    /// (it calls it inside the console keydown, then writes each to
    /// `navigator.clipboard`); the native build performs no clipboard copy, so
    /// this is `wasm32`-only — keeping it out of the native impl surface and the
    /// coverage gate. The reusable, native-tested drain lives on
    /// [`axiom_interface::InterfaceApi`].
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn take_clipboard_requests(&mut self) -> Vec<String> {
        self.interface.take_clipboard_requests()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(state: &OverlayState) -> Vec<String> {
        state.rows().into_iter().map(|(label, _)| label).collect()
    }

    #[test]
    fn starts_hidden_unpinned_normal() {
        let s = OverlayState::new();
        assert!(!s.is_visible());
        assert!(!s.is_pinned());
        assert!(!s.is_console_focused());
        assert_eq!(s.density_label(), "normal");
        assert_eq!(s.command_history_count(), 0);
        assert_eq!(s.position(), (8, 8));
        assert!(!s.is_dragging());
    }

    #[test]
    fn toggle_covers_all_three_cases() {
        let mut s = OverlayState::new();
        s.toggle(); // hidden -> show
        assert!(s.is_visible());
        s.toggle(); // visible + unpinned -> hide
        assert!(!s.is_visible());
        s.pin();
        s.toggle(); // visible + pinned -> stay
        assert!(s.is_visible());
    }

    #[test]
    fn show_and_hide_are_explicit() {
        let mut s = OverlayState::new();
        s.show();
        assert!(s.is_visible());
        s.pin();
        s.hide(); // explicit hide beats the pin
        assert!(!s.is_visible());
    }

    #[test]
    fn pin_unpin_and_toggle_pin() {
        let mut s = OverlayState::new();
        s.toggle_pin(); // unpinned + hidden -> pins + shows
        assert!(s.is_pinned() && s.is_visible());
        s.toggle_pin(); // pinned -> unpins, stays visible
        assert!(!s.is_pinned() && s.is_visible());
        s.unpin(); // idempotent
        assert!(!s.is_pinned());
    }

    #[test]
    fn density_set_cycle_and_label() {
        let mut s = OverlayState::new();
        s.cycle_density();
        assert_eq!(s.density_label(), "verbose");
        s.set_density(OverlayDensity::Compact);
        assert_eq!(s.density_label(), "compact");
        s.set_density_label("verbose");
        assert_eq!(s.density_label(), "verbose");
        s.set_density_label("bogus"); // unchanged
        assert_eq!(s.density_label(), "verbose");
    }

    #[test]
    fn focus_and_blur_console() {
        let mut s = OverlayState::new();
        s.focus_console();
        assert!(s.is_visible() && s.is_console_focused());
        s.blur_console();
        assert!(!s.is_console_focused());
        assert!(s.is_visible());
    }

    #[test]
    fn history_navigation_delegates_to_the_console() {
        let mut s = OverlayState::new();
        s.submit_command("help");
        assert_eq!(s.history_prev(), Some("help".to_string()));
        assert_eq!(s.history_next(), Some(String::new()));
    }

    #[test]
    fn compact_density_shows_four_rows() {
        let mut s = OverlayState::new();
        s.set_density(OverlayDensity::Compact);
        assert_eq!(labels(&s), vec!["fps", "frame ms", "renderer", "fallbacks"]);
    }

    #[test]
    fn normal_density_ends_with_command_history() {
        let s = OverlayState::new();
        let rows = s.rows();
        assert_eq!(rows.last().unwrap().0, "cmd history");
        assert_eq!(rows.last().unwrap().1, "0");
    }

    #[test]
    fn verbose_density_appends_backend_state_and_history() {
        let mut s = OverlayState::new();
        s.set_density(OverlayDensity::Verbose);
        let ls = labels(&s);
        assert!(ls.contains(&"backend select".to_string()));
        assert!(ls.contains(&"overlay state".to_string()));
        assert!(ls.contains(&"history".to_string()));
        assert!(ls.contains(&"cmd history".to_string()));
    }

    #[test]
    fn verbose_history_preview_reads_none_then_recent_commands() {
        let mut s = OverlayState::new();
        s.set_density(OverlayDensity::Verbose);
        let history_value = |s: &OverlayState| {
            s.rows()
                .into_iter()
                .find(|(label, _)| label == "history")
                .unwrap()
                .1
        };
        assert_eq!(history_value(&s), "(none)");
        s.submit_command("help");
        s.submit_command("clear");
        assert_eq!(history_value(&s), "help | clear");
    }

    #[test]
    fn diagnostics_setters_are_reflected_in_rows() {
        let mut s = OverlayState::new();
        s.set_density(OverlayDensity::Compact);
        s.set_frame(7, 6, 1, 30_000, 16_680);
        let fps = s
            .rows()
            .into_iter()
            .find(|(label, _)| label == "fps")
            .unwrap()
            .1;
        assert_eq!(fps, "30.0");
    }

    #[test]
    fn app_rows_render_below_diagnostics_in_normal_but_not_compact() {
        let mut s = OverlayState::new();
        s.set_app_rows(&[
            ("pos".to_string(), "1.0 8.0".to_string()),
            ("look".to_string(), "0.00 0.00".to_string()),
        ]);
        // Normal density shows them, after the engine diagnostics.
        let rows = s.rows();
        assert!(rows.iter().any(|(l, v)| l == "pos" && v == "1.0 8.0"));
        assert!(rows.iter().any(|(l, v)| l == "look" && v == "0.00 0.00"));
        // Compact stays the minimal four rows (no app rows).
        s.set_density(OverlayDensity::Compact);
        assert_eq!(labels(&s).len(), 4);
        // Replacing them swaps the read-out.
        s.set_density(OverlayDensity::Normal);
        s.set_app_rows(&[("pos".to_string(), "2.0 3.0".to_string())]);
        assert!(s.rows().iter().any(|(l, v)| l == "pos" && v == "2.0 3.0"));
        assert!(!s.rows().iter().any(|(l, _)| l == "look"));
    }

    #[test]
    fn header_status_marks_the_pin() {
        let mut s = OverlayState::new();
        assert_eq!(s.header_status(), "normal");
        s.pin();
        assert_eq!(s.header_status(), "normal • pinned");
    }

    #[test]
    fn apply_shortcut_dispatches_each_arm() {
        let mut s = OverlayState::new();
        s.apply_shortcut(OverlayShortcut::ToggleOverlay);
        assert!(s.is_visible());
        s.apply_shortcut(OverlayShortcut::CycleDensity);
        assert_eq!(s.density_label(), "verbose");
        s.apply_shortcut(OverlayShortcut::TogglePinned);
        assert!(s.is_pinned());
        s.apply_shortcut(OverlayShortcut::FocusConsole);
        assert!(s.is_console_focused());
    }

    #[test]
    fn drag_moves_and_clamps_through_the_panel() {
        let mut s = OverlayState::new();
        assert_eq!(s.position(), (8, 8));
        s.drag_begin(20, 18);
        assert!(s.is_dragging());
        s.drag_update(120, 118, 1000, 800);
        assert_eq!(s.position(), (108, 108));
        s.drag_update(5000, 5000, 1000, 800);
        assert_eq!(s.position(), (1000, 800));
        s.drag_end();
        assert!(!s.is_dragging());
    }

    #[test]
    fn submit_runs_commands_records_and_ignores_empty() {
        let mut s = OverlayState::new();
        s.submit_command("overlay.compact");
        assert_eq!(s.density_label(), "compact");
        s.submit_command("help");
        assert_eq!(s.command_history_count(), 2);
        assert!(s
            .recent_results()
            .iter()
            .any(|(ok, cmd, _)| *ok && cmd == "help"));
        s.submit_command("   "); // ignored entirely
        assert_eq!(s.command_history_count(), 2);
    }

    #[test]
    fn submit_unknown_command_logs_an_error() {
        let mut s = OverlayState::new();
        s.submit_command("bogus");
        assert_eq!(s.command_history_count(), 1);
        let results = s.recent_results();
        assert!(results
            .iter()
            .any(|(ok, cmd, msg)| !ok && cmd == "bogus" && msg.contains("unknown")));
    }

    #[test]
    fn clear_command_empties_the_result_log() {
        let mut s = OverlayState::new();
        s.submit_command("help");
        assert!(!s.recent_results().is_empty());
        s.submit_command("clear");
        assert!(s.recent_results().is_empty());
    }

    #[test]
    fn diagnostics_snapshot_and_backend_report_echo_state() {
        let mut s = OverlayState::new();
        s.set_backends(
            "webgl2",
            "axiom-windowing",
            "axiom-runtime",
            "none",
            "none",
            "none",
        );
        assert!(s
            .diagnostics()
            .backend_report_text()
            .contains("renderer: webgl2"));
        s.submit_command("diagnostics.snapshot");
        s.submit_command("backend.report");
        let results = s.recent_results();
        assert!(results
            .iter()
            .any(|(_, cmd, _)| cmd == "diagnostics.snapshot"));
        assert!(results
            .iter()
            .any(|(_, _, msg)| msg.contains("renderer: webgl2")));
    }

    #[test]
    fn copy_command_copies_args_or_falls_back_to_the_snapshot() {
        let mut s = OverlayState::new();
        // `copy <text…>` copies the literal joined arguments ("hello world" = 11
        // chars); the outcome reports the count, distinguishing it from the
        // snapshot arm.
        s.submit_command("copy hello world");
        assert!(s.recent_results().iter().any(|(ok, cmd, msg)| *ok
            && cmd == "copy"
            && msg == "copied 11 chars to the clipboard"));
        // Bare `copy` falls back to the (longer, multi-line) diagnostics snapshot.
        let snapshot_len = s.diagnostics().snapshot_text().chars().count();
        let expected = format!("copied {snapshot_len} chars to the clipboard");
        s.submit_command("copy");
        assert!(s
            .recent_results()
            .iter()
            .any(|(ok, cmd, msg)| *ok && cmd == "copy" && *msg == expected));
    }

    #[test]
    fn pin_unpin_hide_commands_drive_the_panel() {
        let mut s = OverlayState::new();
        s.submit_command("overlay.pin");
        assert!(s.is_pinned() && s.is_visible());
        s.submit_command("overlay.unpin");
        assert!(!s.is_pinned());
        s.submit_command("overlay.hide");
        assert!(!s.is_visible());
        s.submit_command("overlay.normal");
        assert_eq!(s.density_label(), "normal");
        s.submit_command("overlay.verbose");
        assert_eq!(s.density_label(), "verbose");
    }

    #[test]
    fn stub_markers_acknowledge() {
        let mut s = OverlayState::new();
        s.submit_command("replay.mark");
        s.submit_command("perf.mark");
        let results = s.recent_results();
        assert!(results.iter().all(|(ok, _, _)| *ok));
        assert!(results.iter().any(|(_, cmd, _)| cmd == "replay.mark"));
        assert!(results.iter().any(|(_, cmd, _)| cmd == "perf.mark"));
    }

    #[test]
    fn recent_results_caps_at_the_window() {
        let mut s = OverlayState::new();
        (0..8).for_each(|i| s.submit_command(&format!("help {i}")));
        assert_eq!(s.recent_results().len(), 5);
    }

    #[test]
    fn draw_list_is_built_from_the_panel() {
        let mut s = OverlayState::new();
        assert!(s.draw_list().is_empty()); // hidden
        s.set_density(OverlayDensity::Compact);
        s.show();
        s.focus_console();
        let list = s.draw_list();
        let items = list.items();
        use axiom_interface::InterfaceDrawItem;
        // Panel, Header, 4 compact rows, the focused input marker (no results yet).
        assert_eq!(items.len(), 7);
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
                primary: OVERLAY_TITLE.to_string(),
                secondary: "compact".to_string(),
            }
        );
        assert_eq!(
            items[6],
            InterfaceDrawItem::ConsoleInput {
                prompt: ">".to_string(),
                focused: true
            }
        );
    }

    #[test]
    fn verbose_draw_list_widens_the_panel() {
        let mut s = OverlayState::new();
        s.set_density(OverlayDensity::Verbose);
        s.show();
        let list = s.draw_list();
        assert_eq!(
            list.items()[0],
            axiom_interface::InterfaceDrawItem::Panel {
                x: 8,
                y: 8,
                width: 460,
                height: 0
            }
        );
    }
}
