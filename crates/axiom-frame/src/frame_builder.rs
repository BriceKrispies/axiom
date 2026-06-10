//! Deterministic construction helper for [`EngineFrame`].

use axiom_host::HostFrameReport;

use crate::engine_frame::EngineFrame;
use crate::frame_command::FrameCommand;
use crate::frame_diagnostics::FrameDiagnostics;
use crate::frame_error::FrameError;
use crate::frame_lifecycle_state::FrameLifecycleState;
use crate::frame_result::FrameResult;
use crate::frame_step_summary::FrameStepSummary;
use crate::frame_timing::FrameTiming;
use crate::frame_viewport::FrameViewport;

/// Deterministic construction helper for [`EngineFrame`].
///
/// Owns the engine-frame monotonic counter and the last accepted host
/// frame sequence so it can reject out-of-order host reports. Construction
/// is otherwise pure: feeding the same `HostFrameReport` and the same
/// `commands` into two builders that started in the same state produces
/// byte-identical `EngineFrame`s.
///
/// This is **not** a plugin host, a framework, or an async pipeline. It is
/// a small adapter from a single [`HostFrameReport`] to a single
/// [`EngineFrame`].
#[derive(Debug, Clone)]
pub struct FrameBuilder {
    next_engine_frame_index: u64,
    last_host_sequence: Option<u64>,
    fixed_step_nanos: u64,
}

impl FrameBuilder {
    /// Build a fresh builder with `fixed_step_nanos` matching the host
    /// boundary config the reports come from. The next engine frame index
    /// assigned will be `0`.
    pub fn new(fixed_step_nanos: u64) -> Self {
        FrameBuilder {
            next_engine_frame_index: 0,
            last_host_sequence: None,
            fixed_step_nanos,
        }
    }

    /// The engine frame index the next successful `build` call will assign.
    pub const fn next_engine_frame_index(&self) -> u64 {
        self.next_engine_frame_index
    }

    /// The last host frame sequence the builder accepted, or `None` if no
    /// frame has been built yet.
    pub const fn last_host_sequence(&self) -> Option<u64> {
        self.last_host_sequence
    }

    pub const fn fixed_step_nanos(&self) -> u64 {
        self.fixed_step_nanos
    }

