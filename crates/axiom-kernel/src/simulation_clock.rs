//! A deterministic clock advanced only by explicit fixed steps.

use crate::error::KernelError;
use crate::error_code::KernelErrorCode;
use crate::error_scope::KernelErrorScope;
use crate::fixed_step::FixedStep;
use crate::frame_index::FrameIndex;
use crate::result::KernelResult;
use crate::tick::Tick;

/// A simulation clock that advances **only** from explicit fixed-step inputs.
///
/// It never reads wall-clock time. Given the same initial [`FixedStep`] and the
/// same sequence of advances, two clocks always reach byte-identical state, so
/// simulations are replayable. Tick and frame index advance in lock-step, and
/// elapsed nanoseconds are tracked with checked arithmetic that fails loudly on
/// overflow rather than wrapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SimulationClock {
    step: FixedStep,
    tick: Tick,
    frame: FrameIndex,
    elapsed_nanos: u64,
}

impl SimulationClock {
    /// Create an initial clock at tick 0 / frame 0 with the given step.
    pub const fn new(step: FixedStep) -> Self {
        SimulationClock {
            step,
            tick: Tick::ZERO,
            frame: FrameIndex::ZERO,
            elapsed_nanos: 0,
        }
    }

    /// Advance by exactly one fixed step.
    ///
    /// Returns [`KernelErrorCode::RangeOverflow`] if accumulated nanoseconds
    /// would exceed `u64::MAX`.
    pub fn advance(&mut self) -> KernelResult<()> {
        self.elapsed_nanos
            .checked_add(self.step.nanos())
            .ok_or(KernelError::new(
                KernelErrorScope::Time,
                KernelErrorCode::RangeOverflow,
                "simulation clock elapsed nanoseconds overflowed u64",
            ))
            .map(|next_elapsed| {
                // Mutate only on success; on overflow the clock is unchanged.
                self.elapsed_nanos = next_elapsed;
                self.tick = self.tick.next();
                self.frame = self.frame.next();
            })
    }

    /// Advance by `n` fixed steps.
    ///
    /// Equivalent to calling [`Self::advance`] `n` times: advancing by `n` and
    /// advancing one-at-a-time `n` times reach identical state. On overflow the
    /// clock is left unchanged from before the call's overflowing step.
    pub fn advance_by(&mut self, n: u64) -> KernelResult<()> {
        (0..n).try_for_each(|_| self.advance())
    }

    /// The current tick.
    pub const fn tick(&self) -> Tick {
        self.tick
    }

    /// The current frame index.
    pub const fn frame(&self) -> FrameIndex {
        self.frame
    }

    /// Total simulated nanoseconds elapsed since construction.
    pub const fn elapsed_nanos(&self) -> u64 {
        self.elapsed_nanos
    }

    /// The fixed step this clock advances by.
    pub const fn step(&self) -> FixedStep {
        self.step
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clock(nanos: u64) -> SimulationClock {
        SimulationClock::new(FixedStep::new(nanos).unwrap())
    }

    #[test]
    fn initial_clock_is_at_zero() {
        let c = clock(1000);
        assert_eq!(c.tick(), Tick::ZERO);
        assert_eq!(c.frame(), FrameIndex::ZERO);
        assert_eq!(c.elapsed_nanos(), 0);
        assert_eq!(c.step(), FixedStep::new(1000).unwrap());
    }

    #[test]
    fn single_advance_moves_tick_frame_and_time() {
        let mut c = clock(1000);
        c.advance().unwrap();
        assert_eq!(c.tick(), Tick::new(1));
        assert_eq!(c.frame(), FrameIndex::new(1));
        assert_eq!(c.elapsed_nanos(), 1000);
    }

    #[test]
    fn advance_by_equals_repeated_advance() {
        let mut a = clock(500);
        a.advance_by(4).unwrap();

        let mut b = clock(500);
        for _ in 0..4 {
            b.advance().unwrap();
        }

        assert_eq!(a, b, "advance_by(n) must equal n single advances");
        assert_eq!(a.tick(), Tick::new(4));
        assert_eq!(a.elapsed_nanos(), 2000);
    }

    #[test]
    fn advancement_is_deterministic_across_two_clocks() {
        let mut a = clock(16_666_667);
        let mut b = clock(16_666_667);
        a.advance_by(60).unwrap();
        b.advance_by(60).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn overflow_is_reported_not_wrapped() {
        let mut c = clock(u64::MAX);
        c.advance().unwrap(); // elapsed == u64::MAX
        let err = c.advance().unwrap_err();
        assert_eq!(err.scope(), KernelErrorScope::Time);
        assert_eq!(err.code(), KernelErrorCode::RangeOverflow);
    }
}

#[cfg(test)]
mod cov {
    use super::*;
    use crate::fixed_step::FixedStep;

    #[test]
    fn advance_by_runs_and_no_ops_at_zero() {
        let mut c = SimulationClock::new(FixedStep::new(1000).unwrap());
        c.advance_by(3).unwrap();
        assert_eq!(c.tick().raw(), 3);
        c.advance_by(0).unwrap();
        assert_eq!(c.tick().raw(), 3);
    }
}

#[cfg(test)]
mod cov2 {
    use super::*;
    use crate::fixed_step::FixedStep;

    #[test]
    fn advance_reports_overflow() {
        let mut c = SimulationClock::new(FixedStep::new(u64::MAX).unwrap());
        assert!(c.advance().is_ok()); // 0 + MAX
        assert!(c.advance().is_err()); // MAX + MAX overflows
    }

    #[test]
    fn advance_by_propagates_overflow() {
        let mut c = SimulationClock::new(FixedStep::new(u64::MAX).unwrap());
        assert!(c.advance_by(2).is_err());
    }
}
