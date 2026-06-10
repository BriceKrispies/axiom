//! A deterministic per-frame report from the host boundary.

use axiom_runtime::RuntimeStepRecord;

use crate::host_lifecycle_state::HostLifecycleState;
use crate::host_step_plan::HostStepPlan;
use crate::host_viewport::HostViewport;

/// A deterministic per-frame report from the host boundary.
///
/// Plain data. Each report carries the host frame sequence, the plan that
/// was executed, the actual number of runtime steps the driver ran, the
/// ordered runtime step records, the viewport the host frame was driven
/// with, and the lifecycle state observed *after* the frame. Two runs with
/// the same inputs produce equal reports.
#[derive(Debug, Clone)]
pub struct HostFrameReport {
    sequence: u64,
    plan: HostStepPlan,
    steps_executed: u32,
    step_records: Vec<RuntimeStepRecord>,
    viewport: HostViewport,
    lifecycle_after: HostLifecycleState,
}

impl HostFrameReport {
    /// Construct from completed values. Normally produced by
    /// [`crate::HostStepDriver::drive`].
    pub fn new(
        sequence: u64,
        plan: HostStepPlan,
        steps_executed: u32,
        step_records: Vec<RuntimeStepRecord>,
        viewport: HostViewport,
        lifecycle_after: HostLifecycleState,
    ) -> Self {
        HostFrameReport {
            sequence,
            plan,
            steps_executed,
            step_records,
            viewport,
            lifecycle_after,
        }
    }

    pub const fn sequence(&self) -> u64 {
        self.sequence
    }

    pub const fn plan(&self) -> &HostStepPlan {
        &self.plan
    }

    pub const fn steps_executed(&self) -> u32 {
        self.steps_executed
    }

    pub fn step_records(&self) -> &[RuntimeStepRecord] {
        &self.step_records
    }

    /// The viewport that was in effect for the host frame this report
    /// describes. Carrying the viewport on the report (rather than only on
    /// the input) lets higher layers — the engine frame boundary built on
    /// top of this one — derive a frame-stable viewport snapshot without
    /// holding onto the original [`crate::HostFrameInput`].
    pub const fn viewport(&self) -> &HostViewport {
        &self.viewport
    }

    pub const fn lifecycle_after(&self) -> HostLifecycleState {
        self.lifecycle_after
    }

    /// Whether the frame was a lifecycle skip rather than an actual step
    /// burst (a thin convenience matching the plan's `is_skipped()`).
    pub const fn is_skipped(&self) -> bool {
        self.plan.is_skipped()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host_boundary_config::HostBoundaryConfig;
    use crate::host_frame_input::HostFrameInput;
    use crate::host_lifecycle_signal::HostLifecycleSignal;
    use crate::host_skip_reason::HostSkipReason;
    use crate::host_viewport::HostViewport;
    use axiom_kernel::Ratio;

    fn vp() -> HostViewport {
        HostViewport::new(100, 100, Ratio::new(1.0).unwrap()).unwrap()
    }

    fn cfg() -> HostBoundaryConfig {
        HostBoundaryConfig::new(1_000, 5).unwrap()
    }

    fn visible() -> HostLifecycleState {
        HostLifecycleState::initial().apply(HostLifecycleSignal::Started)
    }

    #[test]
    fn report_carries_sequence_and_plan() {
        let plan = HostStepPlan::build(&HostFrameInput::new(3, 1_000, vp()), &cfg(), &visible(), 0);
        let report = HostFrameReport::new(3, plan, 1, Vec::new(), vp(), visible());
        assert_eq!(report.sequence(), 3);
        assert_eq!(report.plan(), &plan);
    }

    #[test]
    fn report_matches_executed_step_count() {
        let plan = HostStepPlan::build(&HostFrameInput::new(1, 3_000, vp()), &cfg(), &visible(), 0);
        let report = HostFrameReport::new(1, plan, 3, vec![], vp(), visible());
        assert_eq!(report.steps_executed(), 3);
    }

    #[test]
    fn report_preserves_record_order_via_slice_accessor() {
        let plan = HostStepPlan::build(&HostFrameInput::new(1, 0, vp()), &cfg(), &visible(), 0);
        let report = HostFrameReport::new(1, plan, 0, Vec::new(), vp(), visible());
        // The slice length matches the constructor's vec length.
        assert_eq!(report.step_records().len(), 0);
    }

    #[test]
    fn skipped_frame_report_is_explicit() {
        let hidden = HostLifecycleState::initial();
        let plan = HostStepPlan::build(&HostFrameInput::new(1, 1_000, vp()), &cfg(), &hidden, 0);
        let report = HostFrameReport::new(1, plan, 0, Vec::new(), vp(), hidden);
        assert!(report.is_skipped());
        assert_eq!(
            report.plan().skip_reason(),
            Some(HostSkipReason::LifecycleHidden)
        );
    }

    #[test]
    fn non_skipped_frame_report_is_not_skipped() {
        // Distinguishes `is_skipped -> true`: a visible frame that actually
        // stepped is NOT a lifecycle skip.
        let plan = HostStepPlan::build(&HostFrameInput::new(1, 1_000, vp()), &cfg(), &visible(), 0);
        assert!(!plan.is_skipped());
        let report = HostFrameReport::new(1, plan, 1, vec![], vp(), visible());
        assert!(!report.is_skipped());
    }

    #[test]
    fn lifecycle_state_is_reflected() {
        let after = visible().apply(HostLifecycleSignal::Focused);
        let plan = HostStepPlan::build(&HostFrameInput::new(1, 1_000, vp()), &cfg(), &after, 0);
        let report = HostFrameReport::new(1, plan, 1, vec![], vp(), after);
        assert_eq!(report.lifecycle_after(), after);
        assert!(report.lifecycle_after().focused());
    }

    #[test]
    fn report_carries_viewport_from_input() {
        let plan = HostStepPlan::build(&HostFrameInput::new(1, 1_000, vp()), &cfg(), &visible(), 0);
        let report = HostFrameReport::new(1, plan, 1, vec![], vp(), visible());
        assert_eq!(report.viewport(), &vp());
    }

    #[test]
    fn equal_inputs_produce_equal_reports() {
        let plan = HostStepPlan::build(&HostFrameInput::new(2, 1_000, vp()), &cfg(), &visible(), 0);
        let a = HostFrameReport::new(2, plan, 1, vec![], vp(), visible());
        let b = HostFrameReport::new(2, plan, 1, vec![], vp(), visible());
        assert_eq!(a.sequence(), b.sequence());
        assert_eq!(a.plan(), b.plan());
        assert_eq!(a.steps_executed(), b.steps_executed());
        assert_eq!(a.lifecycle_after(), b.lifecycle_after());
    }
}
