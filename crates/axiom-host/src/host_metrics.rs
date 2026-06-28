//! `HostMetrics`: an opaque, stably-ordered terminal-metric map.

use crate::score::Score;

/// An opaque key→number map of named outcome metrics with a **stable iteration
/// order** (SPEC-12 §6).
///
/// Like [`crate::HostSessionParams`], order is fixed so the projected JS record
/// (`Record<string, number>`) and any logged form are reproducible. Values are
/// numbers only, each carried through the [`Score`] f64 boundary so no naked
/// `f64` reaches the surface. The host boundary carries these; the game fills
/// them.
///
/// Keys are not de-duplicated; [`Self::with`] appends in order and
/// [`Self::get`] returns the first match, keeping construction branchless.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HostMetrics {
    entries: Vec<(String, Score)>,
}

impl HostMetrics {
    /// An empty metric map.
    pub fn new() -> Self {
        HostMetrics::default()
    }

    /// Append `key → value`, preserving insertion order, and return the map.
    pub fn with(mut self, key: String, value: Score) -> Self {
        self.entries.push((key, value));
        self
    }

    /// The number of metrics.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the map has no metrics.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The value for `key`, or `None` if absent (first match wins).
    pub fn get(&self, key: &str) -> Option<Score> {
        self.entries
            .iter()
            .find(|(stored, _)| stored.as_str() == key)
            .map(|(_, value)| *value)
    }

    /// The metrics in stable insertion order.
    pub fn entries(&self) -> &[(String, Score)] {
        &self.entries
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> HostMetrics {
        HostMetrics::new()
            .with(String::from("kills"), Score::new(4.0))
            .with(String::from("time"), Score::new(12.5))
    }

    #[test]
    fn empty_map_reports_empty() {
        let metrics = HostMetrics::new();
        assert_eq!(metrics.len(), 0);
        assert!(metrics.is_empty());
        assert_eq!(metrics.entries(), &[]);
        assert_eq!(metrics.get("missing"), None);
    }

    #[test]
    fn with_preserves_insertion_order() {
        let metrics = sample();
        assert_eq!(metrics.len(), 2);
        assert!(!metrics.is_empty());
        assert_eq!(
            metrics.entries(),
            &[
                (String::from("kills"), Score::new(4.0)),
                (String::from("time"), Score::new(12.5)),
            ]
        );
    }

    #[test]
    fn get_returns_present_value_and_none_for_absent_key() {
        let metrics = sample();
        assert_eq!(metrics.get("kills"), Some(Score::new(4.0)));
        assert_eq!(metrics.get("absent"), None);
    }

    #[test]
    fn equal_inputs_build_equal_maps_and_order_matters() {
        assert_eq!(sample(), sample());
        let reversed = HostMetrics::new()
            .with(String::from("time"), Score::new(12.5))
            .with(String::from("kills"), Score::new(4.0));
        assert_ne!(sample(), reversed);
    }
}
