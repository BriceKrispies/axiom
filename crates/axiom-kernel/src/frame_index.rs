//! A frame index — a deterministic counter of presented frames.

/// A monotonic index identifying a frame of the simulation.
///
/// Like [`crate::tick::Tick`] this is pure data and never reads a clock. The
/// kernel advances it in lock-step with ticks; higher layers may decide what a
/// "frame" presents. Advancement saturates so it can never wrap or panic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct FrameIndex(u64);

impl FrameIndex {
    /// The index before any frame has advanced.
    pub const ZERO: FrameIndex = FrameIndex(0);

    /// Construct a frame index from a raw value.
    pub const fn new(raw: u64) -> Self {
        FrameIndex(raw)
    }

    /// The raw value.
    pub const fn raw(self) -> u64 {
        self.0
    }

    /// The next frame index. Saturating, so it is total and never panics.
    pub const fn next(self) -> Self {
        FrameIndex(self.0.saturating_add(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_is_default() {
        assert_eq!(FrameIndex::default(), FrameIndex::ZERO);
        assert_eq!(FrameIndex::ZERO.raw(), 0);
    }

    #[test]
    fn new_and_raw_round_trip() {
        assert_eq!(FrameIndex::new(9).raw(), 9);
    }

    #[test]
    fn next_increments_by_one() {
        assert_eq!(FrameIndex::new(0).next(), FrameIndex::new(1));
    }

    #[test]
    fn next_saturates_at_max() {
        assert_eq!(FrameIndex::new(u64::MAX).next(), FrameIndex::new(u64::MAX));
    }

    #[test]
    fn ordering_is_numeric() {
        assert!(FrameIndex::new(3) < FrameIndex::new(4));
    }
}
