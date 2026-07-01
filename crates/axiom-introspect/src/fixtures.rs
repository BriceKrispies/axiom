//! Test-only fixtures: real [`EngineFrame`] values built through the frame
//! builder, so the introspection adapters run against the genuine frame
//! contract rather than hand-rolled stand-ins.
//!
//! This module is `#[cfg(test)]` only; none of it is part of the layer's
//! runtime surface. It uses lower layers (host/runtime/math) as dev
//! dependencies, exactly as the frame layer's own tests do.

use axiom_frame::{EngineFrame, FrameBuilder};
use axiom_host::{
    HostBoundaryConfig, HostFrameInput, HostLifecycleSignal, HostStepDriver, HostViewport,
};
use axiom_kernel::{HandleId, MetricValue, Ratio, TelemetryMetric};
use axiom_runtime::{
    Runtime, RuntimeConfig, RuntimeContext, RuntimeError, RuntimeErrorCode, RuntimeResult,
    RuntimeSystem,
};

const STEP_NANOS: u64 = 1_000;

fn viewport() -> HostViewport {
    HostViewport::new(320, 200, Ratio::new(1.0).unwrap()).unwrap()
}

/// `n` consecutive active engine frames (no systems registered), with
/// monotonically increasing engine frame index and host sequence, built
/// through a single persistent runtime/driver/builder.
pub(crate) fn active_engine_frames(n: u64) -> Vec<EngineFrame> {
    let mut driver = HostStepDriver::new(HostBoundaryConfig::new(STEP_NANOS, 5).unwrap());
    driver.apply_lifecycle_signal(HostLifecycleSignal::Started);
    let mut runtime = Runtime::new(RuntimeConfig::new(STEP_NANOS)).unwrap();
    runtime.initialize().unwrap();
    runtime.start().unwrap();
    let mut builder = FrameBuilder::new(STEP_NANOS);
    let mut frames = Vec::new();
    for seq in 1..=n {
        let report = driver
            .drive(
                &mut runtime,
                HostFrameInput::new(seq, STEP_NANOS, viewport()),
            )
            .unwrap();
        frames.push(builder.build(&report, Vec::new()).unwrap());
    }
    frames
}

struct AlwaysFail;

impl RuntimeSystem for AlwaysFail {
    fn run(&mut self, ctx: &mut RuntimeContext<'_>) -> RuntimeResult<()> {
        let tick = ctx.step().tick();
        ctx.metric(TelemetryMetric::gauge(
            "cube.angle_deg",
            MetricValue::float(1.0),
            Some(tick),
        ));
        Err(RuntimeError::new(RuntimeErrorCode::SystemFailed, "boom"))
    }
}

/// One engine frame whose single runtime step ran a failing system that also
/// emitted a metric, so its step summary carries both a populated per-system
/// report (with an error code) and a telemetry metric.
pub(crate) fn failing_engine_frame() -> EngineFrame {
    let mut driver = HostStepDriver::new(HostBoundaryConfig::new(STEP_NANOS, 5).unwrap());
    driver.apply_lifecycle_signal(HostLifecycleSignal::Started);
    let mut runtime =
        Runtime::new(RuntimeConfig::new(STEP_NANOS).with_fail_on_system_error(false)).unwrap();
    runtime.initialize().unwrap();
    runtime.start().unwrap();
    runtime
        .scheduler_mut()
        .register(HandleId::from_raw(1), "fail", 1, Box::new(AlwaysFail))
        .unwrap();
    let report = driver
        .drive(&mut runtime, HostFrameInput::new(1, STEP_NANOS, viewport()))
        .unwrap();
    FrameBuilder::new(STEP_NANOS)
        .build(&report, Vec::new())
        .unwrap()
}
