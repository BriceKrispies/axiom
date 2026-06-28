//! A bounded, deterministic window of recent frame reports.

use axiom_kernel::{
    BinaryReader, BinaryWriter, KernelError, KernelErrorCode, KernelErrorScope, KernelResult,
    SchemaVersion,
};

use crate::frame_report::FrameReport;

/// The wire schema version of a serialized [`FrameHistory`]. Bumped on
/// incompatible layout changes; the major component gates compatibility.
const HISTORY_SCHEMA: SchemaVersion = SchemaVersion::new(1, 0);

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
    ///
    /// Engine frame indices are monotonic in practice, so at most one report
    /// matches; the scan runs newest-first so that, were an index ever to repeat,
    /// the most recently observed report wins (the answer an agent expects).
    pub fn describe(&self, engine_frame_index: u64) -> Option<&FrameReport> {
        self.frames
            .iter()
            .rev()
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

    /// Serialize the whole retained window to bytes — the snapshot an external
    /// agent reads to inspect every recent frame at once (vs. the single latest
    /// frame). Records the capacity and each report in arrival order.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut writer = BinaryWriter::new();
        HISTORY_SCHEMA.write_to(&mut writer);
        writer.write_u32(self.capacity as u32);
        writer.write_u32(self.frames.len() as u32);
        self.frames.iter().for_each(|frame| frame.write_to(&mut writer));
        writer.into_bytes()
    }

    /// Decode a window previously produced by [`Self::to_bytes`], reconstructing
    /// the ring with its stored capacity. Fails with
    /// [`KernelErrorCode::SchemaVersionMismatch`] for an incompatible major
    /// version, or a binary error for truncated/invalid data.
    pub fn from_bytes(bytes: &[u8]) -> KernelResult<Self> {
        let mut reader = BinaryReader::new(bytes);
        // Branchless sequential decode: schema guard, capacity, count, then each
        // report, all threaded through `and_then` so the first error short-circuits.
        SchemaVersion::read_from(&mut reader)
            .and_then(|version| {
                HISTORY_SCHEMA
                    .is_compatible_with(version)
                    .then_some(())
                    .ok_or_else(|| {
                        KernelError::new(
                            KernelErrorScope::Binary,
                            KernelErrorCode::SchemaVersionMismatch,
                            "FrameHistory schema major version is incompatible",
                        )
                    })
            })
            .and_then(|()| reader.read_u32())
            .and_then(|capacity| {
                reader.read_u32().and_then(|count| {
                    (0..count)
                        .map(|_| FrameReport::read_from(&mut reader))
                        .collect::<KernelResult<Vec<_>>>()
                        .map(|frames| FrameHistory {
                            capacity: (capacity as usize).max(1),
                            frames,
                        })
                })
            })
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

    #[test]
    fn describe_resolves_a_duplicate_index_to_the_most_recent() {
        // Record the same engine frame index twice; the newest-first scan must
        // return the instance recorded last (proven by pointer identity with the
        // newest retained report).
        let mut history = FrameHistory::new(8);
        let report = reports(1).pop().unwrap();
        let idx = report.engine_frame_index();
        history.record(report.clone());
        history.record(report);
        let found = history.describe(idx).unwrap();
        assert!(std::ptr::eq(found, history.recent(1).last().unwrap()));
    }

    #[test]
    fn serializes_and_round_trips_the_whole_window() {
        let mut history = FrameHistory::new(4);
        for report in reports(3) {
            history.record(report);
        }
        let decoded = FrameHistory::from_bytes(&history.to_bytes()).unwrap();
        assert_eq!(decoded.capacity(), 4);
        assert_eq!(decoded.len(), 3);
        assert_eq!(decoded.recent(3), history.recent(3));
    }

    #[test]
    fn serializes_an_empty_window() {
        let history = FrameHistory::new(2);
        let decoded = FrameHistory::from_bytes(&history.to_bytes()).unwrap();
        assert!(decoded.is_empty());
        assert_eq!(decoded.capacity(), 2);
    }

    #[test]
    fn truncation_at_every_prefix_is_err() {
        let mut history = FrameHistory::new(4);
        for report in reports(2) {
            history.record(report);
        }
        let bytes = history.to_bytes();
        for len in 0..bytes.len() {
            assert!(
                FrameHistory::from_bytes(&bytes[..len]).is_err(),
                "truncated decode at len {len} must fail"
            );
        }
    }

    #[test]
    fn incompatible_schema_major_is_rejected() {
        let mut writer = axiom_kernel::BinaryWriter::new();
        SchemaVersion::new(HISTORY_SCHEMA.major() + 1, 0).write_to(&mut writer);
        let bytes = writer.into_bytes();
        assert_eq!(
            FrameHistory::from_bytes(&bytes).unwrap_err().code(),
            KernelErrorCode::SchemaVersionMismatch
        );
    }
}
