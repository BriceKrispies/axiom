//! The Layer-04 engine frame boundary facade.

use axiom_host::{HostFrameReport, HostLifecycleState, HostViewport};

use crate::engine_frame::EngineFrame;
use crate::frame_builder::FrameBuilder;
use crate::frame_command::FrameCommand;
use crate::frame_command_queue::FrameCommandQueue;
use crate::frame_context::FrameContext;
use crate::frame_diagnostics::FrameDiagnostics;
use crate::frame_lifecycle_state::FrameLifecycleState;
use crate::frame_result::FrameResult;
use crate::frame_timing::FrameTiming;
use crate::frame_viewport::FrameViewport;

/// The primary entry point to the Axiom engine frame boundary.
///
/// `FrameApi` is a zero-sized facade. It is the only place future engine
/// systems should reach for to construct frame-boundary values: builders,
/// command queues, viewports, lifecycle projections, timing summaries,
/// diagnostics, and the read-only `FrameContext`. Frame-boundary math
/// validation flows through [`MathApi::validate_finite`] inside the helpers
/// the facade delegates to, which is what makes this facade a real
/// Layer-04 semantic adapter over both Layer-02 math and Layer-03 host.
#[derive(Debug, Clone, Copy, Default)]
pub struct FrameApi {
    _sealed: (),
}

impl FrameApi {
    /// Construct the facade.
    pub const fn new() -> Self {
        FrameApi { _sealed: () }
    }

    // --- Builders / queues ---

    /// Construct a fresh [`FrameBuilder`] paired with the same fixed step
    /// the host boundary uses.
    pub fn frame_builder(&self, fixed_step_nanos: u64) -> FrameBuilder {
        FrameBuilder::new(fixed_step_nanos)
    }

    /// Construct an empty deterministic frame command queue.
    pub fn command_queue(&self) -> FrameCommandQueue {
        FrameCommandQueue::new()
    }

    // --- One-shot construction helpers (no builder state) ---

    /// Adapt a single [`HostFrameReport`] into a complete [`EngineFrame`]
    /// using a fresh builder. Use this when the caller does not need to
    /// keep builder state across frames; for multi-frame use, call
    /// [`Self::frame_builder`] and reuse the builder.
    pub fn engine_frame_from_host_report(
        &self,
        report: &HostFrameReport,
        fixed_step_nanos: u64,
        commands: Vec<FrameCommand>,
    ) -> FrameResult<EngineFrame> {
        FrameBuilder::new(fixed_step_nanos).build(report, commands)
    }

