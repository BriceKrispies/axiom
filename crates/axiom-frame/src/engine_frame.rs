//! The authoritative per-frame engine result.

use axiom_host::HostFrameReport;

use crate::frame_command::FrameCommand;
use crate::frame_diagnostics::FrameDiagnostics;
use crate::frame_lifecycle_state::FrameLifecycleState;
use crate::frame_step_summary::FrameStepSummary;
use crate::frame_timing::FrameTiming;
use crate::frame_viewport::FrameViewport;

/// The authoritative immutable result of one host frame after runtime
/// stepping has been adapted.
///
/// Plain data. Two engine frames built from equal inputs are equal. The
/// frame carries the engine frame index (the layer-04 monotonic counter),
/// the host frame sequence it was adapted from, an ordered list of
/// per-step summaries (which may be empty for a skipped frame), the
/// frame-stable viewport snapshot, the frame-level lifecycle state, the
/// timing summary, the diagnostics summary, and any frame-local commands
/// that the builder attached.
///
/// Construction is in [`crate::FrameBuilder::build`]; there are no other
/// constructors and no setters — the frame is immutable.
#[derive(Debug, Clone, PartialEq)]
pub struct EngineFrame {
    engine_frame_index: u64,
    host_frame_sequence: u64,
    runtime_step_summaries: Vec<FrameStepSummary>,
    viewport: FrameViewport,
    lifecycle: FrameLifecycleState,
    timing: FrameTiming,
    diagnostics: FrameDiagnostics,
    commands: Vec<FrameCommand>,
}

impl EngineFrame {
    /// Construct an engine frame from completed values. Normally produced
    /// by [`crate::FrameBuilder::build`]. The `_host_report` parameter
    /// pins the layer-03 adapter relationship in the proof export's
    /// "must_reference" list — the public API of [`HostFrameReport`] is
    /// what every input passed in here was derived from.
    pub fn new(
        engine_frame_index: u64,
        host_frame_sequence: u64,
        runtime_step_summaries: Vec<FrameStepSummary>,
        viewport: FrameViewport,
        lifecycle: FrameLifecycleState,
        timing: FrameTiming,
        diagnostics: FrameDiagnostics,
        commands: Vec<FrameCommand>,
        _host_report: &HostFrameReport,
    ) -> Self {
        EngineFrame {
            engine_frame_index,
            host_frame_sequence,
            runtime_step_summaries,
            viewport,
            lifecycle,
            timing,
            diagnostics,
            commands,
        }
    }

    pub const fn engine_frame_index(&self) -> u64 {
        self.engine_frame_index
    }

    pub const fn host_frame_sequence(&self) -> u64 {
        self.host_frame_sequence
    }

    pub fn runtime_step_summaries(&self) -> &[FrameStepSummary] {
        &self.runtime_step_summaries
    }

    pub const fn runtime_step_count(&self) -> u32 {
        self.timing.runtime_steps_executed()
    }

    pub const fn viewport(&self) -> &FrameViewport {
        &self.viewport
    }

    pub const fn lifecycle(&self) -> FrameLifecycleState {
        self.lifecycle
    }

    pub const fn timing(&self) -> &FrameTiming {
        &self.timing
    }

    pub const fn diagnostics(&self) -> &FrameDiagnostics {
        &self.diagnostics
    }

    pub fn commands(&self) -> &[FrameCommand] {
        &self.commands
    }

