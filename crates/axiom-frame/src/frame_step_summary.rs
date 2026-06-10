//! Stable summary of one runtime step inside an engine frame.

use axiom_kernel::TelemetryMetric;
use axiom_runtime::RuntimeStepRecord;

use crate::frame_system_report::FrameSystemReport;

/// A stable, value-typed summary of one [`RuntimeStepRecord`].
///
/// `RuntimeStepRecord` carries diagnostics, queue counts, and a full
/// runtime state. Layer 04 keeps the deterministic identity fields every
/// future engine system needs to reason about a step:
///
/// - the runtime frame index (kernel `FrameIndex`),
/// - the runtime simulation tick (kernel `Tick`),
/// - the runtime's monotonic step sequence number,
/// - whether every system in the step succeeded,
/// - an ordered, value-typed report of each system that ran (so the
///   per-system detail the runtime gathered survives the frame boundary),
/// - and the telemetry metrics the step's systems emitted.
///
/// Two equal records produce equal summaries. `Eq`/`Hash` are intentionally
/// not derived: carried [`TelemetryMetric`]s may hold `f32` gauge values,
/// which are not totally ordered.
#[derive(Debug, Clone, PartialEq)]
pub struct FrameStepSummary {
    runtime_frame_index: u64,
    runtime_tick: u64,
    runtime_sequence: u64,
    succeeded: bool,
    systems: Vec<FrameSystemReport>,
    metrics: Vec<TelemetryMetric>,
}

impl FrameStepSummary {
    /// Build a summary from a runtime step record.
    pub fn from_record(record: &RuntimeStepRecord) -> Self {
        let step = record.step();
        let systems = record
            .diagnostics()
            .system_outcomes()
            .iter()
            .map(FrameSystemReport::from_outcome)
            .collect();
        let metrics = record.diagnostics().metrics().to_vec();
        FrameStepSummary {
            runtime_frame_index: step.frame().raw(),
            runtime_tick: step.tick().raw(),
            runtime_sequence: step.sequence(),
            succeeded: record.succeeded(),
            systems,
            metrics,
        }
    }

    /// Build an ordered list of summaries from a slice of records,
    /// preserving the records' original order.
    pub fn list_from_records(records: &[RuntimeStepRecord]) -> Vec<FrameStepSummary> {
        records.iter().map(FrameStepSummary::from_record).collect()
    }

    pub const fn runtime_frame_index(&self) -> u64 {
        self.runtime_frame_index
    }

    pub const fn runtime_tick(&self) -> u64 {
        self.runtime_tick
    }

    pub const fn runtime_sequence(&self) -> u64 {
        self.runtime_sequence
    }

    pub const fn succeeded(&self) -> bool {
        self.succeeded
    }

    /// The ordered per-system reports gathered during this step. Empty when
    /// the runtime ran no systems (or had diagnostics disabled).
    pub fn systems(&self) -> &[FrameSystemReport] {
        &self.systems
    }

