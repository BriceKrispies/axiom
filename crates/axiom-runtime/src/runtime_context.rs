//! The per-step surface a [`crate::runtime_system::RuntimeSystem`] sees.

use axiom_kernel::{InMemoryLogSink, InMemoryTelemetrySink, KernelApi, LogRecord, TelemetryMetric};

use crate::runtime_command_queue::RuntimeCommandQueue;
use crate::runtime_event_queue::RuntimeEventQueue;
use crate::runtime_step::RuntimeStep;

/// The context handed to a runtime system during one step.
///
/// Exposes (and *only* exposes) the deterministic substrate: the current step
/// identity, mutable access to the command and event queues, and structured
/// logging / telemetry emission routed through the kernel facade. No
/// rendering, world, or browser concept is reachable here — that is enforced
/// by what this struct does, not by what it doesn't.
#[derive(Debug)]
pub struct RuntimeContext<'r> {
    step: RuntimeStep,
    commands: &'r mut RuntimeCommandQueue,
    events: &'r mut RuntimeEventQueue,
    kernel: &'r KernelApi,
    log_sink: &'r mut InMemoryLogSink,
    telemetry_sink: &'r mut InMemoryTelemetrySink,
}

impl<'r> RuntimeContext<'r> {
    /// Construct a context — called by [`crate::runtime::Runtime::step`].
    pub fn new(
        step: RuntimeStep,
        commands: &'r mut RuntimeCommandQueue,
        events: &'r mut RuntimeEventQueue,
        kernel: &'r KernelApi,
        log_sink: &'r mut InMemoryLogSink,
        telemetry_sink: &'r mut InMemoryTelemetrySink,
    ) -> Self {
        RuntimeContext {
            step,
            commands,
            events,
            kernel,
            log_sink,
            telemetry_sink,
        }
    }

    /// Identity of the step currently executing.
    pub fn step(&self) -> RuntimeStep {
        self.step
    }

    /// The kernel facade. Exposed so a system can construct kernel-typed
    /// values (clocks, IDs) deterministically without a parallel path.
    pub fn kernel(&self) -> &KernelApi {
        self.kernel
    }

    pub fn commands(&self) -> &RuntimeCommandQueue {
        self.commands
    }

    pub fn commands_mut(&mut self) -> &mut RuntimeCommandQueue {
        self.commands
    }

    pub fn events(&self) -> &RuntimeEventQueue {
        self.events
    }

    pub fn events_mut(&mut self) -> &mut RuntimeEventQueue {
        self.events
    }

    /// Emit a [`LogRecord`] to the runtime's in-memory sink via the kernel.
    /// The kernel never prints — these records are inspected through the
    /// runtime's accessor (or by future export layers).
    pub fn log(&mut self, record: LogRecord) {
        self.kernel.log(&mut *self.log_sink, record);
    }

    /// Emit a [`TelemetryMetric`] to the runtime's in-memory sink via the
    /// kernel.
    pub fn metric(&mut self, metric: TelemetryMetric) {
        self.kernel.record_metric(&mut *self.telemetry_sink, metric);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::{FrameIndex, LogLevel, MetricValue, Tick};

    #[test]
    fn log_and_metric_route_through_kernel_into_sinks() {
        let kernel = KernelApi::new();
        let mut commands = RuntimeCommandQueue::new();
        let mut events = RuntimeEventQueue::new();
        let mut logs = kernel.log_sink();
        let mut telemetry = kernel.telemetry_sink();

        {
            let mut ctx = RuntimeContext::new(
                RuntimeStep::new(FrameIndex::new(0), Tick::new(0), 1_000, 0),
                &mut commands,
                &mut events,
                &kernel,
                &mut logs,
                &mut telemetry,
            );
            ctx.log(LogRecord::new(LogLevel::Info, "runtime.ctx", 1, "tick"));
            ctx.metric(TelemetryMetric::counter("ticks", 1, Some(Tick::new(0))));
            ctx.metric(TelemetryMetric::gauge(
                "load",
                MetricValue::float(0.5),
                None,
            ));
        }

        assert_eq!(logs.len(), 1);
        assert_eq!(telemetry.len(), 2);
        assert_eq!(telemetry.counter_total("ticks"), 1);
    }

    #[test]
    fn queues_are_mutable_through_context() {
        use crate::runtime_command::RuntimeCommand;
        let kernel = KernelApi::new();
        let mut commands = RuntimeCommandQueue::new();
        let mut events = RuntimeEventQueue::new();
        let mut logs = kernel.log_sink();
        let mut telemetry = kernel.telemetry_sink();

        let mut ctx = RuntimeContext::new(
            RuntimeStep::new(FrameIndex::new(0), Tick::new(0), 1_000, 0),
            &mut commands,
            &mut events,
            &kernel,
            &mut logs,
            &mut telemetry,
        );
        ctx.commands_mut()
            .push(RuntimeCommand::new(7, Tick::new(0), vec![]));
        assert_eq!(ctx.commands().len(), 1);
    }
}
