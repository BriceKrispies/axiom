//! The main runtime: owns lifecycle state and drives deterministic stepping.

use axiom_kernel::{InMemoryLogSink, InMemoryTelemetrySink, KernelApi};

use crate::runtime_command_queue::RuntimeCommandQueue;
use crate::runtime_config::RuntimeConfig;
use crate::runtime_context::RuntimeContext;
use crate::runtime_diagnostics::RuntimeDiagnostics;
use crate::runtime_error::RuntimeError;
use crate::runtime_error_code::RuntimeErrorCode;
use crate::runtime_event_queue::RuntimeEventQueue;
use crate::runtime_result::RuntimeResult;
use crate::runtime_scheduler::RuntimeScheduler;
use crate::runtime_state::RuntimeState;
use crate::runtime_step::RuntimeStep;
use crate::runtime_step_record::RuntimeStepRecord;
use crate::runtime_timeline::RuntimeTimeline;

/// The deterministic engine runtime.
///
/// Owns the kernel facade, the timeline (wrapping the kernel
/// `SimulationClock`), the scheduler, the command and event queues, and the
/// runtime's structured logging / telemetry sinks. The state machine is
/// strictly enforced: any illegal lifecycle call returns
/// [`RuntimeErrorCode::InvalidLifecycleTransition`].
///
/// `step()` advances exactly one fixed simulation step, runs every registered
/// system in scheduled order, drains the command/event queues at the step
/// boundary, and returns a [`RuntimeStepRecord`] describing what happened.
pub struct Runtime {
    kernel: KernelApi,
    config: RuntimeConfig,
    state: RuntimeState,
    timeline: RuntimeTimeline,
    scheduler: RuntimeScheduler,
    commands: RuntimeCommandQueue,
    events: RuntimeEventQueue,
    log_sink: InMemoryLogSink,
    telemetry_sink: InMemoryTelemetrySink,
}

impl std::fmt::Debug for Runtime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Runtime")
            .field("state", &self.state)
            .field("config", &self.config)
            .field("timeline", &self.timeline)
            .field("scheduler", &self.scheduler)
            .field("commands_pending", &self.commands.len())
            .field("events_pending", &self.events.len())
            .finish()
    }
}

impl Runtime {
    /// Construct a runtime from `config`. The fixed step is validated against
    /// the kernel; on rejection the returned error wraps the kernel cause.
    pub fn new(config: RuntimeConfig) -> RuntimeResult<Self> {
        let kernel = KernelApi::new();
        let fixed_step = config.validate(&kernel)?;
        let clock = kernel.simulation_clock(fixed_step);
        let log_sink = kernel.log_sink();
        let telemetry_sink = kernel.telemetry_sink();
        Ok(Runtime {
            kernel,
            config,
            state: RuntimeState::Created,
            timeline: RuntimeTimeline::new(clock),
            scheduler: RuntimeScheduler::new(),
            commands: RuntimeCommandQueue::new(),
            events: RuntimeEventQueue::new(),
            log_sink,
            telemetry_sink,
        })
    }

    /// Transition `Created` → `Initialized`.
    pub fn initialize(&mut self) -> RuntimeResult<()> {
        match self.state {
            RuntimeState::Created => {
                self.state = RuntimeState::Initialized;
                Ok(())
            }
            _ => Err(invalid_transition("initialize requires Created")),
        }
    }

    /// Transition `Initialized` or `Paused` → `Running`.
    pub fn start(&mut self) -> RuntimeResult<()> {
        match self.state {
            RuntimeState::Initialized | RuntimeState::Paused => {
                self.state = RuntimeState::Running;
                Ok(())
            }
            _ => Err(invalid_transition("start requires Initialized or Paused")),
        }
    }

    /// Transition `Running` → `Paused`.
    pub fn pause(&mut self) -> RuntimeResult<()> {
        match self.state {
            RuntimeState::Running => {
                self.state = RuntimeState::Paused;
                Ok(())
            }
            _ => Err(invalid_transition("pause requires Running")),
        }
    }

