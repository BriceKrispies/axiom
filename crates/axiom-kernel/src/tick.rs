//! A simulation tick count — a deterministic, data-supplied unit of progress.

/// A monotonic count of fixed simulation steps that have elapsed.
///
/// A `Tick` is pure data: it never reads a clock. It is supplied and advanced
/// explicitly by a [`crate::simulation_clock::SimulationClock`]. Advancement
/// saturates at `u64::MAX` so it can never wrap or panic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Tick(u64);

impl Tick {
    /// The tick before any step has been taken.
    pub const ZERO: Tick = Tick(0);

    /// Construct a tick from a raw count.
    pub const fn new(raw: u64) -> Self {
        Tick(raw)
    }

    /// The raw count.
    pub const fn raw(self) -> u64 {
        self.0
    }

    /// The next tick. Saturating, so it is total and never panics.
    pub const fn next(self) -> Self {
        Tick(self.0.saturating_add(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_is_default() {
        assert_eq!(Tick::default(), Tick::ZERO);
        assert_eq!(Tick::ZERO.raw(), 0);
    }

    #[test]
    fn new_and_raw_round_trip() {
        assert_eq!(Tick::new(123).raw(), 123);
    }

    #[test]
    fn next_increments_by_one() {
        assert_eq!(Tick::new(41).next(), Tick::new(42));
    }

    #[test]
    fn next_saturates_at_max() {
        assert_eq!(Tick::new(u64::MAX).next(), Tick::new(u64::MAX));
    }

    #[test]
    fn ordering_is_numeric() {
        assert!(Tick::new(1) < Tick::new(2));
    }
}
