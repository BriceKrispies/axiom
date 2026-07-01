//! The deterministic process scheduler bookkeeping.
//!
//! `ProcessScheduler` owns scheduler-managed processes (lifecycle-tracked), their
//! wake queue, and their dependency subscriptions. It performs **no** store
//! mutation and emits **no** causal events — that orchestration lives on
//! `SimWorld`, which owns the scheduler alongside the stores and the journal. This
//! split keeps the scheduler a pure, testable state machine and routes all world
//! mutation through the effect boundary.

use std::collections::BTreeMap;

use axiom_ecs::EntityHandle;
use axiom_kernel::{Tick, TickSchedule};

use crate::ids::ProcessId;
use crate::process::ProcessKind;
use crate::process_dependency::{
    DependencyKind, DependencySet, ProcessDependency, ProcessSubscription,
};
use crate::process_handler::{HandlerSpec, ProcessOutput};
use crate::process_lifecycle::{
    ProcessExecutionRecord, ProcessLifecycle, ProcessStatus, ProcessTransition,
};
use crate::sim_tick::SimTick;
use crate::wake_reason::WakeReason;

/// A scheduler-managed process: its kind, subject, lifecycle, and handler spec.
#[derive(Debug, Clone, Copy)]
struct SchedProcess {
    kind: ProcessKind,
    subject: EntityHandle,
    lifecycle: ProcessLifecycle,
    spec: HandlerSpec,
}

/// A process the wake queue surfaced as due and that the scheduler advanced to
/// `Running`; carries everything its handler needs.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DueProcess {
    process: ProcessId,
    subject: EntityHandle,
    spec: HandlerSpec,
    reason: WakeReason,
}

impl DueProcess {
    pub(crate) fn process(&self) -> ProcessId {
        self.process
    }
    pub(crate) fn subject(&self) -> EntityHandle {
        self.subject
    }
    pub(crate) fn spec(&self) -> HandlerSpec {
        self.spec
    }
    pub(crate) fn reason(&self) -> WakeReason {
        self.reason
    }
}

/// The deterministic order processes executed in during a step (ascending by
/// `(wake tick, process id)`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProcessExecutionOrder {
    processes: Vec<ProcessId>,
}

impl ProcessExecutionOrder {
    pub(crate) fn from_vec(processes: Vec<ProcessId>) -> Self {
        ProcessExecutionOrder { processes }
    }
    /// The processes, in execution order.
    pub fn processes(&self) -> &[ProcessId] {
        &self.processes
    }
}

/// One scheduler step's input: the tick to wake due processes for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchedulerStep {
    tick: SimTick,
}

impl SchedulerStep {
    /// A step at `tick`.
    pub const fn new(tick: SimTick) -> Self {
        SchedulerStep { tick }
    }
    /// The step tick.
    pub const fn tick(self) -> SimTick {
        self.tick
    }
}

/// The result of a scheduler step: which processes ran, in order.
#[derive(Debug, Clone, Default)]
pub struct SchedulerStepResult {
    order: ProcessExecutionOrder,
}

impl SchedulerStepResult {
    pub(crate) fn new(order: ProcessExecutionOrder) -> Self {
        SchedulerStepResult { order }
    }
    /// The execution order.
    pub fn order(&self) -> &ProcessExecutionOrder {
        &self.order
    }
}

/// The result of applying the scheduler boundary.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SchedulerBoundary {
    applied_batches: usize,
    applied_effects: usize,
    failed_effects: usize,
}

impl SchedulerBoundary {
    pub(crate) fn new(
        applied_batches: usize,
        applied_effects: usize,
        failed_effects: usize,
    ) -> Self {
        SchedulerBoundary {
            applied_batches,
            applied_effects,
            failed_effects,
        }
    }
    /// How many effect batches were applied.
    pub fn applied_batches(&self) -> usize {
        self.applied_batches
    }
    /// How many individual effects were applied (outcomes recorded).
    pub fn applied_effects(&self) -> usize {
        self.applied_effects
    }
    /// How many effects failed.
    pub fn failed_effects(&self) -> usize {
        self.failed_effects
    }
}

const TERMINAL: [ProcessStatus; 3] = [
    ProcessStatus::Completed,
    ProcessStatus::Canceled,
    ProcessStatus::Failed,
];

fn is_terminal(status: ProcessStatus) -> bool {
    TERMINAL.contains(&status)
}

