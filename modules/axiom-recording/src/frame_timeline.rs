//! [`FrameTimeline`] — a bounded, deterministic ring buffer of [`FrameCapture`]s.
//!
//! It enforces **two** hard memory bounds at once: at most `max_frames` retained
//! captures and at most `max_bytes` of accounted memory. `push_frame` rejects a
//! single capture larger than the whole budget, appends in insertion order, then
//! evicts the oldest captures until *both* bounds hold. Eviction is computed,
//! not looped conditionally, so the whole module stays branchless.

use std::collections::VecDeque;

use axiom_kernel::{FrameIndex, KernelResult};

use crate::error::{
    capture_too_large, frame_not_present, no_next_frame, no_previous_frame, timeline_empty,
    zero_max_bytes, zero_max_frames,
};
use crate::frame_capture::FrameCapture;

/// A bounded deterministic timeline of frame captures in insertion order.
#[derive(Debug, Clone)]
pub struct FrameTimeline {
    max_frames: usize,
    max_bytes: usize,
    current_bytes: usize,
    captures: VecDeque<FrameCapture>,
}

impl FrameTimeline {
    /// A fresh empty timeline with the given bounds. Rejects a zero `max_frames`
    /// or `max_bytes` with a deterministic error (a timeline must retain at least
    /// one frame and have a positive byte budget).
    pub(crate) fn new(max_frames: usize, max_bytes: usize) -> KernelResult<Self> {
        (max_frames == 0)
            .then(|| Err(zero_max_frames()))
            .or_else(|| (max_bytes == 0).then(|| Err(zero_max_bytes())))
            .unwrap_or_else(|| {
                Ok(FrameTimeline {
                    max_frames,
                    max_bytes,
                    current_bytes: 0,
                    captures: VecDeque::new(),
                })
            })
    }

    /// Push a capture, then evict oldest captures until both bounds hold. Rejects
    /// (without mutating) a capture that alone exceeds `max_bytes`. After a
    /// successful return `len() <= max_frames` and `current_bytes() <= max_bytes`.
    pub(crate) fn push_frame(&mut self, capture: FrameCapture) -> KernelResult<()> {
        let too_big = capture.byte_len() > self.max_bytes;
        let rejected = too_big.then(|| Err(capture_too_large()));
        rejected.unwrap_or_else(|| {
            self.current_bytes = self.current_bytes.saturating_add(capture.byte_len());
            self.captures.push_back(capture);
            self.evict();
            Ok(())
        })
    }

    /// Evict the smallest prefix of oldest captures so that both bounds hold.
    /// The eviction count is *computed* (max of the frame-overflow and the
    /// byte-overflow prefix length), so there is no conditional loop.
    fn evict(&mut self) {
        let frames_over = self.captures.len().saturating_sub(self.max_frames);
        let target = self.current_bytes.saturating_sub(self.max_bytes);
        // `before` = cumulative bytes freed *before* removing this capture; we
        // must remove captures while that running total is still below `target`.
        let bytes_over = self
            .captures
            .iter()
            .scan(0_usize, |acc, c| {
                let before = *acc;
                *acc = acc.saturating_add(c.byte_len());
                Some(before)
            })
            .take_while(|&before| before < target)
            .count();
        let evict_n = frames_over.max(bytes_over);
        let removed: usize = self.captures.drain(0..evict_n).map(|c| c.byte_len()).sum();
        self.current_bytes = self.current_bytes.saturating_sub(removed);
    }

    /// Empty the timeline and reset the byte accounting.
    pub(crate) fn clear(&mut self) {
        self.captures.clear();
        self.current_bytes = 0;
    }

    /// The number of retained captures.
    pub(crate) fn len(&self) -> usize {
        self.captures.len()
    }

    /// Whether the timeline retains no captures.
    pub(crate) fn is_empty(&self) -> bool {
        self.captures.is_empty()
    }

    /// The accounted bytes currently retained.
    pub(crate) fn current_bytes(&self) -> usize {
        self.current_bytes
    }

    /// The configured memory budget.
    pub(crate) fn max_bytes(&self) -> usize {
        self.max_bytes
    }

    /// The configured maximum retained frame count.
    pub(crate) fn max_frames(&self) -> usize {
        self.max_frames
    }

    /// The captures in deterministic insertion order (oldest → newest).
    pub(crate) fn captures(&self) -> &VecDeque<FrameCapture> {
        &self.captures
    }

