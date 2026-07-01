//! Plain-data, per-step runtime diagnostics.

use axiom_kernel::TelemetryMetric;

use crate::runtime_error::RuntimeError;
use crate::runtime_step::RuntimeStep;
use crate::system_outcome::SystemOutcome;

/// Concise diagnostics for one completed runtime step.
///
/// Plain data, no formatting or external IO. Future layers can serialize
/// this directly when an actual serde dependency is justified; today it lives
/// in memory and is observed through [`crate::runtime_step_record::RuntimeStepRecord`].
#[derive(Debug, Clone)]
pub struct RuntimeDiagnostics {
    step: RuntimeStep,
    system_outcomes: Vec<SystemOutcome>,
    commands_pushed: u32,
    events_pushed: u32,
    commands_drained: u32,
    events_drained: u32,
    /// Excludes the runtime's own internal step-summary counter.
    metrics: Vec<TelemetryMetric>,
    /// The kernel never reads wall-clock time, so this is always `None`.
    step_duration_nanos: Option<u64>,
}

impl RuntimeDiagnostics {
    /// Begin diagnostics for `step` with empty counters.
    pub fn new(step: RuntimeStep) -> Self {
        RuntimeDiagnostics {
            step,
            system_outcomes: Vec::new(),
            commands_pushed: 0,
            events_pushed: 0,
            commands_drained: 0,
            events_drained: 0,
            metrics: Vec::new(),
            step_duration_nanos: None,
        }
    }

    pub fn step(&self) -> RuntimeStep {
        self.step
    }

    pub fn system_outcomes(&self) -> &[SystemOutcome] {
        &self.system_outcomes
    }

    pub fn commands_pushed(&self) -> u32 {
        self.commands_pushed
    }

    pub fn events_pushed(&self) -> u32 {
        self.events_pushed
    }

    pub fn commands_drained(&self) -> u32 {
        self.commands_drained
    }

    pub fn events_drained(&self) -> u32 {
        self.events_drained
    }

    pub fn step_duration_nanos(&self) -> Option<u64> {
        self.step_duration_nanos
    }

    /// Telemetry metrics emitted by this step's systems, in emission order.
    pub fn metrics(&self) -> &[TelemetryMetric] {
        &self.metrics
    }

    /// All errors recorded by failing systems, in execution order.
    pub fn errors(&self) -> Vec<&RuntimeError> {
        self.system_outcomes
            .iter()
            .filter_map(|o| o.result().as_ref().err())
            .collect()
    }

    pub(crate) fn record_outcomes(&mut self, outcomes: Vec<SystemOutcome>) {
        self.system_outcomes = outcomes;
    }

    pub(crate) fn record_metrics(&mut self, metrics: Vec<TelemetryMetric>) {
        self.metrics = metrics;
    }

    pub(crate) fn record_queue_counts(
        &mut self,
        commands_pushed: u32,
        events_pushed: u32,
        commands_drained: u32,
        events_drained: u32,
    ) {
        self.commands_pushed = commands_pushed;
        self.events_pushed = events_pushed;
        self.commands_drained = commands_drained;
        self.events_drained = events_drained;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime_error_code::RuntimeErrorCode;
    use axiom_kernel::{FrameIndex, HandleId, MetricValue, Tick};

    fn step() -> RuntimeStep {
        RuntimeStep::new(FrameIndex::new(1), Tick::new(1), 1_000, 1)
    }

    #[test]
    fn fresh_diagnostics_have_zero_counters() {
        let d = RuntimeDiagnostics::new(step());
        assert_eq!(d.step().tick(), Tick::new(1));
        assert_eq!(d.commands_pushed(), 0);
        assert_eq!(d.events_drained(), 0);
        assert!(d.system_outcomes().is_empty());
        assert!(d.errors().is_empty());
        assert!(d.step_duration_nanos().is_none());
    }

    #[test]
    fn errors_method_returns_only_failed_outcomes_in_order() {
        let mut d = RuntimeDiagnostics::new(step());
        d.record_outcomes(vec![
            SystemOutcome::new(HandleId::from_raw(1), "ok", 1, Ok(())),
            SystemOutcome::new(
                HandleId::from_raw(2),
                "boom",
                2,
                Err(RuntimeError::new(RuntimeErrorCode::SystemFailed, "x")),
            ),
            SystemOutcome::new(HandleId::from_raw(3), "ok-too", 3, Ok(())),
        ]);
        let errs = d.errors();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code(), RuntimeErrorCode::SystemFailed);
        assert_eq!(d.system_outcomes().len(), 3);
        assert_eq!(d.system_outcomes()[1].name(), "boom");
    }

    #[test]
    fn queue_counts_are_recorded() {
        let mut d = RuntimeDiagnostics::new(step());
        d.record_queue_counts(2, 5, 3, 4);
        assert_eq!(d.commands_pushed(), 2);
        assert_eq!(d.events_pushed(), 5);
        assert_eq!(d.commands_drained(), 3);
        assert_eq!(d.events_drained(), 4);
    }

    #[test]
    fn metrics_default_empty_and_are_recorded() {
        let mut d = RuntimeDiagnostics::new(step());
        assert!(d.metrics().is_empty());
        d.record_metrics(vec![
            TelemetryMetric::counter("frame.draws", 1, Some(Tick::new(1))),
            TelemetryMetric::gauge(
                "cube.angle_deg",
                MetricValue::float(2.0),
                Some(Tick::new(1)),
            ),
        ]);
        assert_eq!(d.metrics().len(), 2);
        assert_eq!(d.metrics()[0].name(), "frame.draws");
        assert_eq!(d.metrics()[1].value(), MetricValue::float(2.0));
    }
}