/// The scheduler's process registry, wake queue, dependencies, and pending
/// (executed-but-not-yet-applied) outputs.
#[derive(Debug, Clone, Default)]
pub struct ProcessScheduler {
    processes: BTreeMap<ProcessId, SchedProcess>,
    // Ordering/bookkeeping delegate to the kernel's `TickSchedule` primitive;
    // `SimTick` <-> kernel `Tick` (both `u64` newtypes) convert at this seam.
    wake_queue: TickSchedule<ProcessId, WakeReason>,
    dependencies: DependencySet,
    pending: Vec<(ProcessId, ProcessOutput)>,
    executions: Vec<ProcessExecutionRecord>,
    next: u64,
}

impl ProcessScheduler {
    /// Create an empty scheduler. The first registered process has id 1.
    pub fn new() -> Self {
        ProcessScheduler {
            processes: BTreeMap::new(),
            wake_queue: TickSchedule::new(),
            dependencies: DependencySet::new(),
            pending: Vec::new(),
            executions: Vec::new(),
            next: 1,
        }
    }

    /// Register a process (status `Scheduled`), returning its id.
    pub(crate) fn register(
        &mut self,
        kind: ProcessKind,
        subject: EntityHandle,
        spec: HandlerSpec,
    ) -> ProcessId {
        let id = ProcessId::from_raw(self.next);
        self.next += 1;
        self.processes.insert(
            id,
            SchedProcess {
                kind,
                subject,
                lifecycle: ProcessLifecycle::new(),
                spec,
            },
        );
        id
    }

    /// The status of a process, if registered.
    pub fn status(&self, process: ProcessId) -> Option<ProcessStatus> {
        self.processes.get(&process).map(|p| p.lifecycle.status())
    }

    /// The kind of a process, if registered.
    pub(crate) fn kind(&self, process: ProcessId) -> Option<ProcessKind> {
        self.processes.get(&process).map(|p| p.kind)
    }

    /// The subject of a process, if registered.
    pub(crate) fn subject(&self, process: ProcessId) -> Option<EntityHandle> {
        self.processes.get(&process).map(|p| p.subject)
    }

    /// Schedule (or move) a process's wake to `tick`. Transitions a `Scheduled`
    /// or `Running` process toward `Sleeping` (best-effort). Returns whether the
    /// process exists.
    pub(crate) fn schedule_wake(
        &mut self,
        process: ProcessId,
        tick: SimTick,
        reason: WakeReason,
    ) -> bool {
        let exists = self.processes.contains_key(&process);
        exists.then(|| {
            self.wake_queue
                .schedule(process, Tick::new(tick.raw()), reason);
            self.processes
                .get_mut(&process)
                .map(|p| p.lifecycle.transition(ProcessStatus::Sleeping, None, tick));
        });
        exists
    }

    /// Cancel a process: remove its wake and transition it to `Canceled`. Returns
    /// whether a live (non-terminal) process was canceled.
    pub(crate) fn cancel(&mut self, process: ProcessId, tick: SimTick) -> bool {
        let cancelable = self
            .status(process)
            .map(|s| !is_terminal(s))
            .unwrap_or(false);
        cancelable.then(|| {
            self.wake_queue.cancel(process);
            self.processes
                .get_mut(&process)
                .map(|p| p.lifecycle.transition(ProcessStatus::Canceled, None, tick));
        });
        cancelable
    }

    /// Subscribe a process to a dependency. Returns `true` if newly added
    /// (`false` if the process is unknown or already subscribed).
    pub(crate) fn subscribe(&mut self, process: ProcessId, dependency: ProcessDependency) -> bool {
        let exists = self.processes.contains_key(&process);
        let subscribed = exists.then(|| self.dependencies.subscribe(process, dependency));
        subscribed.unwrap_or(false)
    }

    /// A process's dependencies, ascending.
    pub fn dependencies_of(&self, process: ProcessId) -> Vec<ProcessDependency> {
        self.dependencies.dependencies_of(process)
    }

    /// A process's subscriptions, ascending.
    pub(crate) fn subscriptions_of(&self, process: ProcessId) -> Vec<ProcessSubscription> {
        self.dependencies.subscriptions_of(process)
    }

    /// The number of distinct subscriptions across all processes.
    pub fn subscription_count(&self) -> usize {
        self.dependencies.len()
    }

    /// Processes subscribed to a `(kind, key)` dependency, ascending.
    pub(crate) fn subscribers_of(&self, kind: DependencyKind, key: u64) -> Vec<ProcessId> {
        self.dependencies
            .subscribers_of(ProcessDependency::new(kind, key))
    }