    /// Transition `Running`, `Paused`, or `Initialized` → `Stopped`.
    /// Terminal states (`Stopped`, `Failed`) are rejected.
    pub fn stop(&mut self) -> RuntimeResult<()> {
        match self.state {
            RuntimeState::Running | RuntimeState::Paused | RuntimeState::Initialized => {
                self.state = RuntimeState::Stopped;
                Ok(())
            }
            _ => Err(invalid_transition(
                "stop requires Running, Paused, or Initialized",
            )),
        }
    }

    /// Advance exactly one deterministic step.
    ///
    /// - Rejects with [`RuntimeErrorCode::StepWhileNotRunning`] unless state is `Running`.
    /// - Advances the timeline (kernel tick / frame, runtime sequence).
    /// - Builds a [`RuntimeContext`] borrowing the runtime's queues and sinks.
    /// - Executes the scheduler in order; the `fail_on_system_error` flag in
    ///   [`RuntimeConfig`] determines whether failure halts the scheduler.
    /// - Drains the command and event queues at the boundary, recording counts.
    /// - If any system failed and the config opts in, transitions to `Failed`.
    /// - If diagnostics are enabled, emits a kernel `LogRecord` summarizing
    ///   the step into the runtime's in-memory log sink.
    #[axiom_zones::sim]
    pub fn step(&mut self) -> RuntimeResult<RuntimeStepRecord> {
        if self.state != RuntimeState::Running {
            return Err(RuntimeError::new(
                RuntimeErrorCode::StepWhileNotRunning,
                "step() requires the runtime to be in Running",
            ));
        }

        let commands_before = self.commands.len();
        let events_before = self.events.len();
        let metrics_before = self.telemetry_sink.len();

        let step = self.timeline.advance()?;
        let mut diagnostics = RuntimeDiagnostics::new(step);

        // Run the scheduler with a context borrowing the runtime's queues and
        // sinks. The kernel is borrowed shared (the facade is stateless).
        let outcomes = {
            let mut ctx = RuntimeContext::new(
                step,
                &mut self.commands,
                &mut self.events,
                &self.kernel,
                &mut self.log_sink,
                &mut self.telemetry_sink,
            );
            self.scheduler
                .execute(&mut ctx, self.config.fail_on_system_error())
        };

        let any_error = outcomes.iter().any(|o| !o.succeeded());
        diagnostics.record_outcomes(outcomes);

        // Capture the metrics the step's systems emitted (everything appended
        // to the sink during the scheduler run), before the runtime's own
        // diagnostics counter is recorded below.
        diagnostics.record_metrics(self.telemetry_sink.metrics()[metrics_before..].to_vec());

        let commands_after = self.commands.len();
        let events_after = self.events.len();
        let commands_pushed = commands_after
            .saturating_sub(commands_before)
            .min(u32::MAX as usize) as u32;
        let events_pushed = events_after
            .saturating_sub(events_before)
            .min(u32::MAX as usize) as u32;

        // Drain at the step boundary.
        let commands_drained = commands_after.min(u32::MAX as usize) as u32;
        let events_drained = events_after.min(u32::MAX as usize) as u32;
        self.commands.clear();
        self.events.clear();

        diagnostics.record_queue_counts(
            commands_pushed,
            events_pushed,
            commands_drained,
            events_drained,
        );

        if any_error && self.config.fail_on_system_error() {
            self.state = RuntimeState::Failed;
        }

        if self.config.diagnostics_enabled() {
            self.emit_step_summary(&diagnostics);
        }

        Ok(RuntimeStepRecord::new(
            step,
            diagnostics,
            self.state,
            self.commands.len(),
            self.events.len(),
        ))
    }