    /// The telemetry metrics the step's systems emitted, in emission order.
    pub fn metrics(&self) -> &[TelemetryMetric] {
        &self.metrics
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_host::{
        HostBoundaryConfig, HostFrameInput, HostLifecycleSignal, HostLifecycleState,
        HostStepDriver, HostViewport,
    };
    use axiom_kernel::Ratio;
    use axiom_runtime::{Runtime, RuntimeConfig};

    const STEP_NANOS: u64 = 1_000;

    fn vp() -> HostViewport {
        HostViewport::new(100, 100, Ratio::new(1.0).unwrap()).unwrap()
    }

    fn driver_and_runtime() -> (HostStepDriver, Runtime) {
        let mut driver = HostStepDriver::new(HostBoundaryConfig::new(STEP_NANOS, 5).unwrap());
        driver.apply_lifecycle_signal(HostLifecycleSignal::Started);
        let mut runtime = Runtime::new(RuntimeConfig::new(STEP_NANOS)).unwrap();
        runtime.initialize().unwrap();
        runtime.start().unwrap();
        let _ = HostLifecycleState::initial(); // touch import for clarity
        (driver, runtime)
    }

    #[test]
    fn summaries_preserve_runtime_step_order() {
        let (mut driver, mut runtime) = driver_and_runtime();
        let report = driver
            .drive(&mut runtime, HostFrameInput::new(1, 3 * STEP_NANOS, vp()))
            .unwrap();
        let summaries = FrameStepSummary::list_from_records(report.step_records());
        let ticks: Vec<u64> = summaries.iter().map(|s| s.runtime_tick()).collect();
        assert_eq!(ticks, vec![1, 2, 3]);
        // Pin frame index and sequence to the third step's value (3), which is
        // distinct from the mutation constant 1.
        let frames: Vec<u64> = summaries.iter().map(|s| s.runtime_frame_index()).collect();
        assert_eq!(frames, vec![1, 2, 3]);
        let seqs: Vec<u64> = summaries.iter().map(|s| s.runtime_sequence()).collect();
        assert_eq!(seqs, vec![1, 2, 3]);
        assert_eq!(summaries[2].runtime_frame_index(), 3);
        assert_eq!(summaries[2].runtime_sequence(), 3);
    }

    #[test]
    fn summary_preserves_frame_and_tick_identity() {
        let (mut driver, mut runtime) = driver_and_runtime();
        let report = driver
            .drive(&mut runtime, HostFrameInput::new(1, STEP_NANOS, vp()))
            .unwrap();
        let summary = FrameStepSummary::from_record(&report.step_records()[0]);
        assert_eq!(summary.runtime_tick(), 1);
        assert_eq!(summary.runtime_frame_index(), 1);
        assert_eq!(summary.runtime_sequence(), 1);
        assert!(summary.succeeded());
    }

    #[test]
    fn list_from_empty_records_is_empty() {
        let summaries = FrameStepSummary::list_from_records(&[]);
        assert!(summaries.is_empty());
    }

    #[test]
    fn identical_runtime_records_produce_identical_summaries() {
        let make = || {
            let (mut driver, mut runtime) = driver_and_runtime();
            let report = driver
                .drive(&mut runtime, HostFrameInput::new(1, 2 * STEP_NANOS, vp()))
                .unwrap();
            FrameStepSummary::list_from_records(report.step_records())
        };
        assert_eq!(make(), make());
    }

    #[test]
    fn failure_status_is_preserved() {
        use axiom_kernel::HandleId;
        use axiom_runtime::{
            RuntimeContext, RuntimeError, RuntimeErrorCode, RuntimeResult, RuntimeSystem,
        };

        struct AlwaysFail;
        impl RuntimeSystem for AlwaysFail {
            fn run(&mut self, _: &mut RuntimeContext<'_>) -> RuntimeResult<()> {
                Err(RuntimeError::new(RuntimeErrorCode::SystemFailed, "boom"))
            }
        }

        // The runtime is configured to *not* halt on system failure so the
        // host driver gets a `RuntimeStepRecord` back rather than a
        // failure. The record's `succeeded()` is `false`, which is what we
        // want to assert flows through to the summary.
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
            .drive(&mut runtime, HostFrameInput::new(1, STEP_NANOS, vp()))
            .unwrap();
        let summary = FrameStepSummary::from_record(&report.step_records()[0]);
        assert!(
            !summary.succeeded(),
            "summary must mirror record.succeeded()"
        );

        // The per-system detail must survive into the frame summary: one
        // system ran, named "fail", at order 1, with the SystemFailed code.
        assert_eq!(summary.systems().len(), 1);
        let system = &summary.systems()[0];
        assert_eq!(system.name(), "fail");
        assert_eq!(system.order(), 1);
        assert!(!system.succeeded());
        assert_eq!(
            system.error_code(),
            Some(RuntimeErrorCode::SystemFailed.raw())
        );
    }

    #[test]
    fn step_without_systems_has_empty_system_list_and_no_metrics() {
        let (mut driver, mut runtime) = driver_and_runtime();
        let report = driver
            .drive(&mut runtime, HostFrameInput::new(1, STEP_NANOS, vp()))
            .unwrap();
        let summary = FrameStepSummary::from_record(&report.step_records()[0]);
        assert!(summary.systems().is_empty());
        assert!(summary.metrics().is_empty());
    }

    #[test]
    fn step_metrics_survive_into_the_summary() {
        use axiom_kernel::{HandleId, MetricValue, TelemetryMetric};
        use axiom_runtime::{RuntimeContext, RuntimeResult, RuntimeSystem};

        struct Emit;
        impl RuntimeSystem for Emit {
            fn run(&mut self, ctx: &mut RuntimeContext<'_>) -> RuntimeResult<()> {
                let tick = ctx.step().tick();
                ctx.metric(TelemetryMetric::gauge(
                    "cube.angle_deg",
                    MetricValue::float(3.0),
                    Some(tick),
                ));
                Ok(())
            }
        }

        let mut driver = HostStepDriver::new(HostBoundaryConfig::new(STEP_NANOS, 5).unwrap());
        driver.apply_lifecycle_signal(HostLifecycleSignal::Started);
        let mut runtime = Runtime::new(RuntimeConfig::new(STEP_NANOS)).unwrap();
        runtime.initialize().unwrap();
        runtime.start().unwrap();
        runtime
            .scheduler_mut()
            .register(HandleId::from_raw(1), "emit", 1, Box::new(Emit))
            .unwrap();
        let report = driver
            .drive(&mut runtime, HostFrameInput::new(1, STEP_NANOS, vp()))
            .unwrap();
        let summary = FrameStepSummary::from_record(&report.step_records()[0]);
        assert_eq!(summary.metrics().len(), 1);
        assert_eq!(summary.metrics()[0].name(), "cube.angle_deg");
        assert_eq!(summary.metrics()[0].value(), MetricValue::float(3.0));
    }
}
