//! Deterministic per-frame timing summary.

use axiom_host::HostFrameReport;

use crate::frame_error::FrameError;
use crate::frame_result::FrameResult;

/// Deterministic per-frame timing summary.
///
/// Built once per engine frame from a [`HostFrameReport`]. Every value is
/// an explicit integer count of nanoseconds (or runtime steps); nothing here
/// is derived from a wall clock. The struct's invariant is that
/// `steps_executed * fixed_step_nanos == consumed_nanos` (when
/// `fixed_step_nanos > 0`) — a mismatch indicates a host-driver bug and is
/// rejected with [`crate::frame_error_code::FrameErrorCode::InvalidFrameTiming`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FrameTiming {
    host_elapsed_nanos: u64,
    consumed_nanos: u64,
    retained_nanos: u64,
    fixed_step_nanos: u64,
    runtime_steps_executed: u32,
    skipped: bool,
}

impl FrameTiming {
    /// Build a timing summary from a host frame report and the boundary's
    /// fixed step (which is **not** carried on the report — the caller
    /// supplies it from the [`axiom_host::HostBoundaryConfig`] it owns).
    pub fn from_host_report(
        report: &HostFrameReport,
        fixed_step_nanos: u64,
    ) -> FrameResult<FrameTiming> {
        let plan = report.plan();
        let consumed_nanos = plan.consumed_nanos();
        let retained_nanos = plan.retained_nanos();
        let runtime_steps_executed = report.steps_executed();
        let skipped = plan.is_skipped();

        // host_elapsed_nanos = consumed + retained; this equals the host's
        // actual elapsed time only when the prior accumulator was zero, but
        // remains a deterministic "frame planning budget" otherwise.
        let host_elapsed_nanos = consumed_nanos.saturating_add(retained_nanos);
        let expected = (runtime_steps_executed as u64).saturating_mul(fixed_step_nanos);
        let timing_is_invalid = (fixed_step_nanos != 0) & (expected != consumed_nanos);

        timing_is_invalid.then_some(()).map_or_else(
            || {
                Ok(FrameTiming {
                    host_elapsed_nanos,
                    consumed_nanos,
                    retained_nanos,
                    fixed_step_nanos,
                    runtime_steps_executed,
                    skipped,
                })
            },
            |()| {
                Err(FrameError::invalid_frame_timing(
                    "frame consumed nanoseconds disagree with steps_executed * fixed_step_nanos",
                ))
            },
        )
    }

    pub const fn host_elapsed_nanos(&self) -> u64 {
        self.host_elapsed_nanos
    }

    pub const fn consumed_nanos(&self) -> u64 {
        self.consumed_nanos
    }

    pub const fn retained_nanos(&self) -> u64 {
        self.retained_nanos
    }

    pub const fn fixed_step_nanos(&self) -> u64 {
        self.fixed_step_nanos
    }

    pub const fn runtime_steps_executed(&self) -> u32 {
        self.runtime_steps_executed
    }

