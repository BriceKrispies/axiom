//! `HostOutcomeSet`: per-player terminal outcomes, stably ordered.

use crate::host_outcome::HostOutcome;
use crate::player_id::PlayerId;

/// The multiplayer terminal seam (SPEC-12 §16.6): a [`PlayerId`]→[`HostOutcome`]
/// map with a **stable iteration order**.
///
/// The room authority owns it and reports each seated player's result once. Like
/// the other host-seam maps, the entries are held in an order-preserving vector
/// so the projected record is reproducible. Cross-ref SPEC-13 for `PlayerId` and
/// the authority deployment.
///
/// Seats are not de-duplicated; [`Self::get`] returns the first match, keeping
/// construction branchless.
#[derive(Debug, Clone, PartialEq)]
pub struct HostOutcomeSet {
    entries: Vec<(PlayerId, HostOutcome)>,
}

impl HostOutcomeSet {
    /// Carry the per-player outcomes in the given stable order.
    pub fn new(entries: Vec<(PlayerId, HostOutcome)>) -> Self {
        HostOutcomeSet { entries }
    }

    /// The number of seated outcomes.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the set has no outcomes.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The outcome for `player`, or `None` if absent (first match wins).
    pub fn get(&self, player: PlayerId) -> Option<&HostOutcome> {
        self.entries
            .iter()
            .find(|(seat, _)| *seat == player)
            .map(|(_, outcome)| outcome)
    }

    /// The per-player outcomes in stable order.
    pub fn entries(&self) -> &[(PlayerId, HostOutcome)] {
        &self.entries
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host_metrics::HostMetrics;
    use crate::score::Score;

    fn outcome(won: bool, score: f64) -> HostOutcome {
        HostOutcome::new(won, Score::new(score), HostMetrics::new())
    }

    fn sample() -> HostOutcomeSet {
        HostOutcomeSet::new(vec![
            (PlayerId::new(0), outcome(true, 10.0)),
            (PlayerId::new(1), outcome(false, 4.0)),
        ])
    }

    #[test]
    fn empty_set_reports_empty() {
        let set = HostOutcomeSet::new(Vec::new());
        assert_eq!(set.len(), 0);
        assert!(set.is_empty());
        assert_eq!(set.entries(), &[]);
        assert_eq!(set.get(PlayerId::new(0)), None);
    }

    #[test]
    fn set_maps_each_seat_to_its_outcome() {
        let set = sample();
        assert_eq!(set.len(), 2);
        assert!(!set.is_empty());
        assert_eq!(set.get(PlayerId::new(0)), Some(&outcome(true, 10.0)));
        assert_eq!(set.get(PlayerId::new(1)), Some(&outcome(false, 4.0)));
        assert_eq!(set.get(PlayerId::new(9)), None);
        assert_eq!(set.entries().len(), 2);
    }

    #[test]
    fn equal_inputs_build_equal_sets() {
        assert_eq!(sample(), sample());
        assert_ne!(sample(), HostOutcomeSet::new(Vec::new()));
    }
}