    /// Emit a `LogRecord` summarizing the just-completed step into the
    /// runtime's in-memory log sink, via the kernel.
    fn emit_step_summary(&mut self, diagnostics: &RuntimeDiagnostics) {
        use axiom_kernel::{LogField, LogLevel, LogRecord, TelemetryMetric};

        let level = if diagnostics.errors().is_empty() {
            LogLevel::Info
        } else {
            LogLevel::Error
        };
        let record = LogRecord::new(level, "runtime.step", 1, "runtime step completed")
            .at(diagnostics.step().tick(), diagnostics.step().frame())
            .with_field(LogField::u64("sequence", diagnostics.step().sequence()))
            .with_field(LogField::u64(
                "commands_pushed",
                diagnostics.commands_pushed() as u64,
            ))
            .with_field(LogField::u64(
                "events_pushed",
                diagnostics.events_pushed() as u64,
            ));
        self.kernel.log(&mut self.log_sink, record);
        self.kernel.record_metric(
            &mut self.telemetry_sink,
            TelemetryMetric::counter("runtime.steps", 1, Some(diagnostics.step().tick())),
        );
    }

    // --- accessors ---

    pub fn state(&self) -> RuntimeState {
        self.state
    }

    pub fn config(&self) -> &RuntimeConfig {
        &self.config
    }

    pub fn scheduler(&self) -> &RuntimeScheduler {
        &self.scheduler
    }

    pub fn scheduler_mut(&mut self) -> &mut RuntimeScheduler {
        &mut self.scheduler
    }

    pub fn timeline(&self) -> &RuntimeTimeline {
        &self.timeline
    }

    pub fn commands(&self) -> &RuntimeCommandQueue {
        &self.commands
    }

    pub fn events(&self) -> &RuntimeEventQueue {
        &self.events
    }

    pub fn log_sink(&self) -> &InMemoryLogSink {
        &self.log_sink
    }

    pub fn telemetry_sink(&self) -> &InMemoryTelemetrySink {
        &self.telemetry_sink
    }

    pub fn current_step(&self) -> RuntimeStep {
        self.timeline.current_step()
    }
}

