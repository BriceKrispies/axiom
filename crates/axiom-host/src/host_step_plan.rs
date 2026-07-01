//! Deterministic per-frame step plan.

use crate::host_boundary_config::HostBoundaryConfig;
use crate::host_frame_input::HostFrameInput;
use crate::host_lifecycle_state::HostLifecycleState;
use crate::host_skip_reason::HostSkipReason;

/// The deterministic plan for a single host frame.
///
/// `HostStepPlan` is a **pure planning object**. It does not call
/// `Runtime::step`; the [`crate::HostStepDriver`] is responsible for
/// executing the plan. Computing the plan as a separate value lets the host
/// boundary be tested without ever constructing a runtime, and it lets
/// adapters peek at the plan before applying it (e.g. to short-circuit
/// rendering on a skipped frame).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostStepPlan {
    sequence: u64,
    steps: u32,
    consumed_nanos: u64,
    retained_nanos: u64,
    skip: Option<HostSkipReason>,
}

impl HostStepPlan {
    /// Compute the plan from the per-frame inputs and the current host
    /// state. Pure and deterministic: identical inputs always produce the
    /// same plan, on every platform, every run.
    ///
    /// Algorithm:
    /// - If `lifecycle.shutdown_requested()` → skip `ShutdownRequested`,
    ///   retain `0` (shutdown discards pending time).
    /// - Else if `lifecycle.suspended()` → skip `LifecycleSuspended`, retain
    ///   per the `retain_accumulator` policy.
    /// - Else if `!lifecycle.visible() && !config.step_while_hidden()` →
    ///   skip `LifecycleHidden`, retain per the policy.
    /// - Else add `elapsed_nanos` to `accumulator_nanos`, take
    ///   `min(accumulator / fixed_step, max_steps_per_frame)` runtime steps,
    ///   consume `steps * fixed_step_nanos`, and retain the unspent slack
    ///   when `retain_accumulator`, else `0`.
    pub fn build(
        input: &HostFrameInput,
        config: &HostBoundaryConfig,
        lifecycle: &HostLifecycleState,
        accumulator_nanos: u64,
    ) -> HostStepPlan {
        let sequence = input.sequence();
        let elapsed = input.elapsed_nanos();

        // Priority order: shutdown wins over suspended, which wins over hidden.
        lifecycle
            .shutdown_requested()
            .then_some(HostStepPlan {
                sequence,
                steps: 0,
                consumed_nanos: 0,
                retained_nanos: 0,
                skip: Some(HostSkipReason::ShutdownRequested),
            })
            .or_else(|| {
                lifecycle.suspended().then_some(HostStepPlan {
                    sequence,
                    steps: 0,
                    consumed_nanos: 0,
                    retained_nanos: retained_after_skip(config, accumulator_nanos, elapsed),
                    skip: Some(HostSkipReason::LifecycleSuspended),
                })
            })
            .or_else(|| {
                (!lifecycle.visible() & !config.step_while_hidden()).then_some(HostStepPlan {
                    sequence,
                    steps: 0,
                    consumed_nanos: 0,
                    retained_nanos: retained_after_skip(config, accumulator_nanos, elapsed),
                    skip: Some(HostSkipReason::LifecycleHidden),
                })
            })
            .unwrap_or_else(|| {
                let fixed = config.fixed_step_nanos();
                let total = accumulator_nanos.saturating_add(elapsed);
                let raw_steps = total.checked_div(fixed).unwrap_or(0);
                let clamped = raw_steps.min(config.max_steps_per_frame() as u64);
                let steps = clamped as u32;
                let consumed_nanos = (steps as u64).saturating_mul(fixed);
                let retained_nanos = [0, total.saturating_sub(consumed_nanos)]
                    [usize::from(config.retain_accumulator())];

                HostStepPlan {
                    sequence,
                    steps,
                    consumed_nanos,
                    retained_nanos,
                    skip: None,
                }
            })
    }

    /// Host frame sequence this plan corresponds to.
    pub const fn sequence(&self) -> u64 {
        self.sequence
    }

    /// Number of `Runtime::step` calls the plan asks for.
    pub const fn steps(&self) -> u32 {
        self.steps
    }

    /// Total simulated nanoseconds the plan consumes.
    pub const fn consumed_nanos(&self) -> u64 {
        self.consumed_nanos
    }

