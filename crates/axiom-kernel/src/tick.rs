//! A simulation tick count — a deterministic, data-supplied unit of progress.

use crate::tick_delta::TickDelta;

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

    /// The tick `delta` steps after this one. Saturating, so it is total and
    /// never panics — a deadline at `now.add(n)` that would overflow clamps to
    /// `u64::MAX`. This is where a `TickDelta` offset is applied to a `Tick`, so
    /// callers never hand-roll tick arithmetic.
    pub const fn add(self, delta: TickDelta) -> Self {
        Tick(self.0.saturating_add(delta.raw()))
    }

    /// The non-negative number of ticks from `earlier` up to this tick.
    /// Saturating, so a tick before `earlier` yields a zero delta rather than
    /// wrapping — total and panic-free.
    pub const fn delta_since(self, earlier: Tick) -> TickDelta {
        TickDelta::new(self.0.saturating_sub(earlier.0))
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

    #[test]
    fn add_delta_advances_and_saturates() {
        assert_eq!(Tick::new(10).add(TickDelta::new(5)), Tick::new(15));
        assert_eq!(Tick::new(7).add(TickDelta::new(0)), Tick::new(7));
        assert_eq!(
            Tick::new(u64::MAX).add(TickDelta::new(1)),
            Tick::new(u64::MAX)
        );
    }

    #[test]
    fn delta_since_measures_and_saturates() {
        assert_eq!(Tick::new(12).delta_since(Tick::new(5)), TickDelta::new(7));
        assert_eq!(Tick::new(5).delta_since(Tick::new(5)), TickDelta::new(0));
        // An earlier "now" yields a zero delta, never a wrap.
        assert_eq!(Tick::new(3).delta_since(Tick::new(9)), TickDelta::new(0));
    }
}
