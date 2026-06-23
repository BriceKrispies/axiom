//! The overlay's pure model: visibility, pin, density, the console, and the last
//! diagnostics snapshot — plus the per-density list of rows to render.
//!
//! This is the single source of truth the DOM controller projects. Every
//! decision (what toggling does, whether pinning protects against an accidental
//! close, which rows a density shows) lives here and is unit-tested without a
//! browser. The controller just calls these methods and paints the result.

use crate::browser_diagnostics::{BrowserDiagnosticsSnapshot, DiagnosticRow};
use crate::debug_command::CommandResult;
use crate::debug_console::ConsoleState;
use crate::debug_overlay_density::OverlayDensity;

/// How many result lines the overlay keeps on screen above the input.
const RECENT_RESULTS: usize = 5;
/// How many recent commands the verbose history preview shows.
const HISTORY_PREVIEW: usize = 4;

/// The overlay's complete display state.
#[derive(Debug, Clone)]
pub struct OverlayState {
    visible: bool,
    pinned: bool,
    density: OverlayDensity,
    console: ConsoleState,
    diagnostics: BrowserDiagnosticsSnapshot,
}

impl Default for OverlayState {
    fn default() -> Self {
        OverlayState {
            // Hidden by default — the overlay only appears once toggled on.
            visible: false,
            pinned: false,
            density: OverlayDensity::default(),
            console: ConsoleState::new(),
            diagnostics: BrowserDiagnosticsSnapshot::stub(),
        }
    }
}

impl OverlayState {
    pub fn new() -> Self {
        OverlayState::default()
    }

    // --- visibility ---------------------------------------------------------

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Show the overlay.
    pub fn show(&mut self) {
        self.visible = true;
    }

    /// Hide the overlay (explicit; ignores the pin — `overlay.hide` and the
    /// programmatic API can always close it).
    pub fn hide(&mut self) {
        self.visible = false;
    }

    /// The Backquote toggle. Showing always works; hiding is suppressed while the
    /// overlay is pinned, so a pinned overlay can't be dismissed by an accidental
    /// Backquote — unpin (Ctrl+Backquote) or run `overlay.hide` to close it.
    pub fn toggle(&mut self) {
        match (self.visible, self.pinned) {
            (false, _) => self.visible = true,
            (true, true) => {} // pinned: protected from toggle-to-hide
            (true, false) => self.visible = false,
        }
    }

    // --- pin ----------------------------------------------------------------

    pub fn is_pinned(&self) -> bool {
        self.pinned
    }

    /// Pin the overlay (and show it — pinning a hidden overlay is meaningless).
    pub fn pin(&mut self) {
        self.pinned = true;
        self.visible = true;
    }

    /// Unpin the overlay (leaves visibility as-is).
    pub fn unpin(&mut self) {
        self.pinned = false;
    }

    /// The Ctrl+Backquote action: flip the pin.
    pub fn toggle_pin(&mut self) {
        if self.pinned {
            self.unpin();
        } else {
            self.pin();
        }
    }

    // --- density ------------------------------------------------------------

    pub fn density(&self) -> OverlayDensity {
        self.density
    }

    pub fn set_density(&mut self, density: OverlayDensity) {
        self.density = density;
    }

    /// The Shift+Backquote action: advance to the next density.
    pub fn cycle_density(&mut self) {
        self.density = self.density.cycle();
    }

    // --- console ------------------------------------------------------------

    pub fn console(&self) -> &ConsoleState {
        &self.console
    }

    pub fn console_mut(&mut self) -> &mut ConsoleState {
        &mut self.console
    }

    /// The Alt+Backquote action: open the overlay and focus the console.
    pub fn focus_console(&mut self) {
        self.visible = true;
        self.console.set_focused(true);
    }

    /// Escape in the console: blur it but keep the overlay open.
    pub fn blur_console(&mut self) {
        self.console.set_focused(false);
    }

    pub fn command_history_count(&self) -> usize {
        self.console.history_len()
    }

    /// ArrowUp recall.
    pub fn history_prev(&mut self) -> Option<String> {
        self.console.navigate_prev()
    }

    /// ArrowDown recall.
    pub fn history_next(&mut self) -> Option<String> {
        self.console.navigate_next()
    }

    // --- diagnostics --------------------------------------------------------

    pub fn diagnostics(&self) -> &BrowserDiagnosticsSnapshot {
        &self.diagnostics
    }

    /// Replace the diagnostics read-out (called by the host each frame).
    pub fn update_diagnostics(&mut self, snapshot: BrowserDiagnosticsSnapshot) {
        self.diagnostics = snapshot;
    }

    // --- view model ---------------------------------------------------------

    /// The header's right-hand status: density, and a pin marker when pinned.
    pub fn header_status(&self) -> String {
        if self.pinned {
            format!("{} • pinned", self.density.label())
        } else {
            self.density.label().to_string()
        }
    }

    /// The ordered rows to render at the current density.
    ///
    /// * `compact` — the four at-a-glance fields.
    /// * `normal` — the core host diagnostics plus the command-history count.
    /// * `verbose` — everything in `normal`, plus the raw backend selection,
    ///   the overlay's own debug state, and a command-history preview.
    pub fn visible_rows(&self) -> Vec<DiagnosticRow> {
        match self.density {
            OverlayDensity::Compact => self.diagnostics.compact_rows(),
            OverlayDensity::Normal => self.normal_rows(),
            OverlayDensity::Verbose => {
                let mut rows = self.normal_rows();
                rows.push(DiagnosticRow::new(
                    "backend select",
                    self.diagnostics.backend_select_text(),
                ));
                rows.push(DiagnosticRow::new("overlay state", self.debug_state_text()));
                rows.push(DiagnosticRow::new("history", self.history_preview()));
                rows
            }
        }
    }

