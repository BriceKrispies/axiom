//! The deterministic logical tick model for process scheduling.
//!
//! Integer ticks only — no wall-clock, no platform time. A [`SimTick`] is a point
//! in logical simulation time; a [`TickDelta`] is a non-negative offset; checked
//! addition rejects overflow cleanly.

/// A non-negative offset between two [`SimTick`]s.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TickDelta(u64);

impl TickDelta {
    /// A delta of `ticks` logical ticks.
    pub const fn new(ticks: u64) -> Self {
        TickDelta(ticks)
    }
}

/// A point in logical simulation time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SimTick(u64);

impl SimTick {
    /// A tick at logical time `value`.
    pub const fn new(value: u64) -> Self {
        SimTick(value)
    }

    /// The raw logical time.
    pub const fn raw(self) -> u64 {
        self.0
    }

    /// Advance by `delta`, `None` on overflow (checked tick math).
    pub fn checked_add(self, delta: TickDelta) -> Option<SimTick> {
        self.0.checked_add(delta.0).map(SimTick)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticks_construct_and_order() {
        assert_eq!(SimTick::new(5).raw(), 5);
        assert!(SimTick::new(3) < SimTick::new(4));
        assert_eq!(SimTick::new(7), SimTick::new(7));
        assert!(TickDelta::new(1) < TickDelta::new(2));
        assert_eq!(
            SimTick::new(0).checked_add(TickDelta::new(9)),
            Some(SimTick::new(9))
        );
    }

    #[test]
    fn checked_add_succeeds_and_overflows_cleanly() {
        assert_eq!(
            SimTick::new(10).checked_add(TickDelta::new(5)),
            Some(SimTick::new(15))
        );
        assert_eq!(
            SimTick::new(0).checked_add(TickDelta::new(0)),
            Some(SimTick::new(0))
        );
        assert_eq!(SimTick::new(u64::MAX).checked_add(TickDelta::new(1)), None);
    }
}