    /// Build an engine frame from a host frame report and an ordered list
    /// of frame-local commands.
    ///
    /// Failure paths:
    /// - `InvalidHostFrameSequence` — `report.sequence()` did not strictly
    ///   increase over `last_host_sequence`.
    /// - `InvalidFrameTiming` — the host report's `steps_executed` did
    ///   not match the plan (propagated from
    ///   [`FrameTiming::from_host_report`]).
    #[axiom_zones::sim]
    pub fn build(
        &mut self,
        report: &HostFrameReport,
        commands: Vec<FrameCommand>,
    ) -> FrameResult<EngineFrame> {
        if let Some(last) = self.last_host_sequence {
            if report.sequence() <= last {
                return Err(FrameError::invalid_host_frame_sequence(
                    "host frame sequence did not strictly increase",
                ));
            }
        }

        let viewport = FrameViewport::from_host(report.viewport());
        let lifecycle = FrameLifecycleState::from_host(report.lifecycle_after());
        let timing = FrameTiming::from_host_report(report, self.fixed_step_nanos)?;
        let summaries = FrameStepSummary::list_from_records(report.step_records());
        let command_count = commands.len().min(u32::MAX as usize) as u32;
        let diagnostics = FrameDiagnostics::new(
            timing.skipped(),
            report.plan().skip_reason(),
            timing.runtime_steps_executed(),
            command_count,
            0,
            lifecycle,
        );
        let engine_frame_index = self.next_engine_frame_index;
        self.next_engine_frame_index = self.next_engine_frame_index.saturating_add(1);
        self.last_host_sequence = Some(report.sequence());

        Ok(EngineFrame::new(
            engine_frame_index,
            report.sequence(),
            summaries,
            viewport,
            lifecycle,
            timing,
            diagnostics,
            commands,
            report,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame_error_code::FrameErrorCode;
    use axiom_host::{
        HostBoundaryConfig, HostFrameInput, HostFrameReport, HostLifecycleSignal,
        HostLifecycleState, HostStepPlan, HostViewport,
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

    fn report(sequence: u64, elapsed: u64, lifecycle: HostLifecycleState) -> HostFrameReport {
        let input = HostFrameInput::new(sequence, elapsed, vp());
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

    #[test]
    fn builder_produces_a_complete_engine_frame() {
        let mut b = FrameBuilder::new(STEP_NANOS);
        let r = report(1, STEP_NANOS, visible());
        let f = b.build(&r, Vec::new()).unwrap();
        assert_eq!(f.engine_frame_index(), 0);
        assert_eq!(f.host_frame_sequence(), 1);
        assert_eq!(f.runtime_step_count(), 1);
        assert_eq!(f.viewport().logical_width(), 100);
        assert_eq!(f.lifecycle(), FrameLifecycleState::Active);
        assert_eq!(f.timing().consumed_nanos(), STEP_NANOS);
        assert!(!f.diagnostics().skipped());
        assert!(f.commands().is_empty());
    }

    #[test]
    fn builder_assigns_monotonic_engine_frame_indices() {
        let mut b = FrameBuilder::new(STEP_NANOS);
        let f0 = b.build(&report(1, STEP_NANOS, visible()), vec![]).unwrap();
        let f1 = b.build(&report(2, STEP_NANOS, visible()), vec![]).unwrap();
        let f2 = b.build(&report(3, STEP_NANOS, visible()), vec![]).unwrap();
        assert_eq!(
            (
                f0.engine_frame_index(),
                f1.engine_frame_index(),
                f2.engine_frame_index()
            ),
            (0, 1, 2)
        );
    }

    #[test]
    fn builder_preserves_host_and_runtime_ordering() {
        let mut b = FrameBuilder::new(STEP_NANOS);
        let r = report(5, 3 * STEP_NANOS, visible());
        let f = b.build(&r, Vec::new()).unwrap();
        assert_eq!(f.host_frame_sequence(), 5);
        assert_eq!(f.runtime_step_count(), 3);
    }

    #[test]
    fn builder_handles_skipped_frames() {
        let mut b = FrameBuilder::new(STEP_NANOS);
        let f = b
            .build(
                &report(1, STEP_NANOS, HostLifecycleState::initial()),
                vec![],
            )
            .unwrap();
        assert!(f.is_skipped());
        assert_eq!(f.runtime_step_count(), 0);
        assert_eq!(f.lifecycle(), FrameLifecycleState::Hidden);
    }

    #[test]
    fn builder_attaches_frame_commands() {
        let mut b = FrameBuilder::new(STEP_NANOS);
        let cmds = vec![
            FrameCommand::new(1, 7, vec![1, 2]),
            FrameCommand::new(2, 8, Vec::new()),
        ];
        let f = b
            .build(&report(1, STEP_NANOS, visible()), cmds.clone())
            .unwrap();
        assert_eq!(f.commands(), cmds.as_slice());
        assert_eq!(f.diagnostics().command_count(), 2);
    }

    #[test]
    fn builder_rejects_invalid_host_sequence() {
        let mut b = FrameBuilder::new(STEP_NANOS);
        b.build(&report(5, STEP_NANOS, visible()), vec![]).unwrap();
        let err = b
            .build(&report(5, STEP_NANOS, visible()), vec![])
            .unwrap_err();
        assert_eq!(err.code(), FrameErrorCode::InvalidHostFrameSequence);

        let err = b
            .build(&report(4, STEP_NANOS, visible()), vec![])
            .unwrap_err();
        assert_eq!(err.code(), FrameErrorCode::InvalidHostFrameSequence);
    }

    #[test]
    fn builder_propagates_invalid_timing() {
        // Hand-craft a report whose steps_executed disagrees with the
        // plan, exercising FrameTiming's consistency check through the
        // builder.
        let input = HostFrameInput::new(1, STEP_NANOS, vp());
        let plan = HostStepPlan::build(&input, &cfg(), &visible(), 0);
        let mismatched = HostFrameReport::new(
            input.sequence(),
            plan,
            0, // wrong: plan.steps() == 1
            Vec::new(),
            vp(),
            visible(),
        );
        let mut b = FrameBuilder::new(STEP_NANOS);
        let err = b.build(&mismatched, Vec::new()).unwrap_err();
        assert_eq!(err.code(), FrameErrorCode::InvalidFrameTiming);
    }

    #[test]
    fn repeated_builder_use_with_identical_input_is_deterministic() {
        let make = || {
            let mut b = FrameBuilder::new(STEP_NANOS);
            let mut frames = Vec::new();
            for seq in [1u64, 2, 3] {
                frames.push(
                    b.build(&report(seq, STEP_NANOS, visible()), vec![])
                        .unwrap(),
                );
            }
            frames
        };
        let a = make();
        let b = make();
        assert_eq!(a, b);
    }

    #[test]
    fn accessors_advance_after_builds() {
        let mut b = FrameBuilder::new(STEP_NANOS);
        b.build(&report(7, STEP_NANOS, visible()), vec![]).unwrap();
        b.build(&report(8, STEP_NANOS, visible()), vec![]).unwrap();
        // After two builds the next index is 2 (distinct from 0 and 1) and the
        // last accepted host sequence is the most recent one (Some, not None).
        assert_eq!(b.next_engine_frame_index(), 2);
        assert_eq!(b.last_host_sequence(), Some(8));
    }

    #[test]
    fn accessors_round_trip_initial_state() {
        let b = FrameBuilder::new(STEP_NANOS);
        assert_eq!(b.next_engine_frame_index(), 0);
        assert_eq!(b.last_host_sequence(), None);
        assert_eq!(b.fixed_step_nanos(), STEP_NANOS);
    }
}
