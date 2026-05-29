//! Read-only per-frame context future engine systems consume.

use axiom_math::MathApi;

use crate::engine_frame::EngineFrame;
use crate::frame_command::FrameCommand;
use crate::frame_diagnostics::FrameDiagnostics;
use crate::frame_lifecycle_state::FrameLifecycleState;
use crate::frame_step_summary::FrameStepSummary;
use crate::frame_timing::FrameTiming;
use crate::frame_viewport::FrameViewport;

/// The read-only per-frame context future engine systems consume.
///
/// A [`FrameContext`] is the borrow-side counterpart of an
/// [`EngineFrame`]: it mirrors every accessor but exposes them through a
/// short-lived reference so a future renderer / picking / debug overlay
/// can read frame data without taking ownership. The context never holds
/// mutable engine state, never knows about scenes/worlds/entities, and
/// never invents a system surface — it is purely a lens onto the
/// authoritative `EngineFrame`.
#[derive(Debug, Clone, Copy)]
pub struct FrameContext<'a> {
    frame: &'a EngineFrame,
}

impl<'a> FrameContext<'a> {
    /// Borrow an engine frame as a context.
    pub const fn new(frame: &'a EngineFrame) -> Self {
        FrameContext { frame }
    }

    pub const fn engine_frame_index(&self) -> u64 {
        self.frame.engine_frame_index()
    }

    pub const fn host_frame_sequence(&self) -> u64 {
        self.frame.host_frame_sequence()
    }

    pub const fn viewport(&self) -> &FrameViewport {
        self.frame.viewport()
    }

    /// The viewport's cached aspect ratio. Already finite by construction;
    /// supply a `MathApi` if a caller wants to assert that against the
    /// engine's scalar policy.
    pub fn viewport_aspect_ratio(&self) -> f32 {
        self.frame.viewport().aspect_ratio()
    }

    /// `true` iff the cached aspect ratio is a finite `f32` according to
    /// the math layer's scalar policy. This is what makes the context a
    /// real Layer-04 read surface over Layer-02 math even on the borrow
    /// side.
    pub fn viewport_aspect_is_finite(&self, math: &MathApi) -> bool {
        math.is_finite_value(self.frame.viewport().aspect_ratio())
    }

    pub const fn lifecycle(&self) -> FrameLifecycleState {
        self.frame.lifecycle()
    }

    pub const fn runtime_step_count(&self) -> u32 {
        self.frame.runtime_step_count()
    }

    pub fn runtime_step_summaries(&self) -> &[FrameStepSummary] {
        self.frame.runtime_step_summaries()
    }

    pub const fn timing(&self) -> &FrameTiming {
        self.frame.timing()
    }

    pub const fn diagnostics(&self) -> &FrameDiagnostics {
        self.frame.diagnostics()
    }

    pub fn commands(&self) -> &[FrameCommand] {
        self.frame.commands()
    }

    pub const fn is_skipped(&self) -> bool {
        self.frame.is_skipped()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame_builder::FrameBuilder;
    use axiom_host::{
        HostBoundaryConfig, HostFrameInput, HostLifecycleSignal, HostLifecycleState,
        HostStepPlan, HostViewport,
    };

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

    fn build_frame(elapsed: u64, lifecycle: HostLifecycleState) -> EngineFrame {
        let input = HostFrameInput::new(1, elapsed, vp());
        let plan = HostStepPlan::build(&input, &cfg(), &lifecycle, 0);
        let report = axiom_host::HostFrameReport::new(
            input.sequence(),
            plan,
            plan.steps(),
            Vec::new(),
            vp(),
            lifecycle,
        );
        let mut builder = FrameBuilder::new(STEP_NANOS);
        builder.build(&report, Vec::new()).unwrap()
    }

    #[test]
    fn context_mirrors_engine_frame_identity() {
        let frame = build_frame(STEP_NANOS, visible());
        let ctx = FrameContext::new(&frame);
        assert_eq!(ctx.engine_frame_index(), frame.engine_frame_index());
        assert_eq!(ctx.host_frame_sequence(), frame.host_frame_sequence());
    }

    #[test]
    fn context_exposes_viewport_facts_deterministically() {
        let frame = build_frame(STEP_NANOS, visible());
        let ctx = FrameContext::new(&frame);
        assert_eq!(ctx.viewport().logical_width(), 100);
        assert_eq!(ctx.viewport_aspect_ratio(), 1.0);
        assert!(ctx.viewport_aspect_is_finite(&math()));
    }

    #[test]
    fn context_exposes_runtime_step_count() {
        let frame = build_frame(3 * STEP_NANOS, visible());
        let ctx = FrameContext::new(&frame);
        assert_eq!(ctx.runtime_step_count(), 3);
        assert_eq!(ctx.runtime_step_summaries().len(), 0);
    }

    #[test]
    fn context_exposes_lifecycle_state() {
        let frame = build_frame(STEP_NANOS, visible());
        let ctx = FrameContext::new(&frame);
        assert_eq!(ctx.lifecycle(), FrameLifecycleState::Active);
    }

    #[test]
    fn context_exposes_timing_and_diagnostics() {
        let frame = build_frame(STEP_NANOS, visible());
        let ctx = FrameContext::new(&frame);
        assert_eq!(ctx.timing().runtime_steps_executed(), 1);
        assert!(!ctx.diagnostics().skipped());
    }

    #[test]
    fn context_reports_skipped_for_skipped_frames() {
        let frame = build_frame(STEP_NANOS, HostLifecycleState::initial());
        let ctx = FrameContext::new(&frame);
        assert!(ctx.is_skipped());
    }

    #[test]
    fn context_output_is_identical_for_identical_frames() {
        let a = build_frame(STEP_NANOS, visible());
        let b = build_frame(STEP_NANOS, visible());
        let ca = FrameContext::new(&a);
        let cb = FrameContext::new(&b);
        assert_eq!(ca.host_frame_sequence(), cb.host_frame_sequence());
        assert_eq!(ca.viewport_aspect_ratio(), cb.viewport_aspect_ratio());
        assert_eq!(ca.runtime_step_count(), cb.runtime_step_count());
        assert_eq!(ca.lifecycle(), cb.lifecycle());
    }

    #[test]
    fn context_commands_mirror_frame_commands() {
        let frame = build_frame(STEP_NANOS, visible());
        let ctx = FrameContext::new(&frame);
        assert_eq!(ctx.commands(), frame.commands());
    }
}