    /// Pop wakes due at or before `tick`, advancing each live process to
    /// `Running`, and return the runnable due processes in `(tick, id)` order.
    /// Terminal/missing entries are skipped (clean dead-entry handling).
    pub(crate) fn take_due(&mut self, tick: SimTick) -> Vec<DueProcess> {
        let entries = self.wake_queue.pop_due(Tick::new(tick.raw()));
        entries
            .into_iter()
            .filter_map(|(process, reason)| {
                self.processes.get_mut(&process).and_then(|sched| {
                    let alive = !is_terminal(sched.lifecycle.status());
                    alive.then(|| {
                        sched.lifecycle.transition(ProcessStatus::Ready, None, tick);
                        sched
                            .lifecycle
                            .transition(ProcessStatus::Running, None, tick);
                        DueProcess {
                            process,
                            subject: sched.subject,
                            spec: sched.spec,
                            reason,
                        }
                    })
                })
            })
            .collect()
    }

    /// Stash an executed process's output until the boundary.
    pub(crate) fn stash(&mut self, process: ProcessId, output: ProcessOutput) {
        self.pending.push((process, output));
    }

    /// Drain the pending executed outputs (FIFO) for boundary application.
    pub(crate) fn take_pending(&mut self) -> Vec<(ProcessId, ProcessOutput)> {
        std::mem::take(&mut self.pending)
    }

    /// Resolve a running process to `target` at `tick`; if it is a reschedule,
    /// re-arm its wake at `reschedule`. Returns the lifecycle transition, if the
    /// process existed and the transition was legal.
    pub(crate) fn finalize(
        &mut self,
        process: ProcessId,
        target: ProcessStatus,
        reschedule: Option<SimTick>,
        tick: SimTick,
    ) -> Option<ProcessTransition> {
        let transition = self
            .processes
            .get_mut(&process)
            .and_then(|sched| sched.lifecycle.transition(target, None, tick));
        reschedule.into_iter().for_each(|at| {
            self.wake_queue
                .schedule(process, Tick::new(at.raw()), WakeReason::Rescheduled)
        });
        transition
    }

    /// Append an execution record to the scheduler's execution log.
    pub(crate) fn record_execution(&mut self, record: ProcessExecutionRecord) {
        self.executions.push(record);
    }

    /// The execution log, in execution order.
    pub fn execution_records(&self) -> &[ProcessExecutionRecord] {
        &self.executions
    }

    /// The pending wake tick of a process, if any.
    pub fn pending_wake(&self, process: ProcessId) -> Option<SimTick> {
        self.wake_queue
            .pending(process)
            .map(|tick| SimTick::new(tick.raw()))
    }

    /// The processes due at or before `tick`, without consuming them (inspection).
    pub fn due_processes(&self, tick: SimTick) -> Vec<ProcessId> {
        self.wake_queue
            .peek_due(Tick::new(tick.raw()))
            .into_iter()
            .map(|(process, _)| process)
            .collect()
    }

    /// The number of pending wakes in the queue.
    pub fn pending_wake_count(&self) -> usize {
        self.wake_queue.len()
    }

    /// The number of registered processes.
    pub fn len(&self) -> usize {
        self.processes.len()
    }

