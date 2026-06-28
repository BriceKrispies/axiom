//! Deterministic fixed-step accumulator.

use crate::frame_error::FrameError;
use crate::frame_result::FrameResult;
use crate::step_budget::StepBudget;

/// Folds real elapsed presentation time into whole fixed simulation steps.
///
/// The engine advances the simulation at a constant fixed step, but a host
/// delivers frames at a variable, wall-clock cadence. The accumulator is the
/// deterministic bridge: each frame it banks the elapsed nanoseconds the host
/// reports and yields how many whole fixed steps now fit, carrying the sub-step
/// remainder to the next frame. Given the same elapsed sequence it produces the
/// same step counts, and the same *total* steps no matter how that elapsed time
/// was chunked across frames — the invariant a deterministic replay relies on.
///
/// It reads no clock of its own: elapsed time enters as explicit data, exactly
/// as [`crate::FrameTiming`] takes its nanoseconds from a host report. The
/// per-frame step count is clamped to a caller-supplied ceiling so a long stall
/// cannot trigger an unbounded catch-up (the "spiral of death"); the clamped
/// time is retained in the bank, not dropped, so a recovered stall drains over
/// subsequent frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FrameAccumulator {
    fixed_step_nanos: u64,
    banked_nanos: u64,
}

impl FrameAccumulator {
    /// Create an accumulator for a fixed step of `fixed_step_nanos`. A zero step
    /// has no meaning (every frame would complete infinitely many steps), so it
    /// is rejected — the one validated boundary that lets [`advance`] divide
    /// without a guard.
    ///
    /// [`advance`]: FrameAccumulator::advance
    pub fn new(fixed_step_nanos: u64) -> FrameResult<FrameAccumulator> {
        (fixed_step_nanos != 0)
            .then_some(FrameAccumulator {
                fixed_step_nanos,
                banked_nanos: 0,
            })
            .ok_or_else(|| {
                FrameError::invalid_frame_timing("frame accumulator fixed step must be non-zero")
            })
    }

    /// Bank `elapsed_nanos` of real time and report how many whole fixed steps
    /// now fit, clamped to `max_steps`. The sub-step remainder stays banked for
    /// the next frame; whole steps clamped away by `max_steps` also stay banked
    /// (never dropped).
    pub fn advance(&mut self, elapsed_nanos: u64, max_steps: u32) -> StepBudget {
        self.banked_nanos = self.banked_nanos.saturating_add(elapsed_nanos);
        let steps = u32::try_from(self.banked_nanos / self.fixed_step_nanos)
            .unwrap_or(u32::MAX)
            .min(max_steps);
        let consumed = u64::from(steps).saturating_mul(self.fixed_step_nanos);
        self.banked_nanos = self.banked_nanos.saturating_sub(consumed);
        let remainder_nanos = self.banked_nanos % self.fixed_step_nanos;
        StepBudget::new(steps, remainder_nanos, self.fixed_step_nanos)
    }

    /// The fixed step size in nanoseconds.
    pub const fn fixed_step_nanos(&self) -> u64 {
        self.fixed_step_nanos
    }

    /// Total time currently banked but not yet consumed by a step: the sub-step
    /// remainder plus any whole steps clamped away by `max_steps`.
    pub const fn banked_nanos(&self) -> u64 {
        self.banked_nanos
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame_error_code::FrameErrorCode;

    const STEP: u64 = 1_000;

    fn acc() -> FrameAccumulator {
        FrameAccumulator::new(STEP).unwrap()
    }

    #[test]
    fn new_rejects_zero_fixed_step() {
        let err = FrameAccumulator::new(0).unwrap_err();
        assert_eq!(err.code(), FrameErrorCode::InvalidFrameTiming);
    }

    #[test]
    fn new_accepts_nonzero_and_starts_empty() {
        let a = acc();
        assert_eq!(a.fixed_step_nanos(), STEP);
        assert_eq!(a.banked_nanos(), 0);
    }

    #[test]
    fn exact_one_step_leaves_no_remainder() {
        let mut a = acc();
        let b = a.advance(STEP, 5);
        assert_eq!(b.steps(), 1);
        assert_eq!(b.remainder_nanos(), 0);
        assert_eq!(a.banked_nanos(), 0);
    }

    #[test]
    fn multiple_whole_steps_in_one_frame() {
        let mut a = acc();
        assert_eq!(a.advance(3 * STEP, 5).steps(), 3);
    }

    #[test]
    fn sub_step_time_is_retained_as_remainder() {
        let mut a = acc();
        let b = a.advance(STEP / 2, 5);
        assert_eq!(b.steps(), 0);
        assert_eq!(b.remainder_nanos(), STEP / 2);
        assert_eq!(a.banked_nanos(), STEP / 2);
    }

    #[test]
    fn remainder_carries_across_frames_to_complete_a_step() {
        let mut a = acc();
        assert_eq!(a.advance(600, 5).steps(), 0);
        // 600 + 600 = 1200 banked -> one step, 200 sub-step remainder.
        let b = a.advance(600, 5);
        assert_eq!(b.steps(), 1);
        assert_eq!(b.remainder_nanos(), 200);
    }

    #[test]
    fn step_count_is_clamped_and_excess_time_is_retained() {
        let mut a = acc();
        let b = a.advance(100 * STEP, 5);
        assert_eq!(b.steps(), 5);
        // 95 whole steps' worth of time stays banked, not dropped.
        assert_eq!(a.banked_nanos(), 95 * STEP);
        assert_eq!(b.remainder_nanos(), 0);
        // A following frame with no new time drains five more.
        assert_eq!(a.advance(0, 5).steps(), 5);
        assert_eq!(a.banked_nanos(), 90 * STEP);
    }

    #[test]
    fn huge_elapsed_saturates_step_count_without_truncating() {
        let mut a = FrameAccumulator::new(1).unwrap();
        let elapsed = u64::from(u32::MAX) + 10;
        assert_eq!(a.advance(elapsed, u32::MAX).steps(), u32::MAX);
    }

    #[test]
    fn total_steps_are_independent_of_frame_chunking() {
        // Same total elapsed, drained to empty, yields the same step count
        // regardless of how it was split across frames — the replay invariant.
        let total = 10 * STEP + 250;
        let drain = |chunks: &[u64]| -> u32 {
            let mut a = acc();
            let mut steps: u32 = chunks.iter().map(|&e| a.advance(e, u32::MAX).steps()).sum();
            // Flush any banked whole steps the chunking left behind.
            steps += a.advance(0, u32::MAX).steps();
            steps
        };
        assert_eq!(drain(&[total]), 10);
        assert_eq!(drain(&[total]), drain(&[STEP, total - STEP]));
        assert_eq!(drain(&[total]), drain(&[1, 1, total - 2]));
    }

    #[test]
    fn every_nanosecond_is_either_consumed_or_banked() {
        // Conservation: consumed + banked == total elapsed, always.
        let mut a = acc();
        let elapsed = [700_u64, 1_500, 50, 9_000];
        let consumed: u64 = elapsed
            .iter()
            .map(|&e| u64::from(a.advance(e, u32::MAX).steps()) * STEP)
            .sum();
        let total: u64 = elapsed.iter().sum();
        assert_eq!(consumed + a.banked_nanos(), total);
    }

    #[test]
    fn identical_inputs_produce_identical_state() {
        let mut a = acc();
        let mut b = acc();
        let _ = a.advance(2 * STEP + 30, 5);
        let _ = b.advance(2 * STEP + 30, 5);
        assert_eq!(a, b);
    }
}
