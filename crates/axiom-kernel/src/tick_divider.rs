//! A fixed-step sub-rate counter: fire once every N simulation ticks.
//!
//! A [`TickDivider`] divides the simulation tick stream by a fixed `period`. It
//! is advanced once per tick (alongside a
//! [`crate::simulation_clock::SimulationClock`] advance) and returns `true` on
//! every `period`-th advance, then re-arms. It is pure data and never reads a
//! clock, so a divider created at any tick still fires every `period` ticks from
//! its own creation — deterministically and reproducibly.
//!
//! It is the fixed-step companion to [`crate::fixed_step::FixedStep`]:
//! `FixedStep` sets the duration of one tick; `TickDivider` runs work at a
//! whole-number sub-rate of it (every Nth tick) — e.g. a half-second cadence at
//! 60 ticks/second is `TickDivider::new(30)`.

use crate::error::KernelError;
use crate::error_code::KernelErrorCode;
use crate::error_scope::KernelErrorScope;
use crate::result::KernelResult;

/// A frequency divider over the simulation tick stream.
///
/// Advanced once per tick; fires (`advance` returns `true`) on each `period`-th
/// advance. The `period` must be strictly positive — a zero period would "fire"
/// on a stream that never progresses — so it is rejected at construction and
/// every constructed `TickDivider` is guaranteed valid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TickDivider {
    period: u32,
    remaining: u32,
}

impl TickDivider {
    /// Construct a divider that fires once every `period` ticks.
    ///
    /// Returns [`KernelErrorCode::InvalidTickDivider`] if `period` is zero.
    pub const fn new(period: u32) -> KernelResult<Self> {
        let valid = period != 0;
        [
            Err(KernelError::new(
                KernelErrorScope::Time,
                KernelErrorCode::InvalidTickDivider,
                "tick divider period must be greater than zero ticks",
            )),
            Ok(TickDivider {
                period,
                remaining: period,
            }),
        ][valid as usize]
    }

    /// Advance by one tick. Returns `true` on the tick that completes a period
    /// (and re-arms for the next), `false` on the ticks in between.
    pub fn advance(&mut self) -> bool {
        // `remaining` is invariantly in `1..=period` on entry (set to `period`
        // at construction and re-armed to `period` on each fire), so the
        // decrement never underflows.
        self.remaining -= 1;
        let fired = self.remaining == 0;
        // Branchless re-arm: on a fire reset to `period`, otherwise keep the
        // decremented value.
        self.remaining = [self.remaining, self.period][fired as usize];
        fired
    }

    /// The period: how many ticks elapse between fires.
    pub const fn period(self) -> u32 {
        self.period
    }

    /// Ticks remaining until the next fire (always in `1..=period`).
    pub const fn ticks_until_next(self) -> u32 {
        self.remaining
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_period_is_rejected() {
        let err = TickDivider::new(0).unwrap_err();
        assert_eq!(err.scope(), KernelErrorScope::Time);
        assert_eq!(err.code(), KernelErrorCode::InvalidTickDivider);
    }

    #[test]
    fn positive_period_is_accepted() {
        let d = TickDivider::new(3).unwrap();
        assert_eq!(d.period(), 3);
        assert_eq!(d.ticks_until_next(), 3);
    }

    #[test]
    fn fires_once_every_period_ticks_and_rearms() {
        let mut d = TickDivider::new(3).unwrap();
        // Two non-firing ticks then a fire — run two full periods to prove the
        // divider re-arms after firing.
        assert_eq!(
            [
                d.advance(),
                d.advance(),
                d.advance(),
                d.advance(),
                d.advance(),
                d.advance(),
            ],
            [false, false, true, false, false, true]
        );
    }

    #[test]
    fn ticks_until_next_counts_down_and_rearms() {
        let mut d = TickDivider::new(2).unwrap();
        assert_eq!(d.ticks_until_next(), 2);
        assert!(!d.advance());
        assert_eq!(d.ticks_until_next(), 1);
        assert!(d.advance());
        assert_eq!(d.ticks_until_next(), 2, "re-armed after firing");
    }

    #[test]
    fn period_one_fires_every_tick() {
        let mut d = TickDivider::new(1).unwrap();
        assert!(d.advance());
        assert!(d.advance());
    }
}