    /// Accumulator carryover after the plan is executed.
    pub const fn retained_nanos(&self) -> u64 {
        self.retained_nanos
    }

    /// Whether the frame was skipped for a lifecycle reason.
    pub const fn skip_reason(&self) -> Option<HostSkipReason> {
        self.skip
    }

    /// Convenience: `true` iff `skip_reason().is_some()`.
    pub const fn is_skipped(&self) -> bool {
        self.skip.is_some()
    }
}

fn retained_after_skip(config: &HostBoundaryConfig, accumulator: u64, elapsed: u64) -> u64 {
    [0, accumulator.saturating_add(elapsed)][usize::from(config.retain_accumulator())]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host_lifecycle_signal::HostLifecycleSignal;
    use crate::host_viewport::HostViewport;
    use axiom_kernel::Ratio;

    const STEP_NANOS: u64 = 1_000;

    fn vp() -> HostViewport {
        HostViewport::new(800, 600, Ratio::new(1.0).unwrap()).unwrap()
    }

    fn cfg() -> HostBoundaryConfig {
        HostBoundaryConfig::new(STEP_NANOS, 5).unwrap()
    }

    fn visible() -> HostLifecycleState {
        HostLifecycleState::initial().apply(HostLifecycleSignal::Started)
    }

    #[test]
    fn exact_one_step_frame() {
        let plan = HostStepPlan::build(
            &HostFrameInput::new(1, STEP_NANOS, vp()),
            &cfg(),
            &visible(),
            0,
        );
        assert_eq!(plan.steps(), 1);
        assert_eq!(plan.consumed_nanos(), STEP_NANOS);
        assert_eq!(plan.retained_nanos(), 0);
        assert!(plan.skip_reason().is_none());
    }

    #[test]
    fn multi_step_catch_up_frame() {
        let plan = HostStepPlan::build(
            &HostFrameInput::new(1, 3 * STEP_NANOS, vp()),
            &cfg(),
            &visible(),
            0,
        );
        assert_eq!(plan.steps(), 3);
        assert_eq!(plan.consumed_nanos(), 3 * STEP_NANOS);
        assert_eq!(plan.retained_nanos(), 0);
    }

    #[test]
    fn max_step_clamp() {
        let plan = HostStepPlan::build(
            &HostFrameInput::new(1, 100 * STEP_NANOS, vp()),
            &cfg(),
            &visible(),
            0,
        );
        assert_eq!(plan.steps(), 5, "clamped to max_steps_per_frame=5");
        assert_eq!(plan.consumed_nanos(), 5 * STEP_NANOS);
        assert_eq!(plan.retained_nanos(), 95 * STEP_NANOS);
    }

    #[test]
    fn retain_accumulator_carries_unspent_time() {
        let plan = HostStepPlan::build(
            &HostFrameInput::new(1, STEP_NANOS / 2, vp()),
            &cfg(),
            &visible(),
            (3 * STEP_NANOS) / 2,
        );
        assert_eq!(plan.steps(), 2);
        assert_eq!(plan.retained_nanos(), 0);

        let plan2 = HostStepPlan::build(
            &HostFrameInput::new(2, STEP_NANOS / 4, vp()),
            &cfg(),
            &visible(),
            STEP_NANOS / 4,
        );
        assert_eq!(plan2.steps(), 0);
        assert_eq!(plan2.retained_nanos(), STEP_NANOS / 2);
    }

    #[test]
    fn no_retain_accumulator_discards_unspent_time() {
        let cfg = cfg().with_retain_accumulator(false);
        let plan = HostStepPlan::build(
            &HostFrameInput::new(1, STEP_NANOS + 250, vp()),
            &cfg,
            &visible(),
            0,
        );
        assert_eq!(plan.steps(), 1);
        assert_eq!(plan.retained_nanos(), 0, "policy discards leftover slack");
    }

    #[test]
    fn hidden_frame_is_skipped() {
        let hidden = HostLifecycleState::initial();
        let plan = HostStepPlan::build(
            &HostFrameInput::new(1, STEP_NANOS, vp()),
            &cfg(),
            &hidden,
            0,
        );
        assert_eq!(plan.skip_reason(), Some(HostSkipReason::LifecycleHidden));
        assert_eq!(plan.steps(), 0);
        // Default config retains accumulator across a hidden skip.
        assert_eq!(plan.retained_nanos(), STEP_NANOS);
    }

    #[test]
    fn hidden_frame_steps_when_policy_allows() {
        let cfg = cfg().with_step_while_hidden(true);
        let hidden = HostLifecycleState::initial();
        let plan = HostStepPlan::build(&HostFrameInput::new(1, STEP_NANOS, vp()), &cfg, &hidden, 0);
        assert!(plan.skip_reason().is_none());
        assert_eq!(plan.steps(), 1);
    }

    #[test]
    fn suspended_frame_is_skipped_unconditionally() {
        let suspended = visible().apply(HostLifecycleSignal::Suspended);
        let cfg = cfg().with_step_while_hidden(true);
        let plan = HostStepPlan::build(
            &HostFrameInput::new(1, STEP_NANOS, vp()),
            &cfg,
            &suspended,
            0,
        );
        assert_eq!(plan.skip_reason(), Some(HostSkipReason::LifecycleSuspended));
        assert_eq!(plan.steps(), 0);
    }

    #[test]
    fn shutdown_frame_is_skipped_and_drops_accumulator() {
        let shutting_down = visible().apply(HostLifecycleSignal::ShutdownRequested);
        let plan = HostStepPlan::build(
            &HostFrameInput::new(1, STEP_NANOS, vp()),
            &cfg(),
            &shutting_down,
            42,
        );
        assert_eq!(plan.skip_reason(), Some(HostSkipReason::ShutdownRequested));
        assert_eq!(plan.retained_nanos(), 0);
    }

    #[test]
    fn identical_inputs_produce_identical_plans() {
        let a = HostStepPlan::build(
            &HostFrameInput::new(7, STEP_NANOS, vp()),
            &cfg(),
            &visible(),
            123,
        );
        let b = HostStepPlan::build(
            &HostFrameInput::new(7, STEP_NANOS, vp()),
            &cfg(),
            &visible(),
            123,
        );
        assert_eq!(a, b);
        assert_eq!(a.sequence(), 7);
    }

    #[test]
    fn is_skipped_matches_skip_reason() {
        let skipped = HostStepPlan::build(
            &HostFrameInput::new(1, STEP_NANOS, vp()),
            &cfg(),
            &HostLifecycleState::initial(),
            0,
        );
        assert!(skipped.is_skipped());
        let stepped = HostStepPlan::build(
            &HostFrameInput::new(1, STEP_NANOS, vp()),
            &cfg(),
            &visible(),
            0,
        );
        assert!(!stepped.is_skipped());
    }
}