    /// The oldest retained capture, or a deterministic error if empty.
    pub(crate) fn oldest_frame(&self) -> KernelResult<&FrameCapture> {
        self.captures.front().ok_or_else(timeline_empty)
    }

    /// The newest retained capture, or a deterministic error if empty.
    pub(crate) fn latest_frame(&self) -> KernelResult<&FrameCapture> {
        self.captures.back().ok_or_else(timeline_empty)
    }

    /// The oldest retained frame index, or a deterministic error if empty.
    pub(crate) fn oldest_frame_index(&self) -> KernelResult<FrameIndex> {
        self.oldest_frame().map(FrameCapture::frame_index)
    }

    /// The newest retained frame index, or a deterministic error if empty.
    pub(crate) fn latest_frame_index(&self) -> KernelResult<FrameIndex> {
        self.latest_frame().map(FrameCapture::frame_index)
    }

    /// The capture at `frame_index`, or a deterministic error if it was never
    /// recorded or has been evicted.
    pub(crate) fn get_frame(&self, frame_index: FrameIndex) -> KernelResult<&FrameCapture> {
        self.captures
            .iter()
            .find(|c| c.frame_index() == frame_index)
            .ok_or_else(frame_not_present)
    }

    /// The insertion position of `frame_index`, if retained.
    fn position_of(&self, frame_index: FrameIndex) -> Option<usize> {
        self.captures
            .iter()
            .position(|c| c.frame_index() == frame_index)
    }

    /// The frame index of the retained capture immediately before `frame_index`,
    /// or a deterministic error if there is none (oldest, or not present).
    pub(crate) fn previous_frame_index(&self, frame_index: FrameIndex) -> KernelResult<FrameIndex> {
        self.position_of(frame_index)
            .and_then(|p| p.checked_sub(1))
            .and_then(|p| self.captures.get(p))
            .map(FrameCapture::frame_index)
            .ok_or_else(no_previous_frame)
    }

