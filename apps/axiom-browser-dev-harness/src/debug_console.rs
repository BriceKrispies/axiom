//! The console's in-memory model: command history, history navigation, the
//! rendered result log, and focus.
//!
//! This is pure state with no DOM knowledge. The DOM controller maps key events
//! onto these methods (ArrowUp → [`ConsoleState::navigate_prev`], Enter →
//! record + dispatch + [`ConsoleState::append_result`], Escape → blur) and reads
//! [`ConsoleState::recent_results`] back out to paint the log.

use crate::debug_command::CommandResult;

/// The console's history, result log, navigation cursor, and focus flag.
#[derive(Debug, Default, Clone)]
pub struct ConsoleState {
    /// Submitted commands, oldest first.
    history: Vec<String>,
    /// Navigation position into `history`. `None` is the live (unsubmitted) line.
    cursor: Option<usize>,
    /// The rendered result log, oldest first.
    results: Vec<CommandResult>,
    /// Whether the console input currently owns focus.
    focused: bool,
}

impl ConsoleState {
    pub fn new() -> Self {
        ConsoleState::default()
    }

    /// Record a submitted command into history (non-empty only) and reset
    /// navigation to the live line. The raw line is trimmed first.
    pub fn record(&mut self, command: &str) {
        let trimmed = command.trim();
        if !trimmed.is_empty() {
            self.history.push(trimmed.to_string());
        }
        self.cursor = None;
    }

    /// Number of commands recorded — surfaced in the overlay as
    /// "command history count".
    pub fn history_len(&self) -> usize {
        self.history.len()
    }

    /// The recorded commands, oldest first (used for the verbose history
    /// preview).
    pub fn history(&self) -> &[String] {
        &self.history
    }

    /// Append a result line to the log.
    pub fn append_result(&mut self, result: CommandResult) {
        self.results.push(result);
    }

    /// Drop every logged result (the `clear` command).
    pub fn clear_results(&mut self) {
        self.results.clear();
    }

    /// The last `max` results, oldest first — what the overlay renders above the
    /// input.
    pub fn recent_results(&self, max: usize) -> &[CommandResult] {
        let start = self.results.len().saturating_sub(max);
        &self.results[start..]
    }

    /// Mark the console focused/blurred (Alt+Backquote focuses, Escape blurs).
    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    pub fn is_focused(&self) -> bool {
        self.focused
    }

    /// ArrowUp: step toward older history. Returns the recalled command to place
    /// in the input, or `None` when there is no history to recall.
    pub fn navigate_prev(&mut self) -> Option<String> {
        let len = self.history.len();
        if len == 0 {
            return None;
        }
        let idx = match self.cursor {
            None => len - 1,
            Some(i) => i.saturating_sub(1),
        };
        self.cursor = Some(idx);
        Some(self.history[idx].clone())
    }

    /// ArrowDown: step toward newer history. Past the newest entry the cursor
    /// returns to the live (empty) line, yielding `Some("")`. At the live line
    /// already, returns `None` (nothing to do).
    pub fn navigate_next(&mut self) -> Option<String> {
        let i = self.cursor?;
        let next = i + 1;
        if next < self.history.len() {
            self.cursor = Some(next);
            Some(self.history[next].clone())
        } else {
            self.cursor = None;
            Some(String::new())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_only_non_empty_commands() {
        let mut c = ConsoleState::new();
        c.record("help");
        c.record("   "); // ignored
        c.record(""); // ignored
        c.record("  clear  "); // trimmed + recorded
        assert_eq!(c.history_len(), 2);
        assert_eq!(c.history(), &["help".to_string(), "clear".to_string()]);
    }

    #[test]
    fn results_append_clear_and_window_to_recent() {
        let mut c = ConsoleState::new();
        for i in 0..5 {
            c.append_result(CommandResult::ok(format!("c{i}"), format!("m{i}")));
        }
        // Only the last 3 are "recent".
        let recent: Vec<&str> = c.recent_results(3).iter().map(|r| r.command.as_str()).collect();
        assert_eq!(recent, vec!["c2", "c3", "c4"]);
        c.clear_results();
        assert!(c.recent_results(3).is_empty());
    }

    #[test]
    fn focus_flag_tracks_set_focused() {
        let mut c = ConsoleState::new();
        assert!(!c.is_focused());
        c.set_focused(true);
        assert!(c.is_focused());
        c.set_focused(false);
        assert!(!c.is_focused());
    }

    #[test]
    fn arrow_up_walks_back_through_history() {
        let mut c = ConsoleState::new();
        c.record("first");
        c.record("second");
        c.record("third");
        // Up from the live line → newest, then older, clamping at the oldest.
        assert_eq!(c.navigate_prev(), Some("third".to_string()));
        assert_eq!(c.navigate_prev(), Some("second".to_string()));
        assert_eq!(c.navigate_prev(), Some("first".to_string()));
        assert_eq!(c.navigate_prev(), Some("first".to_string())); // clamped
    }

    #[test]
    fn arrow_down_walks_forward_to_the_live_line() {
        let mut c = ConsoleState::new();
        c.record("first");
        c.record("second");
        // Walk all the way up…
        c.navigate_prev(); // second
        c.navigate_prev(); // first
        // …then down: first → second → live empty line → nothing.
        assert_eq!(c.navigate_next(), Some("second".to_string()));
        assert_eq!(c.navigate_next(), Some(String::new()));
        assert_eq!(c.navigate_next(), None);
    }

    #[test]
    fn arrow_keys_are_noops_without_history() {
        let mut c = ConsoleState::new();
        assert_eq!(c.navigate_prev(), None);
        assert_eq!(c.navigate_next(), None);
    }

    #[test]
    fn recording_resets_navigation_to_the_live_line() {
        let mut c = ConsoleState::new();
        c.record("old");
        c.navigate_prev(); // cursor at "old"
        c.record("new"); // resets cursor to live line
        // Next Up should recall the newest ("new"), proving the cursor reset.
        assert_eq!(c.navigate_prev(), Some("new".to_string()));
    }
}