    /// Borrow an [`EngineFrame`] as a [`FrameContext`].
    pub const fn frame_context<'a>(&self, frame: &'a EngineFrame) -> FrameContext<'a> {
        FrameContext::new(frame)
    }

    // --- Snapshots / projections from host data ---

    /// Construct a [`FrameViewport`] snapshot from a host viewport, routing
    /// finite-aspect validation through [`MathApi`].
    pub fn frame_viewport(&self, viewport: &HostViewport) -> FrameViewport {
        FrameViewport::from_host(viewport)
    }

    /// Project a host lifecycle state onto the four frame-level states.
    pub const fn frame_lifecycle(&self, state: HostLifecycleState) -> FrameLifecycleState {
        FrameLifecycleState::from_host(state)
    }

    /// Construct a [`FrameTiming`] summary directly from a host report.
    pub fn frame_timing(
        &self,
        report: &HostFrameReport,
        fixed_step_nanos: u64,
    ) -> FrameResult<FrameTiming> {
        FrameTiming::from_host_report(report, fixed_step_nanos)
    }

    /// Build a [`FrameDiagnostics`] summary directly. The caller supplies
    /// the counts that are not derivable from a host report alone (the
    /// frame-local command count, the frame-local validation failure
    /// count).
    pub const fn frame_diagnostics(
        &self,
        skipped: bool,
        skip_reason: Option<axiom_host::HostSkipReason>,
        runtime_step_count: u32,
        command_count: u32,
        validation_failure_count: u32,
        lifecycle: FrameLifecycleState,
    ) -> FrameDiagnostics {
        FrameDiagnostics::new(
            skipped,
            skip_reason,
            runtime_step_count,
            command_count,
            validation_failure_count,
            lifecycle,
        )
    }

    /// Validate that `next` is a strict successor of `previous` in host
    /// frame sequence. Returns `Ok(())` or an
    /// `InvalidHostFrameSequence` error.
    pub fn validate_host_frame_transition(
        &self,
        previous: u64,
        next: u64,
    ) -> FrameResult<()> {
        if next > previous {
            Ok(())
        } else {
            Err(crate::frame_error::FrameError::invalid_host_frame_sequence(
                "host frame sequence must strictly increase",
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame_error_code::FrameErrorCode;
    use axiom_host::{
        HostBoundaryConfig, HostFrameInput, HostLifecycleSignal, HostSkipReason, HostStepPlan,
    };
    use axiom_math::MathApi;

    const STEP_NANOS: u64 = 1_000;

    fn math() -> MathApi {
        MathApi::new()
    }

    fn host_vp() -> HostViewport {
        HostViewport::new(&math(), 100, 100, 1.0).unwrap()
    }

    fn cfg() -> HostBoundaryConfig {
        HostBoundaryConfig::new(STEP_NANOS, 5).unwrap()
    }

    fn visible() -> HostLifecycleState {
        HostLifecycleState::initial().apply(HostLifecycleSignal::Started)
    }

    fn synthesize_report(elapsed: u64, lifecycle: HostLifecycleState) -> HostFrameReport {
        let input = HostFrameInput::new(1, elapsed, host_vp());
        let plan = HostStepPlan::build(&input, &cfg(), &lifecycle, 0);
        HostFrameReport::new(
            input.sequence(),
            plan,
            plan.steps(),
            Vec::new(),
            host_vp(),
            lifecycle,
        )
    }

    fn api() -> FrameApi {
        FrameApi::new()
    }

    #[test]
    fn new_and_default_are_equivalent() {
        // Both construction paths yield a facade that builds an identical queue.
        let mut from_new = FrameApi::new().command_queue();
        let mut from_default = FrameApi::default().command_queue();
        assert_eq!(from_new.push(7, vec![1]), from_default.push(7, vec![1]));
        assert_eq!(from_new.len(), from_default.len());
    }

    #[test]
    fn frame_builder_round_trips_through_facade() {
        let mut b = api().frame_builder(STEP_NANOS);
        let r = synthesize_report(STEP_NANOS, visible());
        let f = b.build(&r, Vec::new()).unwrap();
        assert_eq!(f.engine_frame_index(), 0);
        assert_eq!(f.runtime_step_count(), 1);
    }

    #[test]
    fn command_queue_round_trips_through_facade() {
        let mut q = api().command_queue();
        assert!(q.is_empty());
        let s = q.push(7, vec![1]);
        assert_eq!(s, 1);
        assert_eq!(q.len(), 1);
    }

    #[test]
    fn engine_frame_from_host_report_builds_a_complete_frame() {
        let r = synthesize_report(STEP_NANOS, visible());
        let f = api()
            .engine_frame_from_host_report(&r, STEP_NANOS, Vec::new())
            .unwrap();
        assert_eq!(f.engine_frame_index(), 0);
        assert_eq!(f.host_frame_sequence(), 1);
    }

    #[test]
    fn frame_context_borrows_an_engine_frame() {
        let r = synthesize_report(STEP_NANOS, visible());
        let f = api()
            .engine_frame_from_host_report(&r, STEP_NANOS, Vec::new())
            .unwrap();
        let ctx = api().frame_context(&f);
        assert_eq!(ctx.host_frame_sequence(), f.host_frame_sequence());
    }

    #[test]
    fn frame_viewport_projects_host_viewport() {
        let v = api().frame_viewport(&host_vp());
        assert_eq!(v.logical_width(), 100);
        assert!(v.aspect_ratio().is_finite());
    }

    #[test]
    fn frame_lifecycle_projects_host_state() {
        assert_eq!(
            api().frame_lifecycle(visible()),
            FrameLifecycleState::Active
        );
        assert_eq!(
            api().frame_lifecycle(HostLifecycleState::initial()),
            FrameLifecycleState::Hidden
        );
    }

    #[test]
    fn frame_timing_round_trips_through_facade() {
        let r = synthesize_report(2 * STEP_NANOS, visible());
        let t = api().frame_timing(&r, STEP_NANOS).unwrap();
        assert_eq!(t.runtime_steps_executed(), 2);
    }

    #[test]
    fn frame_diagnostics_round_trips_through_facade() {
        let d = api().frame_diagnostics(
            true,
            Some(HostSkipReason::LifecycleSuspended),
            0,
            0,
            0,
            FrameLifecycleState::Suspended,
        );
        assert!(d.skipped());
        assert_eq!(d.skip_reason(), Some(HostSkipReason::LifecycleSuspended));
    }

    #[test]
    fn validate_host_frame_transition_accepts_strict_increase() {
        assert!(api().validate_host_frame_transition(3, 4).is_ok());
    }

    #[test]
    fn validate_host_frame_transition_rejects_equal_and_decreasing() {
        assert_eq!(
            api()
                .validate_host_frame_transition(3, 3)
                .unwrap_err()
                .code(),
            FrameErrorCode::InvalidHostFrameSequence
        );
        assert_eq!(
            api()
                .validate_host_frame_transition(3, 2)
                .unwrap_err()
                .code(),
            FrameErrorCode::InvalidHostFrameSequence
        );
    }

    #[test]
    fn facade_can_adapt_a_skipped_host_frame() {
        let r = synthesize_report(STEP_NANOS, HostLifecycleState::initial());
        let f = api()
            .engine_frame_from_host_report(&r, STEP_NANOS, Vec::new())
            .unwrap();
        assert!(f.is_skipped());
        assert_eq!(f.lifecycle(), FrameLifecycleState::Hidden);
    }
}