    pub const fn skipped(&self) -> bool {
        self.skipped
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame_error_code::FrameErrorCode;
    use axiom_host::{
        HostBoundaryConfig, HostFrameInput, HostLifecycleSignal, HostLifecycleState, HostStepPlan,
        HostViewport,
    };
    use axiom_kernel::Ratio;

    const STEP_NANOS: u64 = 1_000;

    fn vp() -> HostViewport {
        HostViewport::new(100, 100, Ratio::new(1.0).unwrap()).unwrap()
    }

    fn cfg() -> HostBoundaryConfig {
        HostBoundaryConfig::new(STEP_NANOS, 5).unwrap()
    }

    fn visible() -> HostLifecycleState {
        HostLifecycleState::initial().apply(HostLifecycleSignal::Started)
    }

    fn report_for(
        elapsed: u64,
        accumulator: u64,
        lifecycle: HostLifecycleState,
    ) -> HostFrameReport {
        let input = HostFrameInput::new(1, elapsed, vp());
        let plan = HostStepPlan::build(&input, &cfg(), &lifecycle, accumulator);
        HostFrameReport::new(
            input.sequence(),
            plan,
            plan.steps(),
            Vec::new(),
            vp(),
            lifecycle,
        )
    }

    #[test]
    fn exact_one_step_timing() {
        let r = report_for(STEP_NANOS, 0, visible());
        let t = FrameTiming::from_host_report(&r, STEP_NANOS).unwrap();
        assert_eq!(t.runtime_steps_executed(), 1);
        assert_eq!(t.consumed_nanos(), STEP_NANOS);
        assert_eq!(t.retained_nanos(), 0);
        assert_eq!(t.host_elapsed_nanos(), STEP_NANOS);
        assert_eq!(t.fixed_step_nanos(), STEP_NANOS);
        assert!(!t.skipped());
    }

    #[test]
    fn multi_step_timing() {
        let r = report_for(3 * STEP_NANOS, 0, visible());
        let t = FrameTiming::from_host_report(&r, STEP_NANOS).unwrap();
        assert_eq!(t.runtime_steps_executed(), 3);
        assert_eq!(t.consumed_nanos(), 3 * STEP_NANOS);
    }

    #[test]
    fn max_step_clamped_timing_preserves_retained_nanos() {
        let r = report_for(100 * STEP_NANOS, 0, visible());
        let t = FrameTiming::from_host_report(&r, STEP_NANOS).unwrap();
        assert_eq!(t.runtime_steps_executed(), 5);
        assert_eq!(t.consumed_nanos(), 5 * STEP_NANOS);
        assert_eq!(t.retained_nanos(), 95 * STEP_NANOS);
    }

    #[test]
    fn retain_accumulator_timing_reports_carryover() {
        let r = report_for(STEP_NANOS / 2, 0, visible());
        let t = FrameTiming::from_host_report(&r, STEP_NANOS).unwrap();
        assert_eq!(t.runtime_steps_executed(), 0);
        assert_eq!(t.consumed_nanos(), 0);
        assert_eq!(t.retained_nanos(), STEP_NANOS / 2);
    }

    #[test]
    fn skipped_frame_timing_marks_skipped() {
        let r = report_for(STEP_NANOS, 0, HostLifecycleState::initial());
        let t = FrameTiming::from_host_report(&r, STEP_NANOS).unwrap();
        assert!(t.skipped());
        assert_eq!(t.runtime_steps_executed(), 0);
        assert_eq!(t.consumed_nanos(), 0);
    }

    #[test]
    fn identical_input_produces_identical_timing() {
        let a =
            FrameTiming::from_host_report(&report_for(2 * STEP_NANOS, 0, visible()), STEP_NANOS)
                .unwrap();
        let b =
            FrameTiming::from_host_report(&report_for(2 * STEP_NANOS, 0, visible()), STEP_NANOS)
                .unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn mismatched_steps_executed_is_rejected_as_invalid_timing() {
        let input = HostFrameInput::new(1, STEP_NANOS, vp());
        let plan = HostStepPlan::build(&input, &cfg(), &visible(), 0);
        let mismatched = HostFrameReport::new(
            input.sequence(),
            plan,
            0,
            Vec::new(),
            vp(),
            visible(),
        );
        let err = FrameTiming::from_host_report(&mismatched, STEP_NANOS).unwrap_err();
        assert_eq!(err.code(), FrameErrorCode::InvalidFrameTiming);
    }
}

#[cfg(test)]
mod cov {
    use super::*;
    use axiom_host::{
        HostBoundaryConfig, HostFrameInput, HostFrameReport, HostLifecycleSignal,
        HostLifecycleState, HostStepPlan, HostViewport,
    };
    use axiom_kernel::Ratio;

    fn report() -> HostFrameReport {
        let vp = HostViewport::new(100, 100, Ratio::new(1.0).unwrap()).unwrap();
        let cfg = HostBoundaryConfig::new(1_000, 5).unwrap();
        let lifecycle = HostLifecycleState::initial().apply(HostLifecycleSignal::Started);
        let input = HostFrameInput::new(1, 1_000, vp);
        let plan = HostStepPlan::build(&input, &cfg, &lifecycle, 0);
        HostFrameReport::new(
            input.sequence(),
            plan,
            plan.steps(),
            Vec::new(),
            vp,
            lifecycle,
        )
    }

    #[test]
    fn mismatched_fixed_step_is_rejected() {
        let err = FrameTiming::from_host_report(&report(), 1_001).unwrap_err();
        assert_eq!(
            err.code(),
            crate::frame_error_code::FrameErrorCode::InvalidFrameTiming
        );
    }

    #[test]
    fn zero_fixed_step_skips_the_consistency_check() {
        assert!(FrameTiming::from_host_report(&report(), 0).is_ok());
    }
}