#[cfg(test)]
mod cov {
    use super::*;
    use crate::host_lifecycle_signal::HostLifecycleSignal;
    use crate::host_viewport::HostViewport;
    use axiom_kernel::Ratio;

    fn vp() -> HostViewport {
        HostViewport::new(800, 600, Ratio::new(1.0).unwrap()).unwrap()
    }

    fn visible() -> HostLifecycleState {
        HostLifecycleState::initial().apply(HostLifecycleSignal::Started)
    }

    #[test]
    fn zero_fixed_step_produces_zero_steps() {
        // `fixed_step_nanos == 0` must schedule zero steps, not divide by zero.
        let cfg = HostBoundaryConfig::new(0, 5).unwrap();
        let plan = HostStepPlan::build(
            &HostFrameInput::new(1, 1_000_000, vp()),
            &cfg,
            &visible(),
            0,
        );
        assert_eq!(plan.steps(), 0);
        assert_eq!(plan.consumed_nanos(), 0);
    }

    #[test]
    fn hidden_skip_without_retain_drops_accumulator() {
        // Exercises `retained_after_skip`'s else arm via a hidden skip with a
        // non-retaining config.
        let cfg = HostBoundaryConfig::new(1_000, 5)
            .unwrap()
            .with_retain_accumulator(false);
        let hidden = HostLifecycleState::initial();
        let plan = HostStepPlan::build(&HostFrameInput::new(1, 1_000, vp()), &cfg, &hidden, 5_000);
        assert_eq!(plan.skip_reason(), Some(HostSkipReason::LifecycleHidden));
        assert_eq!(plan.retained_nanos(), 0);
    }
}
