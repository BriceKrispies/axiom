//! The per-frame fixed-step budget produced by the accumulator.

/// How many fixed simulation steps a frame should run, plus the sub-step
/// remainder left in the accumulator afterwards.
///
/// Produced by [`crate::FrameAccumulator::advance`]. Every field is an explicit
/// integer count of nanoseconds or whole steps — nothing here is a float and
/// nothing is derived from a wall clock, exactly like [`crate::FrameTiming`].
///
/// `steps` is the deterministic, simulation-driving output: a fixed update runs
/// exactly this many times this frame. `remainder_nanos` is the time left in the
/// accumulator that did not complete a step — it is in `[0, fixed_step_nanos)`
/// and is **presentation-only**. The `0..1` interpolation fraction a renderer
/// wants between the last two ticks is `remainder_nanos / fixed_step_nanos`;
/// that division is float math computed at the presentation boundary (where
/// float math is unconstrained), never inside the deterministic spine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StepBudget {
    steps: u32,
    remainder_nanos: u64,
    fixed_step_nanos: u64,
}

impl StepBudget {
    /// Construct a budget. `remainder_nanos` is always `< fixed_step_nanos` by
    /// construction in [`crate::FrameAccumulator::advance`].
    pub(crate) const fn new(steps: u32, remainder_nanos: u64, fixed_step_nanos: u64) -> Self {
        StepBudget {
            steps,
            remainder_nanos,
            fixed_step_nanos,
        }
    }

    /// The number of fixed simulation steps to run this frame.
    pub const fn steps(&self) -> u32 {
        self.steps
    }

    /// Sub-step time left in the accumulator, in `[0, fixed_step_nanos)`.
    pub const fn remainder_nanos(&self) -> u64 {
        self.remainder_nanos
    }

    /// The fixed step size, so a presentation layer can compute the
    /// interpolation fraction `remainder_nanos / fixed_step_nanos` itself.
    pub const fn fixed_step_nanos(&self) -> u64 {
        self.fixed_step_nanos
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accessors_return_constructed_fields() {
        let b = StepBudget::new(3, 250, 1_000);
        assert_eq!(b.steps(), 3);
        assert_eq!(b.remainder_nanos(), 250);
        assert_eq!(b.fixed_step_nanos(), 1_000);
    }

    #[test]
    fn equal_budgets_compare_and_format() {
        let a = StepBudget::new(1, 0, 16);
        let b = StepBudget::new(1, 0, 16);
        assert_eq!(a, b);
        assert!(format!("{a:?}").contains("StepBudget"));
    }
}