    /// Whether no processes are registered.
    pub fn is_empty(&self) -> bool {
        self.processes.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process_handler::HandlerSpec;

    fn handle(raw: u64) -> EntityHandle {
        EntityHandle::new(axiom_kernel::EntityId::from_raw(raw), 0)
    }

    #[test]
    fn register_schedule_and_take_due_advances_to_running() {
        let mut sched = ProcessScheduler::new();
        assert!(sched.is_empty());
        let p = sched.register(ProcessKind::new(1), handle(1), HandlerSpec::complete());
        assert_eq!(sched.status(p), Some(ProcessStatus::Scheduled));
        assert_eq!(sched.kind(p), Some(ProcessKind::new(1)));
        assert_eq!(sched.subject(p), Some(handle(1)));
        assert!(sched.schedule_wake(p, SimTick::new(5), WakeReason::Scheduled));
        assert_eq!(sched.status(p), Some(ProcessStatus::Sleeping));
        assert_eq!(sched.pending_wake(p), Some(SimTick::new(5)));
        assert!(sched.take_due(SimTick::new(4)).is_empty());
        assert_eq!(sched.due_processes(SimTick::new(5)), vec![p]);
        let due = sched.take_due(SimTick::new(5));
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].process(), p);
        assert_eq!(due[0].reason(), WakeReason::Scheduled);
        assert_eq!(sched.status(p), Some(ProcessStatus::Running));
        assert!(!sched.schedule_wake(
            ProcessId::from_raw(99),
            SimTick::new(1),
            WakeReason::Scheduled
        ));
    }

    #[test]
    fn finalize_completes_and_reschedules() {
        let mut sched = ProcessScheduler::new();
        let p = sched.register(ProcessKind::new(1), handle(1), HandlerSpec::complete());
        sched.schedule_wake(p, SimTick::new(0), WakeReason::Scheduled);
        sched.take_due(SimTick::new(0));
        let transition = sched
            .finalize(
                p,
                ProcessStatus::Sleeping,
                Some(SimTick::new(9)),
                SimTick::new(0),
            )
            .unwrap();
        assert_eq!(transition.to(), ProcessStatus::Sleeping);
        assert_eq!(transition.from(), ProcessStatus::Running);
        assert_eq!(sched.status(p), Some(ProcessStatus::Sleeping));
        assert_eq!(sched.pending_wake(p), Some(SimTick::new(9)));
        sched.record_execution(crate::process_lifecycle::ProcessExecutionRecord::new(
            p, 0, transition,
        ));
        assert_eq!(sched.execution_records().len(), 1);
        assert_eq!(sched.execution_records()[0].process(), p);
        sched.take_due(SimTick::new(9));
        let done = sched
            .finalize(p, ProcessStatus::Completed, None, SimTick::new(9))
            .unwrap();
        assert_eq!(done.to(), ProcessStatus::Completed);
    }

    #[test]
    fn cancel_is_terminal_and_skips_dead_due_entries() {
        let mut sched = ProcessScheduler::new();
        let p = sched.register(ProcessKind::new(1), handle(1), HandlerSpec::complete());
        sched.schedule_wake(p, SimTick::new(5), WakeReason::Scheduled);
        assert!(sched.cancel(p, SimTick::new(1)));
        assert_eq!(sched.status(p), Some(ProcessStatus::Canceled));
        assert!(!sched.cancel(p, SimTick::new(2)));
        assert!(sched.take_due(SimTick::new(5)).is_empty());
    }

    #[test]
    fn subscribe_requires_known_process_and_dedups() {
        let mut sched = ProcessScheduler::new();
        let p = sched.register(ProcessKind::new(1), handle(1), HandlerSpec::complete());
        let dep = ProcessDependency::new(DependencyKind::FactKindChanged, 7);
        assert!(sched.subscribe(p, dep));
        assert!(!sched.subscribe(p, dep), "dedup");
        assert!(
            !sched.subscribe(ProcessId::from_raw(99), dep),
            "unknown process"
        );
        assert_eq!(
            sched.subscribers_of(DependencyKind::FactKindChanged, 7),
            vec![p]
        );
        assert_eq!(sched.dependencies_of(p).len(), 1);
        assert_eq!(sched.subscription_count(), 1);
        let subs = sched.subscriptions_of(p);
        assert_eq!(subs.len(), 1);
        assert_eq!(subs[0].process(), p);
        assert_eq!(subs[0].dependency(), dep);
    }

    #[test]
    fn stash_and_take_pending_round_trip() {
        use crate::process_handler::{ProcessContext, ProcessHandler};
        let mut sched = ProcessScheduler::new();
        let p = sched.register(ProcessKind::new(1), handle(1), HandlerSpec::complete());
        let ctx = ProcessContext::new(handle(1), SimTick::new(0));
        let output = HandlerSpec::complete().run(&ctx);
        sched.stash(p, output);
        let pending = sched.take_pending();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].0, p);
        assert!(sched.take_pending().is_empty(), "pending drained");
    }

    #[test]
    fn step_and_boundary_result_accessors() {
        let order =
            ProcessExecutionOrder::from_vec(vec![ProcessId::from_raw(1), ProcessId::from_raw(2)]);
        assert_eq!(order.processes().len(), 2);
        let result = SchedulerStepResult::new(order);
        assert_eq!(result.order().processes().len(), 2);
        let step = SchedulerStep::new(SimTick::new(3));
        assert_eq!(step.tick(), SimTick::new(3));
        let boundary = SchedulerBoundary::new(2, 3, 1);
        assert_eq!(
            (
                boundary.applied_batches(),
                boundary.applied_effects(),
                boundary.failed_effects()
            ),
            (2, 3, 1)
        );
        assert_eq!(
            SchedulerBoundary::default(),
            SchedulerBoundary::new(0, 0, 0)
        );
    }
}
