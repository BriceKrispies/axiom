//! A spin component an app spawns onto a node: data-declared rotation the
//! engine animates each tick.

use axiom_math::Vec3;

/// A pure rotation about `axis`, one full revolution every `period_ticks`
/// frames. The engine's spin system animates it deterministically from the
/// frame tick, so rotation is replayable and never reads a wall clock.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spin {
    pub axis: Vec3,
    pub period_ticks: u32,
}

impl Spin {
    /// A spin about `axis` with a default one-tick period (set the real period
    /// with [`Self::period`]).
    pub const fn around(axis: Vec3) -> Self {
        Spin {
            axis,
            period_ticks: 1,
        }
    }

    /// Set the number of ticks in one full revolution.
    pub const fn period(self, period_ticks: u32) -> Self {
        Spin {
            axis: self.axis,
            period_ticks,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn around_then_period_builds_the_spin() {
        let spin = Spin::around(Vec3::UNIT_Y).period(360);
        assert_eq!(spin.axis, Vec3::UNIT_Y);
        assert_eq!(spin.period_ticks, 360);
    }

    #[test]
    fn around_defaults_to_one_tick_period() {
        assert_eq!(Spin::around(Vec3::UNIT_X).period_ticks, 1);
    }
}
