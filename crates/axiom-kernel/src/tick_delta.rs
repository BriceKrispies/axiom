//! A non-negative offset between two [`crate::tick::Tick`]s.

/// A non-negative count of fixed simulation steps between two [`Tick`]s.
///
/// `TickDelta` is pure integer time data — never a wall-clock duration and never
/// a floating-point number — the companion to [`Tick`]: a `Tick` is a point in
/// logical time, a `TickDelta` is a distance between two such points. Higher
/// layers and modules name it so a deadline offset (`after(n)` ticks, the ticks a
/// state machine has been in a state) crosses an API as a typed quantity rather
/// than a bare `u64`.
///
/// [`Tick`]: crate::tick::Tick
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TickDelta(u64);

impl TickDelta {
    /// A delta of `ticks` logical steps.
    pub const fn new(ticks: u64) -> Self {
        TickDelta(ticks)
    }

    /// The raw step count.
    pub const fn raw(self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_and_raw_round_trip() {
        assert_eq!(TickDelta::new(42).raw(), 42);
    }

    #[test]
    fn equality_and_clone_and_debug() {
        let a = TickDelta::new(7);
        let b = a;
        assert_eq!(a, b.clone());
        assert_ne!(TickDelta::new(1), TickDelta::new(2));
        assert_eq!(format!("{a:?}"), "TickDelta(7)");
    }
}
