//! The main runtime: owns lifecycle state and drives deterministic stepping.

use axiom_kernel::{InMemoryLogSink, InMemoryTelemetrySink, KernelApi};

use crate::preparation_schedule::PreparationSchedule;
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
        config.validate(&kernel).map(|fixed_step| {
            let clock = kernel.simulation_clock(fixed_step);
            let log_sink = kernel.log_sink();
            let telemetry_sink = kernel.telemetry_sink();
            Runtime {
                kernel,
                config,
                state: RuntimeState::Created,
                timeline: RuntimeTimeline::new(clock),
                scheduler: RuntimeScheduler::new(),
                commands: RuntimeCommandQueue::new(),
                events: RuntimeEventQueue::new(),
                log_sink,
                telemetry_sink,
            }
        })
    }

    /// Transition `Created` → `Initialized`.
    pub fn initialize(&mut self) -> RuntimeResult<()> {
        (self.state == RuntimeState::Created)
            .then_some(RuntimeState::Initialized)
            .map_or(
                Err(invalid_transition("initialize requires Created")),
                |next| {
                    self.state = next;
                    Ok(())
                },
            )
    }

    /// Run the startup preparation phase: `Initialized` → `Prepared` (every
    /// task returned `Ok`) or `Failed` (any task returned `Err`).
    ///
    /// This is the **preparation barrier**. `start` is reachable only from
    /// `Prepared`, so a simulation cannot begin stepping until every task in
    /// `schedule` has run to completion. Preparation runs exactly once per
    /// launch: calling `prepare` from any state other than `Initialized` —
    /// including `Prepared` itself — returns
    /// [`RuntimeErrorCode::InvalidLifecycleTransition`].
    ///
    /// The schedule is taken **by value** and dropped before this returns. That
    /// is what makes "temporary startup work dies at the barrier" a guarantee
    /// rather than a convention: no task survives the phase, so none can be
    /// re-run, inspected, or accidentally driven from the frame loop.
    ///
    /// On failure the returned error keeps **both** facts about what went wrong:
    /// its message is the failing task's name, and its code is the code that
    /// task itself returned. The runtime does not overwrite a task's diagnosis
    /// with [`RuntimeErrorCode::PreparationFailed`]; that code exists for a
    /// *task* to use when it has nothing more specific to say.
    #[axiom_zones::sim]
    pub fn prepare(&mut self, schedule: PreparationSchedule) -> RuntimeResult<()> {
        (self.state == RuntimeState::Initialized)
            .then_some(schedule)
            .map_or(
                Err(invalid_transition("prepare requires Initialized")),
                |s| self.run_preparation(s),
            )
    }

    /// The body of the preparation phase: run every task in push order, settle
    /// the lifecycle on the outcome, and drop the schedule.
    ///
    /// Taking `schedule` by value is deliberate — it dies here, at the barrier.
    #[axiom_zones::sim]
    fn run_preparation(&mut self, mut schedule: PreparationSchedule) -> RuntimeResult<()> {
        let failure = schedule.execute();
        self.state = [RuntimeState::Prepared, RuntimeState::Failed][usize::from(failure.is_some())];
        failure.map_or(Ok(()), |(name, cause)| {
            Err(RuntimeError::new(cause.code(), name))
        })
    }

    /// Transition `Prepared` or `Paused` → `Running`.
    ///
    /// `Initialized` is deliberately **not** accepted: an initialized runtime
    /// has not yet run its preparation phase, and starting one would let a
    /// simulation step over a world that was never built. Reaching `Running`
    /// from a fresh runtime is `initialize()`, then
    /// [`Runtime::prepare`], then `start()`. Resuming from `Paused` needs no
    /// second preparation — the phase already ran for this launch.
    pub fn start(&mut self) -> RuntimeResult<()> {
        ((self.state == RuntimeState::Prepared) | (self.state == RuntimeState::Paused))
            .then_some(RuntimeState::Running)
            .map_or(
                Err(invalid_transition("start requires Prepared or Paused")),
                |next| {
                    self.state = next;
                    Ok(())
                },
            )
    }

    /// Transition `Running` → `Paused`.
    pub fn pause(&mut self) -> RuntimeResult<()> {
        (self.state == RuntimeState::Running)
            .then_some(RuntimeState::Paused)
            .map_or(Err(invalid_transition("pause requires Running")), |next| {
                self.state = next;
                Ok(())
            })
    }

    /// Transition `Running`, `Paused`, `Initialized`, or `Prepared` → `Stopped`.
    /// Terminal states (`Stopped`, `Failed`) are rejected.
    ///
    /// A runtime that has completed preparation but never started is still a
    /// live runtime holding prepared products, so shutting it down is legal.
    pub fn stop(&mut self) -> RuntimeResult<()> {
        ((self.state == RuntimeState::Running)
            | (self.state == RuntimeState::Paused)
            | (self.state == RuntimeState::Initialized)
            | (self.state == RuntimeState::Prepared))
            .then_some(RuntimeState::Stopped)
            .map_or(
                Err(invalid_transition(
                    "stop requires Running, Paused, Initialized, or Prepared",
                )),
                |next| {
                    self.state = next;
                    Ok(())
                },
            )
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
        (self.state == RuntimeState::Running)
            .then_some(())
            .ok_or_else(|| {
                RuntimeError::new(
                    RuntimeErrorCode::StepWhileNotRunning,
                    "step() requires the runtime to be in Running",
                )
            })
            .and_then(|()| self.run_one_step())
    }

    /// The body of one `Running` step: advance the timeline, run the scheduler,
    /// drain the queues, and record the result.
    ///
    /// Carries its own `#[sim]` marker: the zone lint matches a marker only on
    /// the function it is attached to, so `step`'s marker does not reach here.
    #[axiom_zones::sim]
    fn run_one_step(&mut self) -> RuntimeResult<RuntimeStepRecord> {
        let commands_before = self.commands.len();
        let events_before = self.events.len();
        let metrics_before = self.telemetry_sink.len();

        self.timeline.advance().map(|step| {
            let mut diagnostics = RuntimeDiagnostics::new(step);

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

            diagnostics.record_metrics(self.telemetry_sink.metrics()[metrics_before..].to_vec());

            let commands_after = self.commands.len();
            let events_after = self.events.len();
            let commands_pushed = commands_after
                .saturating_sub(commands_before)
                .min(u32::MAX as usize) as u32;
            let events_pushed = events_after
                .saturating_sub(events_before)
                .min(u32::MAX as usize) as u32;

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

            (any_error & self.config.fail_on_system_error())
                .then(|| self.state = RuntimeState::Failed);

            self.config
                .diagnostics_enabled()
                .then(|| self.emit_step_summary(&diagnostics));

            RuntimeStepRecord::new(
                step,
                diagnostics,
                self.state,
                self.commands.len(),
                self.events.len(),
            )
        })
    }

    /// Emit a `LogRecord` summarizing the just-completed step into the
    /// runtime's in-memory log sink, via the kernel.
    fn emit_step_summary(&mut self, diagnostics: &RuntimeDiagnostics) {
        use axiom_kernel::{LogField, LogLevel, LogRecord, TelemetryMetric};

        let level = [LogLevel::Error, LogLevel::Info][usize::from(diagnostics.errors().is_empty())];
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
    use crate::preparation_task::PreparationTask;
    use crate::runtime_command::RuntimeCommand;
    use crate::runtime_event::RuntimeEvent;
    use crate::runtime_system::RuntimeSystem;
    use axiom_kernel::{HandleId, Tick};
    use std::cell::Cell;
    use std::rc::Rc;

    fn cfg() -> RuntimeConfig {
        RuntimeConfig::new(1_000)
    }

    /// Drive a fresh runtime all the way to `Running` through the barrier.
    fn started() -> Runtime {
        let mut rt = Runtime::new(cfg()).unwrap();
        rt.initialize().unwrap();
        rt.prepare(PreparationSchedule::new()).unwrap();
        rt.start().unwrap();
        rt
    }

    /// Counts its runs into a cell the caller keeps, so a test can assert how
    /// many times preparation actually executed it.
    struct Counting {
        runs: Rc<Cell<u32>>,
    }

    impl PreparationTask for Counting {
        fn prepare(&mut self) -> RuntimeResult<()> {
            self.runs.set(self.runs.get() + 1);
            Ok(())
        }
    }

    /// Fails with a code of its own choosing, after recording that it ran.
    struct Failing {
        runs: Rc<Cell<u32>>,
        code: RuntimeErrorCode,
    }

    impl PreparationTask for Failing {
        fn prepare(&mut self) -> RuntimeResult<()> {
            self.runs.set(self.runs.get() + 1);
            Err(RuntimeError::new(self.code, "intentional"))
        }
    }

    fn counter() -> Rc<Cell<u32>> {
        Rc::new(Cell::new(0))
    }

    fn counting(runs: &Rc<Cell<u32>>) -> Box<dyn PreparationTask> {
        Box::new(Counting { runs: runs.clone() })
    }

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
        rt.prepare(PreparationSchedule::new()).unwrap();
        assert_eq!(rt.state(), RuntimeState::Prepared);
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

    /// **The barrier.** An initialized runtime has not prepared, so it cannot
    /// start — and therefore cannot step. This is the whole point of the phase:
    /// the transition that used to be legal is now the one that is refused.
    #[test]
    fn start_without_preparation_is_rejected() {
        let mut rt = Runtime::new(cfg()).unwrap();
        rt.initialize().unwrap();

        let err = rt.start().unwrap_err();
        assert_eq!(err.code(), RuntimeErrorCode::InvalidLifecycleTransition);
        assert_eq!(
            rt.state(),
            RuntimeState::Initialized,
            "the refused transition left the state untouched"
        );
        assert_eq!(
            rt.step().unwrap_err().code(),
            RuntimeErrorCode::StepWhileNotRunning,
            "and the simulation cannot advance behind start()'s back"
        );
    }

    #[test]
    fn start_from_created_is_rejected() {
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
        rt.prepare(PreparationSchedule::new()).unwrap();
        rt.start().unwrap();
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

        assert_eq!(record.diagnostics().commands_pushed(), 2);
        assert_eq!(record.diagnostics().events_pushed(), 1);
        assert_eq!(record.diagnostics().commands_drained(), 2);
        assert_eq!(record.diagnostics().events_drained(), 1);
        assert!(rt.commands().is_empty());
        assert!(rt.events().is_empty());
    }

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
        rt.prepare(PreparationSchedule::new()).unwrap();
        rt.start().unwrap();
        rt.scheduler_mut()
            .register(HandleId::from_raw(1), "f", 1, Box::new(F))
            .unwrap();
        let record = rt.step().unwrap();
        assert!(!record.succeeded(), "the system did fail");
        assert_eq!(record.state_after(), RuntimeState::Running);
        assert_eq!(rt.state(), RuntimeState::Running);
    }

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
        // Diagnostics are enabled by default, so the runtime also emits its own
        // `runtime.steps` counter, which must not appear in captured metrics.
        let mut rt = started();
        rt.scheduler_mut()
            .register(HandleId::from_raw(1), "emit", 1, Box::new(Emit))
            .unwrap();
        let record = rt.step().unwrap();
        let metrics = record.diagnostics().metrics();
        assert_eq!(
            metrics.len(),
            1,
            "only the system metric, not runtime.steps"
        );
        assert_eq!(metrics[0].name(), "cube.angle_deg");
        assert_eq!(metrics[0].value(), MetricValue::float(7.0));

        let mut bare = started();
        assert!(bare.step().unwrap().diagnostics().metrics().is_empty());
    }

    #[test]
    fn diagnostics_disabled_emits_nothing() {
        let mut rt = Runtime::new(cfg().with_diagnostics_enabled(false)).unwrap();
        rt.initialize().unwrap();
        rt.prepare(PreparationSchedule::new()).unwrap();
        rt.start().unwrap();
        rt.step().unwrap();
        assert_eq!(rt.log_sink().len(), 0);
        assert_eq!(rt.telemetry_sink().len(), 0);
    }

    #[test]
    fn preparation_runs_before_running() {
        let runs = counter();
        let mut schedule = PreparationSchedule::new();
        schedule.push("work", counting(&runs));

        let mut rt = Runtime::new(cfg()).unwrap();
        rt.initialize().unwrap();
        assert_eq!(runs.get(), 0, "nothing ran before prepare()");

        rt.prepare(schedule).unwrap();
        assert_eq!(runs.get(), 1, "the task ran during prepare()");
        assert_eq!(
            rt.state(),
            RuntimeState::Prepared,
            "and completed work leaves the runtime prepared, not running"
        );
    }

    #[test]
    fn successful_preparation_permits_the_transition() {
        let runs = counter();
        let mut schedule = PreparationSchedule::new();
        schedule.push("work", counting(&runs));

        let mut rt = Runtime::new(cfg()).unwrap();
        rt.initialize().unwrap();
        rt.prepare(schedule).unwrap();
        rt.start().unwrap();

        assert_eq!(rt.state(), RuntimeState::Running);
        assert!(rt.step().is_ok(), "and stepping is now permitted");
    }

    #[test]
    fn failed_preparation_blocks_the_transition() {
        let runs = counter();
        let mut schedule = PreparationSchedule::new();
        schedule.push(
            "boom",
            Box::new(Failing {
                runs: runs.clone(),
                code: RuntimeErrorCode::PreparationFailed,
            }),
        );

        let mut rt = Runtime::new(cfg()).unwrap();
        rt.initialize().unwrap();
        assert!(rt.prepare(schedule).is_err());

        assert_eq!(rt.state(), RuntimeState::Failed, "Failed is terminal");
        assert_eq!(
            rt.start().unwrap_err().code(),
            RuntimeErrorCode::InvalidLifecycleTransition
        );
    }

    #[test]
    fn a_failing_task_stops_the_remaining_tasks() {
        let before = counter();
        let failed = counter();
        let after = counter();
        let mut schedule = PreparationSchedule::new();
        schedule.push("before", counting(&before));
        schedule.push(
            "boom",
            Box::new(Failing {
                runs: failed.clone(),
                code: RuntimeErrorCode::SystemFailed,
            }),
        );
        schedule.push("after", counting(&after));

        let mut rt = Runtime::new(cfg()).unwrap();
        rt.initialize().unwrap();
        assert!(rt.prepare(schedule).is_err());

        assert_eq!(before.get(), 1, "the task before the failure ran");
        assert_eq!(failed.get(), 1, "the failing task ran");
        assert_eq!(after.get(), 0, "the task after the failure did not");
    }

    #[test]
    fn the_error_names_the_failing_task_and_keeps_its_code() {
        let runs = counter();
        let mut schedule = PreparationSchedule::new();
        schedule.push("first", counting(&runs));
        schedule.push(
            "course-compile",
            Box::new(Failing {
                runs: counter(),
                // A code of the task's own choosing, deliberately *not*
                // PreparationFailed — the runtime must not overwrite it.
                code: RuntimeErrorCode::KernelFailure,
            }),
        );

        let mut rt = Runtime::new(cfg()).unwrap();
        rt.initialize().unwrap();
        let err = rt.prepare(schedule).unwrap_err();

        assert_eq!(
            err.message(),
            "course-compile",
            "the message identifies which task failed"
        );
        assert_eq!(
            err.code(),
            RuntimeErrorCode::KernelFailure,
            "the task's own diagnosis survives the barrier"
        );
    }

    #[test]
    fn preparation_runs_exactly_once_per_launch() {
        let runs = counter();
        let mut schedule = PreparationSchedule::new();
        schedule.push("work", counting(&runs));

        let mut rt = Runtime::new(cfg()).unwrap();
        rt.initialize().unwrap();
        rt.prepare(schedule).unwrap();

        assert_eq!(
            rt.prepare(PreparationSchedule::new()).unwrap_err().code(),
            RuntimeErrorCode::InvalidLifecycleTransition,
            "a second phase is refused from Prepared"
        );

        rt.start().unwrap();
        assert_eq!(
            rt.prepare(PreparationSchedule::new()).unwrap_err().code(),
            RuntimeErrorCode::InvalidLifecycleTransition,
            "and from Running"
        );

        rt.pause().unwrap();
        assert_eq!(
            rt.prepare(PreparationSchedule::new()).unwrap_err().code(),
            RuntimeErrorCode::InvalidLifecycleTransition,
            "and from Paused"
        );

        assert_eq!(runs.get(), 1, "the task ran exactly once");
    }

    #[test]
    fn an_empty_schedule_prepares_immediately() {
        let mut rt = Runtime::new(cfg()).unwrap();
        rt.initialize().unwrap();

        assert!(rt.prepare(PreparationSchedule::new()).is_ok());
        assert_eq!(rt.state(), RuntimeState::Prepared);
    }

    #[test]
    fn stepping_does_not_rerun_preparation() {
        let runs = counter();
        let mut schedule = PreparationSchedule::new();
        schedule.push("work", counting(&runs));

        let mut rt = Runtime::new(cfg()).unwrap();
        rt.initialize().unwrap();
        rt.prepare(schedule).unwrap();
        rt.start().unwrap();
        (0..100).for_each(|_| {
            rt.step().unwrap();
        });

        assert_eq!(runs.get(), 1, "100 steps re-ran no preparation task");
    }

    #[test]
    fn preparation_is_rejected_before_initialize() {
        let runs = counter();
        let mut schedule = PreparationSchedule::new();
        schedule.push("work", counting(&runs));

        let mut rt = Runtime::new(cfg()).unwrap();
        let err = rt.prepare(schedule).unwrap_err();

        assert_eq!(err.code(), RuntimeErrorCode::InvalidLifecycleTransition);
        assert_eq!(rt.state(), RuntimeState::Created, "state is untouched");
        assert_eq!(runs.get(), 0, "and a refused phase runs no task");
    }

    #[test]
    fn preparation_is_rejected_from_terminal_states() {
        let mut stopped = Runtime::new(cfg()).unwrap();
        stopped.initialize().unwrap();
        stopped.stop().unwrap();
        assert_eq!(
            stopped
                .prepare(PreparationSchedule::new())
                .unwrap_err()
                .code(),
            RuntimeErrorCode::InvalidLifecycleTransition
        );

        let mut failed = Runtime::new(cfg()).unwrap();
        failed.initialize().unwrap();
        let mut schedule = PreparationSchedule::new();
        schedule.push(
            "boom",
            Box::new(Failing {
                runs: counter(),
                code: RuntimeErrorCode::PreparationFailed,
            }),
        );
        assert!(failed.prepare(schedule).is_err());
        assert_eq!(
            failed
                .prepare(PreparationSchedule::new())
                .unwrap_err()
                .code(),
            RuntimeErrorCode::InvalidLifecycleTransition,
            "a failed phase cannot be retried"
        );
    }

    #[test]
    fn stop_is_legal_from_prepared() {
        let mut rt = Runtime::new(cfg()).unwrap();
        rt.initialize().unwrap();
        rt.prepare(PreparationSchedule::new()).unwrap();

        rt.stop().unwrap();
        assert_eq!(rt.state(), RuntimeState::Stopped);
    }

    #[test]
    fn pause_and_resume_do_not_reenter_preparation() {
        let runs = counter();
        let mut schedule = PreparationSchedule::new();
        schedule.push("work", counting(&runs));

        let mut rt = Runtime::new(cfg()).unwrap();
        rt.initialize().unwrap();
        rt.prepare(schedule).unwrap();
        rt.start().unwrap();
        rt.pause().unwrap();
        rt.start().unwrap();

        assert_eq!(rt.state(), RuntimeState::Running);
        assert_eq!(runs.get(), 1, "resuming needs no second phase");
    }

    #[test]
    fn a_failed_preparation_leaves_the_step_gate_closed() {
        let mut schedule = PreparationSchedule::new();
        schedule.push(
            "boom",
            Box::new(Failing {
                runs: counter(),
                code: RuntimeErrorCode::PreparationFailed,
            }),
        );

        let mut rt = Runtime::new(cfg()).unwrap();
        rt.initialize().unwrap();
        assert!(rt.prepare(schedule).is_err());

        assert_eq!(
            rt.step().unwrap_err().code(),
            RuntimeErrorCode::StepWhileNotRunning
        );
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
                .push(crate::runtime_event::RuntimeEvent::new(
                    1,
                    Tick::new(0),
                    vec![],
                ));
            Ok(())
        }
    }

    fn started(cfg: RuntimeConfig) -> Runtime {
        let mut rt = Runtime::new(cfg).unwrap();
        rt.initialize().unwrap();
        rt.prepare(PreparationSchedule::new()).unwrap();
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
        assert_eq!(rt.state(), RuntimeState::Running);
    }

    #[test]
    fn scheduler_accessor_reflects_registered_systems() {
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
        assert_eq!(rt.state(), RuntimeState::Running);
        assert!(rt.scheduler().is_empty());
        assert!(rt.scheduler_mut().is_empty());
        assert_eq!(rt.commands().len(), 0);
        assert_eq!(rt.events().len(), 0);
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