    /// The frame index of the retained capture immediately after `frame_index`,
    /// or a deterministic error if there is none (latest, or not present).
    pub(crate) fn next_frame_index(&self, frame_index: FrameIndex) -> KernelResult<FrameIndex> {
        self.position_of(frame_index)
            .map(|p| p + 1)
            .and_then(|p| self.captures.get(p))
            .map(FrameCapture::frame_index)
            .ok_or_else(no_next_frame)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::{KernelErrorCode, Tick};

    /// A capture at `(frame, frame*10)` whose render payload is `pad` bytes long,
    /// for predictable byte budgeting.
    fn cap(frame: u64, pad: usize) -> FrameCapture {
        FrameCapture::new(
            FrameIndex::new(frame),
            Tick::new(frame * 10),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            vec![0_u8; pad],
        )
    }

    #[test]
    fn zero_max_frames_is_rejected() {
        assert_eq!(
            FrameTimeline::new(0, 1024).unwrap_err().code(),
            KernelErrorCode::OutOfBounds
        );
    }

    #[test]
    fn zero_max_bytes_is_rejected() {
        assert!(FrameTimeline::new(8, 0).is_err());
    }

    #[test]
    fn pushing_frames_preserves_insertion_order() {
        let mut t = FrameTimeline::new(8, 1 << 20).unwrap();
        (0..4).for_each(|f| t.push_frame(cap(f, 0)).unwrap());
        let order: Vec<u64> = t.captures().iter().map(|c| c.frame_index().raw()).collect();
        assert_eq!(order, vec![0, 1, 2, 3]);
        assert_eq!(t.len(), 4);
        assert!(!t.is_empty());
    }

    #[test]
    fn bounded_max_frames_evicts_the_oldest_deterministically() {
        let mut t = FrameTimeline::new(3, 1 << 20).unwrap();
        (0..5).for_each(|f| t.push_frame(cap(f, 0)).unwrap());
        let order: Vec<u64> = t.captures().iter().map(|c| c.frame_index().raw()).collect();
        assert_eq!(order, vec![2, 3, 4]);
        assert_eq!(t.len(), 3);
    }

    #[test]
    fn bounded_max_bytes_evicts_oldest_frames_deterministically() {
        // Budget holds ~2 captures of this size.
        let one = cap(0, 64).byte_len();
        let mut t = FrameTimeline::new(1000, one * 2 + 8).unwrap();
        (0..5).for_each(|f| t.push_frame(cap(f, 64)).unwrap());
        let order: Vec<u64> = t.captures().iter().map(|c| c.frame_index().raw()).collect();
        assert_eq!(order, vec![3, 4]);
        assert!(t.current_bytes() <= t.max_bytes());
    }

    #[test]
    fn a_single_capture_larger_than_max_bytes_is_rejected() {
        let mut t = FrameTimeline::new(10, 32).unwrap();
        let big = cap(0, 1024);
        assert!(big.byte_len() > 32);
        assert!(t.push_frame(big).is_err());
        // Rejected push does not mutate the timeline.
        assert_eq!(t.len(), 0);
        assert_eq!(t.current_bytes(), 0);
    }

    #[test]
    fn current_bytes_updates_after_push_and_eviction() {
        let mut t = FrameTimeline::new(2, 1 << 20).unwrap();
        t.push_frame(cap(0, 10)).unwrap();
        let after_one = t.current_bytes();
        assert_eq!(after_one, cap(0, 10).byte_len());
        t.push_frame(cap(1, 10)).unwrap();
        t.push_frame(cap(2, 10)).unwrap(); // evicts frame 0
        assert_eq!(t.len(), 2);
        assert_eq!(
            t.current_bytes(),
            cap(1, 10).byte_len() + cap(2, 10).byte_len()
        );
    }

    #[test]
    fn fetching_an_existing_frame_returns_the_exact_capture() {
        let mut t = FrameTimeline::new(8, 1 << 20).unwrap();
        let c = cap(7, 3);
        t.push_frame(c.clone()).unwrap();
        assert_eq!(t.get_frame(FrameIndex::new(7)).unwrap(), &c);
    }

    #[test]
    fn fetching_an_evicted_frame_returns_a_deterministic_error() {
        let mut t = FrameTimeline::new(2, 1 << 20).unwrap();
        (0..3).for_each(|f| t.push_frame(cap(f, 0)).unwrap()); // evicts 0
        assert!(t.get_frame(FrameIndex::new(0)).is_err());
        // A never-recorded frame is also a deterministic error.
        assert!(t.get_frame(FrameIndex::new(99)).is_err());
    }

    #[test]
    fn latest_and_oldest_frames() {
        let mut t = FrameTimeline::new(8, 1 << 20).unwrap();
        // Empty timeline → deterministic errors.
        assert!(t.latest_frame().is_err());
        assert!(t.oldest_frame().is_err());
        assert!(t.latest_frame_index().is_err());
        assert!(t.oldest_frame_index().is_err());
        (5..9).for_each(|f| t.push_frame(cap(f, 0)).unwrap());
        assert_eq!(t.oldest_frame_index().unwrap(), FrameIndex::new(5));
        assert_eq!(t.latest_frame_index().unwrap(), FrameIndex::new(8));
    }

    #[test]
    fn previous_and_next_navigation() {
        let mut t = FrameTimeline::new(8, 1 << 20).unwrap();
        (1..4).for_each(|f| t.push_frame(cap(f, 0)).unwrap()); // 1,2,3
        assert_eq!(
            t.previous_frame_index(FrameIndex::new(2)).unwrap(),
            FrameIndex::new(1)
        );
        assert_eq!(
            t.next_frame_index(FrameIndex::new(2)).unwrap(),
            FrameIndex::new(3)
        );
        // Edges + missing selection.
        assert!(t.previous_frame_index(FrameIndex::new(1)).is_err());
        assert!(t.next_frame_index(FrameIndex::new(3)).is_err());
        assert!(t.previous_frame_index(FrameIndex::new(99)).is_err());
        assert!(t.next_frame_index(FrameIndex::new(99)).is_err());
    }

    #[test]
    fn clear_removes_all_captures_and_resets_bytes() {
        let mut t = FrameTimeline::new(8, 1 << 20).unwrap();
        (0..3).for_each(|f| t.push_frame(cap(f, 5)).unwrap());
        t.clear();
        assert!(t.is_empty());
        assert_eq!(t.len(), 0);
        assert_eq!(t.current_bytes(), 0);
    }

    #[test]
    fn max_frames_and_max_bytes_getters() {
        let t = FrameTimeline::new(11, 2048).unwrap();
        assert_eq!(t.max_frames(), 11);
        assert_eq!(t.max_bytes(), 2048);
    }
}