    fn normal_rows(&self) -> Vec<DiagnosticRow> {
        let mut rows = self.diagnostics.core_rows();
        rows.push(DiagnosticRow::new(
            "cmd history",
            self.command_history_count().to_string(),
        ));
        rows
    }

    /// The raw overlay debug state line shown at verbose density.
    fn debug_state_text(&self) -> String {
        format!(
            "density={} pinned={} visible={} console={}",
            self.density.label(),
            self.pinned,
            self.visible,
            self.console.is_focused(),
        )
    }

    /// The last few submitted commands, newest last, joined for the preview row.
    fn history_preview(&self) -> String {
        let history = self.console.history();
        let start = history.len().saturating_sub(HISTORY_PREVIEW);
        let preview = &history[start..];
        if preview.is_empty() {
            "(none)".to_string()
        } else {
            preview.join(" | ")
        }
    }

    /// The result lines the overlay shows above the console input.
    pub fn recent_results(&self) -> &[CommandResult] {
        self.console.recent_results(RECENT_RESULTS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_hidden_unpinned_normal() {
        let s = OverlayState::new();
        assert!(!s.is_visible());
        assert!(!s.is_pinned());
        assert_eq!(s.density(), OverlayDensity::Normal);
        assert_eq!(s.command_history_count(), 0);
    }

    #[test]
    fn toggle_shows_then_hides_when_unpinned() {
        let mut s = OverlayState::new();
        s.toggle();
        assert!(s.is_visible());
        s.toggle();
        assert!(!s.is_visible());
    }

    #[test]
    fn pin_protects_against_toggle_hide_but_explicit_hide_wins() {
        let mut s = OverlayState::new();
        s.pin();
        assert!(s.is_visible() && s.is_pinned());
        // Backquote toggle cannot hide a pinned overlay…
        s.toggle();
        assert!(s.is_visible());
        // …but the explicit hide (overlay.hide / API) always can.
        s.hide();
        assert!(!s.is_visible());
    }

    #[test]
    fn toggle_pin_flips_and_unpin_keeps_visibility() {
        let mut s = OverlayState::new();
        s.toggle_pin(); // pins + shows
        assert!(s.is_pinned() && s.is_visible());
        s.toggle_pin(); // unpins, stays visible
        assert!(!s.is_pinned() && s.is_visible());
        // Now an unpinned, visible overlay hides on toggle.
        s.toggle();
        assert!(!s.is_visible());
    }

    #[test]
    fn cycle_density_walks_the_ring() {
        let mut s = OverlayState::new();
        assert_eq!(s.density(), OverlayDensity::Normal);
        s.cycle_density();
        assert_eq!(s.density(), OverlayDensity::Verbose);
        s.cycle_density();
        assert_eq!(s.density(), OverlayDensity::Compact);
        s.cycle_density();
        assert_eq!(s.density(), OverlayDensity::Normal);
    }

    #[test]
    fn focus_console_opens_the_overlay_and_focuses() {
        let mut s = OverlayState::new();
        s.focus_console();
        assert!(s.is_visible());
        assert!(s.console().is_focused());
        s.blur_console();
        assert!(!s.console().is_focused());
        assert!(s.is_visible()); // blur keeps the overlay open
    }

    #[test]
    fn compact_density_shows_four_rows() {
        let mut s = OverlayState::new();
        s.set_density(OverlayDensity::Compact);
        let rows = s.visible_rows();
        let labels: Vec<&str> = rows.iter().map(|r| r.label.as_str()).collect();
        assert_eq!(labels, vec!["fps", "frame ms", "renderer", "fallbacks"]);
    }

    #[test]
    fn normal_density_ends_with_the_command_history_count() {
        let s = OverlayState::new();
        let rows = s.visible_rows();
        assert_eq!(rows.last().unwrap().label, "cmd history");
        assert_eq!(rows.last().unwrap().value, "0");
    }

    #[test]
    fn verbose_density_appends_backend_state_and_history_rows() {
        let mut s = OverlayState::new();
        s.set_density(OverlayDensity::Verbose);
        let rows = s.visible_rows();
        let labels: Vec<&str> = rows.iter().map(|r| r.label.as_str()).collect();
        assert!(labels.contains(&"backend select"));
        assert!(labels.contains(&"overlay state"));
        assert!(labels.contains(&"history"));
        // Verbose is a superset of normal.
        assert!(labels.contains(&"cmd history"));
    }

    #[test]
    fn verbose_history_preview_reads_none_then_the_recent_commands() {
        let mut s = OverlayState::new();
        s.set_density(OverlayDensity::Verbose);
        let history_row = |s: &OverlayState| {
            s.visible_rows()
                .into_iter()
                .find(|r| r.label == "history")
                .unwrap()
                .value
        };
        assert_eq!(history_row(&s), "(none)");
        s.console_mut().record("help");
        s.console_mut().record("clear");
        assert_eq!(history_row(&s), "help | clear");
    }

    #[test]
    fn update_diagnostics_is_reflected_in_rows() {
        let mut s = OverlayState::new();
        s.set_density(OverlayDensity::Compact);
        s.update_diagnostics(BrowserDiagnosticsSnapshot::stub().with_frame(7, 7, 30.0, 33.0));
        let fps = s.visible_rows().into_iter().find(|r| r.label == "fps").unwrap().value;
        assert_eq!(fps, "30.0");
    }

    #[test]
    fn header_status_marks_the_pin() {
        let mut s = OverlayState::new();
        assert_eq!(s.header_status(), "normal");
        s.pin();
        assert_eq!(s.header_status(), "normal • pinned");
    }
}
