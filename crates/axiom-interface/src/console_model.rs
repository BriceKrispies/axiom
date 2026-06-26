//! [`ConsoleModel`] — the in-memory model of a console: command history, history
//! navigation, and the result log. Pure, branchless, no platform knowledge.
//! (Focus ownership lives in [`crate::focus_state`], one level up, so it can
//! transfer between panels.)

use crate::interface_command::CommandOutcome;

/// A console's history, navigation cursor, and result log.
#[derive(Debug, Default, Clone)]
pub(crate) struct ConsoleModel {
    history: Vec<String>,
    /// Navigation position into `history`. `None` is the live (unsubmitted) line.
    cursor: Option<usize>,
    results: Vec<CommandOutcome>,
}

impl ConsoleModel {
    pub(crate) fn new() -> Self {
        ConsoleModel::default()
    }

    /// Record a submitted command (non-empty, trimmed) and reset navigation.
    pub(crate) fn record(&mut self, command: &str) {
        let trimmed = command.trim();
        (!trimmed.is_empty()).then(|| self.history.push(trimmed.to_string()));
        self.cursor = None;
    }

    pub(crate) fn history_len(&self) -> usize {
        self.history.len()
    }

    /// The last `max` recorded commands, oldest first (for a history preview).
    pub(crate) fn recent_history(&self, max: usize) -> &[String] {
        let start = self.history.len().saturating_sub(max);
        &self.history[start..]
    }

    pub(crate) fn append_result(&mut self, outcome: CommandOutcome) {
        self.results.push(outcome);
    }

    pub(crate) fn clear_results(&mut self) {
        self.results.clear();
    }

    /// The last `max` results, oldest first.
    pub(crate) fn recent_results(&self, max: usize) -> &[CommandOutcome] {
        let start = self.results.len().saturating_sub(max);
        &self.results[start..]
    }

    /// Step toward older history. The recalled command, or `None` if empty.
    pub(crate) fn recall_prev(&mut self) -> Option<String> {
        let len = self.history.len();
        (len > 0).then(|| {
            let idx = self.cursor.map_or(len - 1, |i| i.saturating_sub(1));
            self.cursor = Some(idx);
            self.history[idx].clone()
        })
    }

    /// Step toward newer history. Past the newest entry the cursor returns to the
    /// live (empty) line, yielding `Some("")`; at the live line already, `None`.
    pub(crate) fn recall_next(&mut self) -> Option<String> {
        self.cursor.map(|i| {
            let next = i + 1;
            let in_range = next < self.history.len();
            self.cursor = in_range.then_some(next);
            self.history.get(next).cloned().unwrap_or_default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_only_non_empty_and_resets_cursor() {
        let mut c = ConsoleModel::new();
        c.record("help");
        c.record("   ");
        c.record("  clear  ");
        assert_eq!(c.history_len(), 2);
    }

    #[test]
    fn results_append_clear_and_window() {
        let mut c = ConsoleModel::new();
        (0..5).for_each(|i| c.append_result(CommandOutcome::ok(format!("c{i}"), "m")));
        let recent: Vec<&str> = c
            .recent_results(3)
            .iter()
            .map(CommandOutcome::command)
            .collect();
        assert_eq!(recent, vec!["c2", "c3", "c4"]);
        c.clear_results();
        assert!(c.recent_results(3).is_empty());
    }

    #[test]
    fn recall_walks_history_both_ways() {
        let mut c = ConsoleModel::new();
        ["first", "second", "third"]
            .iter()
            .for_each(|cmd| c.record(cmd));
        assert_eq!(c.recall_prev(), Some("third".to_string()));
        assert_eq!(c.recall_prev(), Some("second".to_string()));
        assert_eq!(c.recall_prev(), Some("first".to_string()));
        assert_eq!(c.recall_prev(), Some("first".to_string())); // clamped
        assert_eq!(c.recall_next(), Some("second".to_string()));
        assert_eq!(c.recall_next(), Some("third".to_string()));
        assert_eq!(c.recall_next(), Some(String::new())); // live line
        assert_eq!(c.recall_next(), None);
    }

    #[test]
    fn recall_is_noop_without_history() {
        let mut c = ConsoleModel::new();
        assert_eq!(c.recall_prev(), None);
        assert_eq!(c.recall_next(), None);
    }

    #[test]
    fn recent_history_windows_the_tail() {
        let mut c = ConsoleModel::new();
        assert!(c.recent_history(4).is_empty());
        ["a", "b", "c", "d", "e"]
            .iter()
            .for_each(|cmd| c.record(cmd));
        assert_eq!(
            c.recent_history(3),
            &["c".to_string(), "d".to_string(), "e".to_string()]
        );
        assert_eq!(c.recent_history(10).len(), 5);
    }
}