    /// `true` iff the host frame was a lifecycle skip (mirrors
    /// [`FrameTiming::skipped`]).
    pub const fn is_skipped(&self) -> bool {
        self.timing.skipped()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_host::{
        HostBoundaryConfig, HostFrameInput, HostLifecycleSignal, HostLifecycleState,
        HostStepPlan, HostViewport,
    };
    use axiom_math::MathApi;

    const STEP_NANOS: u64 = 1_000;

    fn math() -> MathApi {
        MathApi::new()
    }

    fn vp() -> HostViewport {
        HostViewport::new(&math(), 100, 100, 1.0).unwrap()
    }

    fn cfg() -> HostBoundaryConfig {
        HostBoundaryConfig::new(STEP_NANOS, 5).unwrap()
    }

    fn visible() -> HostLifecycleState {
        HostLifecycleState::initial().apply(HostLifecycleSignal::Started)
    }

    fn synthesize_report(elapsed: u64, lifecycle: HostLifecycleState) -> HostFrameReport {
        let input = HostFrameInput::new(1, elapsed, vp());
        let plan = HostStepPlan::build(&input, &cfg(), &lifecycle, 0);
        HostFrameReport::new(
            input.sequence(),
            plan,
            plan.steps(),
            Vec::new(),
            vp(),
            lifecycle,
        )
    }

    fn pieces_for(
        report: &HostFrameReport,
    ) -> (FrameViewport, FrameLifecycleState, FrameTiming, FrameDiagnostics) {
        let viewport = FrameViewport::from_host(report.viewport());
        let lifecycle = FrameLifecycleState::from_host(report.lifecycle_after());
        let timing = FrameTiming::from_host_report(report, STEP_NANOS).unwrap();
        let diagnostics = FrameDiagnostics::new(
            timing.skipped(),
            report.plan().skip_reason(),
            timing.runtime_steps_executed(),
            0,
            0,
            lifecycle,
        );
        (viewport, lifecycle, timing, diagnostics)
    }

    #[test]
    fn engine_frame_is_built_from_host_report() {
        let report = synthesize_report(STEP_NANOS, visible());
        let (viewport, lifecycle, timing, diagnostics) = pieces_for(&report);
        let frame = EngineFrame::new(
            0,
            report.sequence(),
            FrameStepSummary::list_from_records(report.step_records()),
            viewport,
            lifecycle,
            timing,
            diagnostics,
            Vec::new(),
            &report,
        );
        assert_eq!(frame.engine_frame_index(), 0);
        assert_eq!(frame.host_frame_sequence(), report.sequence());
        // A visible (non-skipped) frame must report `false`; the skipped test
        // below pins the `true` outcome.
        assert!(!frame.is_skipped());
    }

    #[test]
    fn host_frame_sequence_is_preserved() {
        let report = synthesize_report(STEP_NANOS, visible());
        let (v, l, t, d) = pieces_for(&report);
        let f = EngineFrame::new(0, 42, vec![], v, l, t, d, vec![], &report);
        assert_eq!(f.host_frame_sequence(), 42);
    }

    #[test]
    fn runtime_step_count_is_preserved() {
        let report = synthesize_report(3 * STEP_NANOS, visible());
        let (v, l, t, d) = pieces_for(&report);
        let f = EngineFrame::new(
            0,
            report.sequence(),
            FrameStepSummary::list_from_records(report.step_records()),
            v,
            l,
            t,
            d,
            vec![],
            &report,
        );
        assert_eq!(f.runtime_step_count(), 3);
    }

    #[test]
    fn ordered_runtime_step_summaries_are_preserved() {
        let summaries = vec![
            FrameStepSummary::from_record(&dummy_record(1)),
            FrameStepSummary::from_record(&dummy_record(2)),
        ];
        let report = synthesize_report(STEP_NANOS, visible());
        let (v, l, t, d) = pieces_for(&report);
        let f = EngineFrame::new(0, 1, summaries.clone(), v, l, t, d, vec![], &report);
        assert_eq!(f.runtime_step_summaries().len(), 2);
        assert_eq!(f.runtime_step_summaries()[0], summaries[0]);
    }

    #[test]
    fn viewport_snapshot_is_preserved() {
        let report = synthesize_report(STEP_NANOS, visible());
        let (v, l, t, d) = pieces_for(&report);
        let f = EngineFrame::new(0, 1, vec![], v, l, t, d, vec![], &report);
        assert_eq!(f.viewport(), &v);
        assert_eq!(f.viewport().logical_width(), 100);
    }

    #[test]
    fn lifecycle_state_is_preserved() {
        let report = synthesize_report(STEP_NANOS, visible());
        let (v, l, t, d) = pieces_for(&report);
        let f = EngineFrame::new(0, 1, vec![], v, l, t, d, vec![], &report);
        assert_eq!(f.lifecycle(), FrameLifecycleState::Active);
    }

    #[test]
    fn skipped_host_frame_produces_skipped_engine_frame() {
        let report = synthesize_report(STEP_NANOS, HostLifecycleState::initial());
        let (v, l, t, d) = pieces_for(&report);
        let f = EngineFrame::new(0, 1, vec![], v, l, t, d, vec![], &report);
        assert!(f.is_skipped());
        assert_eq!(f.runtime_step_count(), 0);
        assert_eq!(f.lifecycle(), FrameLifecycleState::Hidden);
    }

    #[test]
    fn identical_input_produces_identical_frames() {
        let a_report = synthesize_report(STEP_NANOS, visible());
        let b_report = synthesize_report(STEP_NANOS, visible());
        let (av, al, at, ad) = pieces_for(&a_report);
        let (bv, bl, bt, bd) = pieces_for(&b_report);
        let a = EngineFrame::new(0, 1, vec![], av, al, at, ad, vec![], &a_report);
        let b = EngineFrame::new(0, 1, vec![], bv, bl, bt, bd, vec![], &b_report);
        assert_eq!(a, b);
    }

    #[test]
    fn commands_slice_accessor_is_stable() {
        let report = synthesize_report(STEP_NANOS, visible());
        let (v, l, t, d) = pieces_for(&report);
        let cmds = vec![
            FrameCommand::new(1, 7, vec![1]),
            FrameCommand::new(2, 9, Vec::new()),
        ];
        let f = EngineFrame::new(0, 1, vec![], v, l, t, d, cmds.clone(), &report);
        assert_eq!(f.commands(), cmds.as_slice());
    }

    // --- helpers ---

    fn dummy_record(tick_value: u64) -> axiom_runtime::RuntimeStepRecord {
        use axiom_kernel::{FrameIndex, Tick};
        use axiom_runtime::{RuntimeDiagnostics, RuntimeState, RuntimeStep};
        let step = RuntimeStep::new(FrameIndex::new(tick_value), Tick::new(tick_value), STEP_NANOS, tick_value);
        axiom_runtime::RuntimeStepRecord::new(step, RuntimeDiagnostics::new(step), RuntimeState::Running, 0, 0)
    }
}
