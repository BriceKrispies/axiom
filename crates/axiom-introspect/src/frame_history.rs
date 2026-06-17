//! A bounded, deterministic window of recent frame reports.

use crate::frame_report::FrameReport;

/// A fixed-capacity ring of the most recent [`FrameReport`]s, in arrival
/// order.
///
/// Recording past capacity evicts the oldest report, so memory is bounded and
/// the contents are a pure function of the observed frame sequence — no clock,
/// no allocation growth, replay-identical. Capacity is clamped to at least 1.
#[derive(Debug, Clone)]
pub struct FrameHistory {
    capacity: usize,
    frames: Vec<FrameReport>,
}

impl FrameHistory {
    /// Create an empty history retaining at most `capacity` reports (clamped
    /// to a minimum of 1).
    pub fn new(capacity: usize) -> Self {
        FrameHistory {
            capacity: capacity.max(1),
            frames: Vec::new(),
        }
    }

    /// Append a report, evicting the oldest if at capacity.
    pub fn record(&mut self, report: FrameReport) {
        (self.frames.len() == self.capacity).then(|| self.frames.remove(0));
        self.frames.push(report);
    }

    /// The most recent `n` reports in arrival order (fewer if the history is
    /// shorter than `n`).
    pub fn recent(&self, n: usize) -> &[FrameReport] {
        let start = self.frames.len().saturating_sub(n);
        &self.frames[start..]
    }

    /// The retained report with the given engine frame index, if present.
    pub fn describe(&self, engine_frame_index: u64) -> Option<&FrameReport> {
        self.frames
            .iter()
            .find(|frame| frame.engine_frame_index() == engine_frame_index)
    }

    /// The number of retained reports.
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    /// Whether no reports are retained.
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// The maximum number of reports this history retains.
    pub const fn capacity(&self) -> usize {
        self.capacity
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures;

    fn reports(n: u64) -> Vec<FrameReport> {
        fixtures::active_engine_frames(n)
            .iter()
            .map(FrameReport::from_frame)
            .collect()
    }

    #[test]
    fn new_clamps_zero_capacity_to_one() {
        let history = FrameHistory::new(0);
        assert_eq!(history.capacity(), 1);
        assert!(history.is_empty());
        assert_eq!(history.len(), 0);
    }

    #[test]
    fn records_up_to_capacity() {
        let mut history = FrameHistory::new(8);
        for report in reports(3) {
            history.record(report);
        }
        assert_eq!(history.len(), 3);
        assert!(!history.is_empty());
    }

    #[test]
    fn recording_past_capacity_evicts_oldest() {
        let mut history = FrameHistory::new(2);
        let all = reports(3);
        let kept_index = all[2].engine_frame_index();
        let evicted_index = all[0].engine_frame_index();
        for report in all {
            history.record(report);
        }
        assert_eq!(history.len(), 2);
        assert!(history.describe(evicted_index).is_none());
        assert!(history.describe(kept_index).is_some());
    }

    #[test]
    fn recent_clamps_to_available() {
        let mut history = FrameHistory::new(8);
        for report in reports(3) {
            history.record(report);
        }
        assert_eq!(history.recent(0).len(), 0);
        assert_eq!(history.recent(2).len(), 2);
        assert_eq!(history.recent(3).len(), 3);
        assert_eq!(history.recent(99).len(), 3);
    }

    #[test]
    fn describe_finds_present_and_misses_absent() {
        let mut history = FrameHistory::new(8);
        let all = reports(2);
        let present = all[1].engine_frame_index();
        for report in all {
            history.record(report);
        }
        assert!(history.describe(present).is_some());
        assert!(history.describe(9_999).is_none());
    }
}