fn invalid_transition(message: &'static str) -> RuntimeError {
    RuntimeError::new(RuntimeErrorCode::InvalidLifecycleTransition, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime_command::RuntimeCommand;
    use crate::runtime_event::RuntimeEvent;
    use crate::runtime_system::RuntimeSystem;
    use axiom_kernel::{HandleId, Tick};

    fn cfg() -> RuntimeConfig {
        RuntimeConfig::new(1_000)
    }

    fn started() -> Runtime {
        let mut rt = Runtime::new(cfg()).unwrap();
        rt.initialize().unwrap();
        rt.start().unwrap();
        rt
    }

    // --- Lifecycle transition tests ---

    #[test]
    fn fresh_runtime_starts_in_created() {
        let rt = Runtime::new(cfg()).unwrap();
        assert_eq!(rt.state(), RuntimeState::Created);
    }

    #[test]
    fn happy_path_created_to_running_to_stopped() {
        let mut rt = Runtime::new(cfg()).unwrap();
        rt.initialize().unwrap();
        assert_eq!(rt.state(), RuntimeState::Initialized);
        rt.start().unwrap();
        assert_eq!(rt.state(), RuntimeState::Running);
        rt.pause().unwrap();
        assert_eq!(rt.state(), RuntimeState::Paused);
        rt.start().unwrap();
        assert_eq!(rt.state(), RuntimeState::Running);
        rt.stop().unwrap();
        assert_eq!(rt.state(), RuntimeState::Stopped);
    }

    #[test]
    fn double_initialize_is_rejected() {
        let mut rt = Runtime::new(cfg()).unwrap();
        rt.initialize().unwrap();
        let err = rt.initialize().unwrap_err();
        assert_eq!(err.code(), RuntimeErrorCode::InvalidLifecycleTransition);
    }

    #[test]
    fn start_without_initialize_is_rejected() {
        let mut rt = Runtime::new(cfg()).unwrap();
        let err = rt.start().unwrap_err();
        assert_eq!(err.code(), RuntimeErrorCode::InvalidLifecycleTransition);
    }

    #[test]
    fn pause_without_running_is_rejected() {
        let mut rt = Runtime::new(cfg()).unwrap();
        let err = rt.pause().unwrap_err();
        assert_eq!(err.code(), RuntimeErrorCode::InvalidLifecycleTransition);
    }

    #[test]
    fn stop_in_failed_state_is_rejected() {
        let mut rt = Runtime::new(cfg()).unwrap();
        rt.initialize().unwrap();
        rt.start().unwrap();
        // Force-fail via a failing system.
        struct F;
        impl RuntimeSystem for F {
            fn run(&mut self, _: &mut RuntimeContext<'_>) -> RuntimeResult<()> {
                Err(RuntimeError::new(RuntimeErrorCode::SystemFailed, "x"))
            }
        }
        rt.scheduler_mut()
            .register(HandleId::from_raw(1), "f", 1, Box::new(F))
            .unwrap();
        let _ = rt.step().unwrap();
        assert_eq!(rt.state(), RuntimeState::Failed);
        assert_eq!(
            rt.stop().unwrap_err().code(),
            RuntimeErrorCode::InvalidLifecycleTransition
        );
    }

    // --- Stepping determinism ---

    #[test]
    fn step_requires_running_state() {
        let mut rt = Runtime::new(cfg()).unwrap();
        let err = rt.step().unwrap_err();
        assert_eq!(err.code(), RuntimeErrorCode::StepWhileNotRunning);
    }

    #[test]
    fn each_step_increments_tick_frame_and_sequence_by_one() {
        let mut rt = started();
        let r1 = rt.step().unwrap();
        let r2 = rt.step().unwrap();
        assert_eq!(r1.step().tick(), Tick::new(1));
        assert_eq!(r2.step().tick(), Tick::new(2));
        assert_eq!(r2.step().sequence(), 2);
        assert_eq!(r1.step().fixed_delta_nanos(), 1_000);
    }

    #[test]
    fn two_identically_configured_runtimes_produce_identical_steps() {
        let mut a = started();
        let mut b = started();
        let mut last_a = None;
        let mut last_b = None;
        for _ in 0..16 {
            last_a = Some(a.step().unwrap().step());
            last_b = Some(b.step().unwrap().step());
        }
        assert_eq!(last_a, last_b);
    }

    // --- System ordering through Runtime::step ---

    #[test]
    fn systems_run_in_scheduled_order_each_step() {
        use std::sync::{Arc, Mutex};
        struct Trace {
            name: &'static str,
            trace: Arc<Mutex<Vec<&'static str>>>,
        }
        impl RuntimeSystem for Trace {
            fn run(&mut self, _: &mut RuntimeContext<'_>) -> RuntimeResult<()> {
                self.trace.lock().unwrap().push(self.name);
                Ok(())
            }
        }

        let mut rt = started();
        let trace = Arc::new(Mutex::new(Vec::new()));
        rt.scheduler_mut()
            .register(
                HandleId::from_raw(2),
                "b",
                20,
                Box::new(Trace {
                    name: "b",
                    trace: trace.clone(),
                }),
            )
            .unwrap();
        rt.scheduler_mut()
            .register(
                HandleId::from_raw(1),
                "a",
                10,
                Box::new(Trace {
                    name: "a",
                    trace: trace.clone(),
                }),
            )
            .unwrap();
        rt.step().unwrap();
        rt.step().unwrap();
        assert_eq!(*trace.lock().unwrap(), vec!["a", "b", "a", "b"]);
    }

    // --- Queue draining at boundary ---

    #[test]
    fn commands_and_events_are_drained_at_step_boundary() {
        struct Producer;
        impl RuntimeSystem for Producer {
            fn run(&mut self, ctx: &mut RuntimeContext<'_>) -> RuntimeResult<()> {
                let tick = ctx.step().tick();
                ctx.commands_mut()
                    .push(RuntimeCommand::new(1, tick, vec![]));
                ctx.commands_mut()
                    .push(RuntimeCommand::new(2, tick, vec![]));
                ctx.events_mut().push(RuntimeEvent::new(9, tick, vec![]));
                Ok(())
            }
        }

        let mut rt = started();
        rt.scheduler_mut()
            .register(HandleId::from_raw(1), "p", 1, Box::new(Producer))
            .unwrap();
        let record = rt.step().unwrap();

        // The systems pushed 2 commands and 1 event, all of which got drained.
        assert_eq!(record.diagnostics().commands_pushed(), 2);
        assert_eq!(record.diagnostics().events_pushed(), 1);
        assert_eq!(record.diagnostics().commands_drained(), 2);
        assert_eq!(record.diagnostics().events_drained(), 1);
        assert!(rt.commands().is_empty());
        assert!(rt.events().is_empty());
    }

    // --- Failure handling ---

    #[test]
    fn system_failure_transitions_runtime_to_failed_by_default() {
        struct F;
        impl RuntimeSystem for F {
            fn run(&mut self, _: &mut RuntimeContext<'_>) -> RuntimeResult<()> {
                Err(RuntimeError::new(RuntimeErrorCode::SystemFailed, "x"))
            }
        }
        let mut rt = started();
        rt.scheduler_mut()
            .register(HandleId::from_raw(1), "f", 1, Box::new(F))
            .unwrap();
        let record = rt.step().unwrap();
        assert_eq!(record.state_after(), RuntimeState::Failed);
        assert_eq!(rt.state(), RuntimeState::Failed);
        assert!(!record.succeeded());
        assert_eq!(record.diagnostics().errors().len(), 1);
    }

    #[test]
    fn continue_on_error_keeps_runtime_running() {
        struct F;
        impl RuntimeSystem for F {
            fn run(&mut self, _: &mut RuntimeContext<'_>) -> RuntimeResult<()> {
                Err(RuntimeError::new(RuntimeErrorCode::SystemFailed, "x"))
            }
        }
        let mut rt = Runtime::new(cfg().with_fail_on_system_error(false)).unwrap();
        rt.initialize().unwrap();
        rt.start().unwrap();
        rt.scheduler_mut()
            .register(HandleId::from_raw(1), "f", 1, Box::new(F))
            .unwrap();
        let record = rt.step().unwrap();
        assert!(!record.succeeded(), "the system did fail");
        assert_eq!(record.state_after(), RuntimeState::Running);
        assert_eq!(rt.state(), RuntimeState::Running);
    }

    // --- Diagnostics emission ---

    #[test]
    fn diagnostics_enabled_emits_a_log_and_a_metric_per_step() {
        let mut rt = started();
        let log_count_before = rt.log_sink().len();
        let metric_count_before = rt.telemetry_sink().len();
        rt.step().unwrap();
        rt.step().unwrap();
        assert_eq!(rt.log_sink().len(), log_count_before + 2);
        assert_eq!(rt.telemetry_sink().len(), metric_count_before + 2);
    }

    #[test]
    fn system_metrics_are_captured_per_step_excluding_internal_counter() {
        use axiom_kernel::{MetricValue, TelemetryMetric};
        struct Emit;
        impl RuntimeSystem for Emit {
            fn run(&mut self, ctx: &mut RuntimeContext<'_>) -> RuntimeResult<()> {
                let tick = ctx.step().tick();
                ctx.metric(TelemetryMetric::gauge(
                    "cube.angle_deg",
                    MetricValue::float(7.0),
                    Some(tick),
                ));
                Ok(())
            }
        }
        // Default config has diagnostics enabled, so the runtime also emits its
        // own `runtime.steps` counter — which must NOT appear in the step's
        // captured metrics.
        let mut rt = started();
        rt.scheduler_mut()
            .register(HandleId::from_raw(1), "emit", 1, Box::new(Emit))
            .unwrap();
        let record = rt.step().unwrap();
        let metrics = record.diagnostics().metrics();
        assert_eq!(metrics.len(), 1, "only the system metric, not runtime.steps");
        assert_eq!(metrics[0].name(), "cube.angle_deg");
        assert_eq!(metrics[0].value(), MetricValue::float(7.0));

        // A runtime with no emitting system captures no metrics.
        let mut bare = started();
        assert!(bare.step().unwrap().diagnostics().metrics().is_empty());
    }

    #[test]
    fn diagnostics_disabled_emits_nothing() {
        let mut rt = Runtime::new(cfg().with_diagnostics_enabled(false)).unwrap();
        rt.initialize().unwrap();
        rt.start().unwrap();
        rt.step().unwrap();
        assert_eq!(rt.log_sink().len(), 0);
        assert_eq!(rt.telemetry_sink().len(), 0);
    }
}

#[cfg(test)]
mod cov {
    use super::*;
    use crate::runtime_system::RuntimeSystem;
    use axiom_kernel::{HandleId, Tick};

    struct AccessorSystem;
    impl RuntimeSystem for AccessorSystem {
        fn run(&mut self, ctx: &mut RuntimeContext<'_>) -> RuntimeResult<()> {
            let _ = ctx.kernel();
            let _ = ctx.step();
            let _ = ctx.commands();
            let _ = ctx.events();
            let _ = ctx.commands_mut();
            ctx.events_mut()
                .push(crate::runtime_event::RuntimeEvent::new(1, Tick::new(0), vec![]));
            Ok(())
        }
    }

    fn started(cfg: RuntimeConfig) -> Runtime {
        let mut rt = Runtime::new(cfg).unwrap();
        rt.initialize().unwrap();
        rt.start().unwrap();
        rt
    }

    #[test]
    fn new_rejects_invalid_config() {
        assert!(Runtime::new(RuntimeConfig::new(0)).is_err());
    }

    #[test]
    fn debug_renders_runtime() {
        let rt = started(RuntimeConfig::new(1_000));
        assert!(format!("{:?}", rt).contains("Runtime"));
    }

    #[test]
    fn diagnostics_enabled_step_keeps_the_runtime_running() {
        let mut rt = started(RuntimeConfig::new(1_000).with_diagnostics_enabled(true));
        rt.scheduler_mut()
            .register(HandleId::from_raw(1), "acc", 1, Box::new(AccessorSystem))
            .unwrap();
        rt.step().unwrap();
        // The diagnostics-enabled step completes without tripping the runtime.
        assert_eq!(rt.state(), RuntimeState::Running);
    }

    #[test]
    fn scheduler_accessor_reflects_registered_systems() {
        // A registered system makes the live scheduler non-empty; a leaked
        // `Default` scheduler would report len 0.
        let mut rt = started(RuntimeConfig::new(1_000));
        rt.scheduler_mut()
            .register(HandleId::from_raw(1), "acc", 1, Box::new(AccessorSystem))
            .unwrap();
        assert_eq!(rt.scheduler().len(), 1);
        assert!(!rt.scheduler().is_empty());
    }

    #[test]
    fn accessors_reflect_a_freshly_started_runtime() {
        let mut rt = started(RuntimeConfig::new(1_000));
        // A freshly started runtime is Running with nothing registered or queued.
        assert_eq!(rt.state(), RuntimeState::Running);
        assert!(rt.scheduler().is_empty());
        assert!(rt.scheduler_mut().is_empty());
        assert_eq!(rt.commands().len(), 0);
        assert_eq!(rt.events().len(), 0);
        // The remaining accessors are reachable on the started runtime.
        let _ = rt.config();
        let tl = rt.timeline();
        let _ = (tl.frame(), tl.tick(), tl.sequence(), tl.elapsed_nanos());
        let _ = (rt.log_sink(), rt.telemetry_sink(), rt.current_step());
    }

    #[test]
    fn step_propagates_clock_overflow() {
        let mut rt = started(RuntimeConfig::new(u64::MAX));
        assert!(rt.step().is_ok()); // 0 + MAX
        assert!(rt.step().is_err()); // MAX + MAX overflows
    }
}
