//! Test-only fixtures: real `EngineFrame`s in the active and skipped states,
//! built directly from the host step plan (the same way the frame layer's own
//! tests build them). `#[cfg(test)]` only — not part of the layer's surface.

use axiom_frame::{EngineFrame, FrameBuilder};
use axiom_host::{
    HostBoundaryConfig, HostFrameInput, HostFrameReport, HostLifecycleSignal, HostLifecycleState,
    HostStepPlan, HostViewport,
};
use axiom_math::MathApi;

const STEP_NANOS: u64 = 1_000;

fn viewport() -> HostViewport {
    HostViewport::new(&MathApi::new(), 320, 200, 1.0).unwrap()
}

fn config() -> HostBoundaryConfig {
    HostBoundaryConfig::new(STEP_NANOS, 5).unwrap()
}

fn build(lifecycle: HostLifecycleState, elapsed: u64) -> EngineFrame {
    let input = HostFrameInput::new(1, elapsed, viewport());
    let plan = HostStepPlan::build(&input, &config(), &lifecycle, 0);
    let report = HostFrameReport::new(
        input.sequence(),
        plan,
        plan.steps(),
        Vec::new(),
        viewport(),
        lifecycle,
    );
    FrameBuilder::new(STEP_NANOS)
        .build(&report, Vec::new())
        .unwrap()
}

/// An active engine frame: host started, one runtime step — `advance` runs.
pub(crate) fn active_engine_frame() -> EngineFrame {
    let lifecycle = HostLifecycleState::initial().apply(HostLifecycleSignal::Started);
    build(lifecycle, STEP_NANOS)
}

/// A skipped engine frame: host not visible — `advance` runs no systems.
pub(crate) fn skipped_engine_frame() -> EngineFrame {
    build(HostLifecycleState::initial(), STEP_NANOS)
}

/// A visible-but-zero-step engine frame: not skipped, yet no runtime step ran
/// (zero elapsed time) — `advance` still runs no systems.
pub(crate) fn active_zero_step_engine_frame() -> EngineFrame {
    let lifecycle = HostLifecycleState::initial().apply(HostLifecycleSignal::Started);
    build(lifecycle, 0)
}
