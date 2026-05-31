//! The agent-facing introspection facade.

use axiom_frame::EngineFrame;

use crate::frame_history::FrameHistory;
use crate::frame_report::FrameReport;

/// The query surface an agent uses to interrogate a running engine.
///
/// An owner feeds each completed [`EngineFrame`] to [`Self::observe`]; the
/// facade projects it into a [`FrameReport`] and retains it in a bounded
/// [`FrameHistory`]. Everything else is a read: describe a frame by index,
/// fetch the recent window, or hand out a serialized snapshot. The facade
/// holds no engine state of its own and never reads a clock — its answers are
/// a pure function of the frames it has observed.
#[derive(Debug)]
pub struct IntrospectApi {
    history: FrameHistory,
}

impl IntrospectApi {
    /// Create a facade retaining at most `capacity` recent frames.
    pub fn new(capacity: usize) -> Self {
        IntrospectApi {
            history: FrameHistory::new(capacity),
        }
    }

    /// Record one completed engine frame.
    pub fn observe(&mut self, frame: &EngineFrame) {
        self.history.record(FrameReport::from_frame(frame));
    }

    /// The recorded report for the given engine frame index, if still retained.
    pub fn describe_frame(&self, engine_frame_index: u64) -> Option<&FrameReport> {
        self.history.describe(engine_frame_index)
    }

    /// The most recent `n` reports, in arrival order.
    pub fn recent(&self, n: usize) -> &[FrameReport] {
        self.history.recent(n)
    }

    /// The most recently observed report, if any.
    pub fn latest(&self) -> Option<&FrameReport> {
        self.history.recent(1).last()
    }

    /// How many frames are currently retained.
    pub fn frame_count(&self) -> usize {
        self.history.len()
    }

    /// A serialized snapshot of the most recent frame — the bytes an external
    /// agent reads. `None` until at least one frame has been observed.
    pub fn snapshot_bytes(&self) -> Option<Vec<u8>> {
        self.latest().map(FrameReport::to_bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures;

    #[test]
    fn fresh_facade_is_empty() {
        let api = IntrospectApi::new(4);
        assert_eq!(api.frame_count(), 0);
        assert!(api.latest().is_none());
        assert!(api.snapshot_bytes().is_none());
        assert_eq!(api.recent(3).len(), 0);
        assert!(api.describe_frame(0).is_none());
    }

    #[test]
    fn observing_records_and_answers_queries() {
        let mut api = IntrospectApi::new(8);
        let frames = fixtures::active_engine_frames(3);
        for frame in &frames {
            api.observe(frame);
        }
        assert_eq!(api.frame_count(), 3);

        // Indices are monotonic across the observed frames.
        let indices: Vec<u64> = api
            .recent(3)
            .iter()
            .map(FrameReport::engine_frame_index)
            .collect();
        assert!(indices.windows(2).all(|w| w[0] < w[1]));

        // describe_frame round-trips a known index; an absent one misses.
        let known = frames[1].engine_frame_index();
        assert_eq!(
            api.describe_frame(known).unwrap().engine_frame_index(),
            known
        );
        assert!(api.describe_frame(1_000_000).is_none());

        // latest is the last observed frame.
        let last = frames[2].engine_frame_index();
        assert_eq!(api.latest().unwrap().engine_frame_index(), last);
    }

    #[test]
    fn snapshot_bytes_round_trip_to_the_latest_report() {
        let mut api = IntrospectApi::new(8);
        api.observe(&fixtures::failing_engine_frame());
        let bytes = api.snapshot_bytes().expect("a frame was observed");
        let decoded = FrameReport::from_bytes(&bytes).unwrap();
        assert_eq!(&decoded, api.latest().unwrap());
        assert_eq!(decoded.systems().len(), 1);
    }

    #[test]
    fn observation_sequence_is_deterministic() {
        let build = || {
            let mut api = IntrospectApi::new(8);
            for frame in &fixtures::active_engine_frames(2) {
                api.observe(frame);
            }
            api.recent(2).to_vec()
        };
        assert_eq!(build(), build());
    }
}
