//! The process-scheduler facade surface of `SimCoreApi`.
//!
//! A child module of `facade`, so it may use the private `world` field. Ticks,
//! kinds, statuses, and reasons cross the facade as plain integer codes; the
//! internal `SimTick`/`HandlerSpec`/enum types are never named by consumers.

use axiom_ecs::{EntityHandle, EntityRegistry};

use crate::cause::CauseRef;
use crate::dirty_set::{DirtyFact, DirtyKind, DirtyRelation, DirtySubject};
use crate::fact::FactValue;
use crate::ids::{CausalEventId, FactId, ProcessId, RelationId};
use crate::process_dependency::DependencyKind;
use crate::process_handler::HandlerSpec;
use crate::process_lifecycle::ProcessExecutionRecord;
use crate::process_wake_queue::WakeReason;
use crate::scheduler::SchedulerStep;
use crate::sim_tick::{SimTick, TickDelta};
use crate::sim_world;

use super::SimCoreApi;

impl SimCoreApi {
    // --- dependency-kind codes ---
    /// Dependency code: a fact of a kind changed (key = fact kind).
    pub const DEP_FACT_KIND: u8 = 0;
    /// Dependency code: a relation of a kind changed (key = relation kind).
    pub const DEP_RELATION_KIND: u8 = 1;
    /// Dependency code: a subject changed (key = entity slot raw id).
    pub const DEP_SUBJECT: u8 = 2;
    /// Dependency code: a definition changed (key = definition id raw).
    pub const DEP_DEFINITION: u8 = 3;
    /// Dependency code: a residue changed (key = residue id raw).
    pub const DEP_RESIDUE: u8 = 4;
    /// Dependency code: a body surface changed (key = surface id raw).
    pub const DEP_BODY_SURFACE: u8 = 5;
    /// Dependency code: a wound changed (key = wound id raw).
    pub const DEP_WOUND: u8 = 6;
    /// Dependency code: an explicit dependency on another process (key = id raw).
    pub const DEP_PROCESS: u8 = 7;
    /// Dependency code: a generic dependency.
    pub const DEP_GENERIC: u8 = 8;

    // --- dirty-kind codes ---
    /// Dirty code: added.
    pub const DIRTY_ADDED: u8 = 0;
    /// Dirty code: updated.
    pub const DIRTY_UPDATED: u8 = 1;
    /// Dirty code: removed.
    pub const DIRTY_REMOVED: u8 = 2;
    /// Dirty code: touched.
    pub const DIRTY_TOUCHED: u8 = 3;
    /// Dirty code: dependency-invalidated.
    pub const DIRTY_DEPENDENCY_INVALIDATED: u8 = 4;

    // --- process status codes ---
    /// Status code: scheduled.
    pub const STATUS_SCHEDULED: u8 = 0;
    /// Status code: sleeping.
    pub const STATUS_SLEEPING: u8 = 1;
    /// Status code: ready.
    pub const STATUS_READY: u8 = 2;
    /// Status code: running.
    pub const STATUS_RUNNING: u8 = 3;
    /// Status code: completed.
    pub const STATUS_COMPLETED: u8 = 4;
    /// Status code: canceled.
    pub const STATUS_CANCELED: u8 = 5;
    /// Status code: failed.
    pub const STATUS_FAILED: u8 = 6;
}

impl SimCoreApi {
    // --- scheduler causal-event codes (for filtering causal records) ---
    /// Causal code: a process was scheduled (registered).
    pub const SCHED_EVENT_SCHEDULED: u32 = sim_world::SCHED_PROCESS_SCHEDULED;
    /// Causal code: a process woke.
    pub const SCHED_EVENT_WOKE: u32 = sim_world::SCHED_PROCESS_WOKE;
    /// Causal code: a process started executing.
    pub const SCHED_EVENT_STARTED: u32 = sim_world::SCHED_PROCESS_STARTED;
    /// Causal code: a process completed.
    pub const SCHED_EVENT_COMPLETED: u32 = sim_world::SCHED_PROCESS_COMPLETED;
    /// Causal code: a process slept / rescheduled.
    pub const SCHED_EVENT_SLEPT: u32 = sim_world::SCHED_PROCESS_SLEPT;
    /// Causal code: a process was canceled.
    pub const SCHED_EVENT_CANCELED: u32 = sim_world::SCHED_PROCESS_CANCELED;
    /// Causal code: a process failed.
    pub const SCHED_EVENT_FAILED: u32 = sim_world::SCHED_PROCESS_FAILED;
    /// Causal code: a dirty invalidation was recorded.
    pub const SCHED_EVENT_DIRTY_INVALIDATION: u32 = sim_world::SCHED_DIRTY_INVALIDATION;
    /// Causal code: a process was woken by a dirty dependency.
    pub const SCHED_EVENT_WOKEN_BY_DIRTY: u32 = sim_world::SCHED_WOKEN_BY_DIRTY;
    /// Causal code: a process produced an effect batch.
    pub const SCHED_EVENT_PRODUCED_EFFECTS: u32 = sim_world::SCHED_PRODUCED_EFFECTS;
    /// Causal code: an effect batch was applied.
    pub const SCHED_EVENT_EFFECTS_APPLIED: u32 = sim_world::SCHED_EFFECTS_APPLIED;

    /// Add `delta` ticks to `tick`, `None` on overflow (checked logical-tick math).
    pub fn tick_add(&self, tick: u64, delta: u64) -> Option<u64> {
        SimTick::new(tick)
            .checked_add(TickDelta::new(delta))
            .map(SimTick::raw)
    }
}

impl SimCoreApi {
    /// Register a process whose handler completes immediately (no effects).
    pub fn register_process(&mut self, kind: u32, subject: EntityHandle, tick: u64) -> ProcessId {
        self.world.register_scheduler_process(
            kind,
            subject,
            HandlerSpec::complete(),
            SimTick::new(tick),
        )
    }

    /// Register a process whose handler updates `fact` to `value` then completes.
    pub fn register_process_updating_fact(
        &mut self,
        kind: u32,
        subject: EntityHandle,
        fact: FactId,
        value: FactValue,
        effect_tick: u64,
        tick: u64,
    ) -> ProcessId {
        let spec = HandlerSpec::update_fact_then_complete(fact, value, effect_tick);
        self.world
            .register_scheduler_process(kind, subject, spec, SimTick::new(tick))
    }

    /// Register a process whose handler adds a fact (of `fact_kind`, on its
    /// subject) then completes.
    pub fn register_process_adding_fact(
        &mut self,
        kind: u32,
        subject: EntityHandle,
        fact_kind: u32,
        value: FactValue,
        effect_tick: u64,
        tick: u64,
    ) -> ProcessId {
        let spec = HandlerSpec::add_fact_then_complete(fact_kind, value, effect_tick);
        self.world
            .register_scheduler_process(kind, subject, spec, SimTick::new(tick))
    }

    /// Register a process whose handler requests a reschedule `delta` ticks later.
    pub fn register_process_rescheduling(
        &mut self,
        kind: u32,
        subject: EntityHandle,
        delta: u64,
        tick: u64,
    ) -> ProcessId {
        let spec = HandlerSpec::reschedule_after(TickDelta::new(delta));
        self.world
            .register_scheduler_process(kind, subject, spec, SimTick::new(tick))
    }

    /// Register a process whose handler fails.
    pub fn register_failing_process(
        &mut self,
        kind: u32,
        subject: EntityHandle,
        tick: u64,
    ) -> ProcessId {
        self.world.register_scheduler_process(
            kind,
            subject,
            HandlerSpec::fail(),
            SimTick::new(tick),
        )
    }

    /// Register a process whose handler cancels.
    pub fn register_canceling_process(
        &mut self,
        kind: u32,
        subject: EntityHandle,
        tick: u64,
    ) -> ProcessId {
        self.world.register_scheduler_process(
            kind,
            subject,
            HandlerSpec::cancel(),
            SimTick::new(tick),
        )
    }

    /// Wake-reason code: scheduled.
    pub const WAKE_SCHEDULED: u8 = 0;
    /// Wake-reason code: rescheduled.
    pub const WAKE_RESCHEDULED: u8 = 1;
    /// Wake-reason code: dirty dependency.
    pub const WAKE_DIRTY_DEPENDENCY: u8 = 2;
    /// Wake-reason code: explicit.
    pub const WAKE_EXPLICIT: u8 = 3;
    /// Wake-reason code: generic.
    pub const WAKE_GENERIC: u8 = 4;

    /// Schedule a process to wake at `tick` (explicit wake). Returns whether the
    /// process exists.
    pub fn schedule_process_wake(&mut self, process: ProcessId, tick: u64) -> bool {
        self.world
            .schedule_scheduler_wake(process, SimTick::new(tick), WakeReason::Explicit)
    }

    /// Schedule a process to wake at `tick` with an explicit reason code. Returns
    /// `false` if the process is unknown or the reason code is out of range.
    pub fn schedule_process_wake_with_reason(
        &mut self,
        process: ProcessId,
        tick: u64,
        reason_code: u8,
    ) -> bool {
        WakeReason::from_code(reason_code)
            .map(|reason| {
                self.world
                    .schedule_scheduler_wake(process, SimTick::new(tick), reason)
            })
            .unwrap_or(false)
    }

    /// Reschedule a process's wake to `tick`. Returns whether the process exists.
    pub fn reschedule_process_wake(&mut self, process: ProcessId, tick: u64) -> bool {
        self.world
            .schedule_scheduler_wake(process, SimTick::new(tick), WakeReason::Rescheduled)
    }

    /// Cancel a scheduler process. Returns whether a live process was canceled.
    pub fn cancel_scheduler_process(&mut self, process: ProcessId, tick: u64) -> bool {
        self.world
            .cancel_scheduler_process(process, SimTick::new(tick))
    }
}

impl SimCoreApi {
    /// Subscribe a process to a dependency (`dep_code` + selector `key`). Returns
    /// `false` if the code is out of range, the process is unknown, or it was
    /// already subscribed.
    pub fn subscribe_process(&mut self, process: ProcessId, dep_code: u8, key: u64) -> bool {
        DependencyKind::from_code(dep_code)
            .map(|kind| self.world.subscribe_dependency(process, kind, key))
            .unwrap_or(false)
    }

    /// The dependencies a process is subscribed to, as `(dep_code, key)`,
    /// ascending.
    pub fn process_dependencies(&self, process: ProcessId) -> Vec<(u8, u64)> {
        self.world
            .scheduler()
            .dependencies_of(process)
            .into_iter()
            .map(|dependency| (dependency.kind().code(), dependency.key()))
            .collect()
    }

    /// Mark a fact dirty (`dirty_code`). Returns whether the code was valid.
    pub fn mark_dirty_fact(&mut self, fact: FactId, fact_kind: u32, dirty_code: u8) -> bool {
        DirtyKind::from_code(dirty_code)
            .map(|kind| self.world.mark_dirty_fact(fact, fact_kind, kind, None))
            .is_some()
    }

    /// Mark a relation dirty (`dirty_code`). Returns whether the code was valid.
    pub fn mark_dirty_relation(
        &mut self,
        relation: RelationId,
        relation_kind: u32,
        dirty_code: u8,
    ) -> bool {
        DirtyKind::from_code(dirty_code)
            .map(|kind| {
                self.world
                    .mark_dirty_relation(relation, relation_kind, kind, None)
            })
            .is_some()
    }

    /// Mark a subject dirty (`dirty_code`). Returns whether the code was valid.
    pub fn mark_dirty_subject(&mut self, subject: EntityHandle, dirty_code: u8) -> bool {
        DirtyKind::from_code(dirty_code)
            .map(|kind| self.world.mark_dirty_subject(subject, kind, None))
            .is_some()
    }

    /// Wake processes subscribed to the current dirty set, then clear it. Returns
    /// the number of processes woken.
    pub fn apply_dirty_invalidations(&mut self, tick: u64, cause: Option<CauseRef>) -> usize {
        self.world
            .apply_dirty_invalidations(SimTick::new(tick), cause)
    }

    /// The dirty fact ids, ascending.
    pub fn dirty_fact_ids(&self) -> Vec<FactId> {
        self.world
            .dirty()
            .dirty_facts()
            .map(|d: DirtyFact| d.fact())
            .collect()
    }

    /// The dirty relation ids, ascending.
    pub fn dirty_relation_ids(&self) -> Vec<RelationId> {
        self.world
            .dirty()
            .dirty_relations()
            .map(|d: DirtyRelation| d.relation())
            .collect()
    }

    /// The number of dirty subjects.
    pub fn dirty_subject_count(&self) -> usize {
        self.world.dirty().dirty_subjects().count()
    }

    /// The total number of dirty entries.
    pub fn dirty_len(&self) -> usize {
        self.world.dirty().len()
    }

    /// Whether anything is dirty.
    pub fn is_dirty(&self) -> bool {
        !self.world.dirty().is_empty()
    }
}

impl SimCoreApi {
    /// Step the scheduler at `tick`: run every due process's handler (no effects
    /// applied yet). Returns the execution order (process ids).
    pub fn step_scheduler(&mut self, tick: u64) -> Vec<ProcessId> {
        let result = self
            .world
            .step_scheduler(SchedulerStep::new(SimTick::new(tick)));
        result.order().processes().to_vec()
    }

    /// Apply the scheduler boundary at `tick`: apply every stashed process's
    /// effects, resolve lifecycles, and journal. Returns
    /// `(applied_batches, applied_effects, failed_effects)`.
    pub fn apply_scheduler_boundary(
        &mut self,
        tick: u64,
        registry: &EntityRegistry,
    ) -> (usize, usize, usize) {
        let boundary = self
            .world
            .apply_scheduler_boundary(SimTick::new(tick), registry);
        (
            boundary.applied_batches(),
            boundary.applied_effects(),
            boundary.failed_effects(),
        )
    }

    /// The processes due at or before `tick`, without consuming them.
    pub fn due_process_ids(&self, tick: u64) -> Vec<ProcessId> {
        self.world.scheduler().due_processes(SimTick::new(tick))
    }

    /// The status code of a process, if registered.
    pub fn process_status_code(&self, process: ProcessId) -> Option<u8> {
        self.world
            .scheduler()
            .status(process)
            .map(|status| status.code())
    }

    /// The pending wake tick of a process, if any.
    pub fn process_pending_wake(&self, process: ProcessId) -> Option<u64> {
        self.world
            .scheduler()
            .pending_wake(process)
            .map(SimTick::raw)
    }

    /// The number of registered scheduler processes.
    pub fn scheduler_process_count(&self) -> usize {
        self.world.scheduler().len()
    }

    /// The kind code of a scheduler process, if registered.
    pub fn scheduler_process_kind(&self, process: ProcessId) -> Option<u32> {
        self.world.scheduler().kind(process).map(|kind| kind.code())
    }

    /// The number of pending wakes in the scheduler's wake queue.
    pub fn pending_wake_count(&self) -> usize {
        self.world.scheduler().pending_wake_count()
    }

    /// The scheduler causal events for a process (it is the parent cause),
    /// ascending — the "query causal events by process" surface.
    pub fn scheduler_events_for_process(&self, process: ProcessId) -> Vec<CausalEventId> {
        self.events_by_parent(self.cause_process(process))
    }

    /// A process's subscriptions, as `(subscriber, dep_code, key)`, ascending.
    pub fn process_subscriptions(&self, process: ProcessId) -> Vec<(ProcessId, u8, u64)> {
        self.world
            .scheduler()
            .subscriptions_of(process)
            .into_iter()
            .map(|sub| {
                (
                    sub.process(),
                    sub.dependency().kind().code(),
                    sub.dependency().key(),
                )
            })
            .collect()
    }

    /// The number of distinct subscriptions across all processes.
    pub fn subscription_count(&self) -> usize {
        self.world.scheduler().subscription_count()
    }

    /// The scheduler's execution records, as
    /// `(process, tick, produced_effects, from_status, to_status, has_cause)`.
    pub fn execution_records(&self) -> Vec<(ProcessId, u64, usize, u8, u8, bool)> {
        self.world
            .scheduler()
            .execution_records()
            .iter()
            .map(|record: &ProcessExecutionRecord| {
                let transition = record.transition();
                (
                    record.process(),
                    record.tick().raw(),
                    record.produced_effects(),
                    transition.from().code(),
                    transition.to().code(),
                    transition.cause().is_some(),
                )
            })
            .collect()
    }

    /// The dirty facts in detail, as `(fact, fact_kind, dirty_code)`, ascending.
    pub fn dirty_fact_details(&self) -> Vec<(FactId, u32, u8)> {
        self.world
            .dirty()
            .dirty_facts()
            .map(|d: DirtyFact| (d.fact(), d.fact_kind(), d.reason().kind().code()))
            .collect()
    }

    /// The dirty relations in detail, as `(relation, relation_kind, dirty_code)`.
    pub fn dirty_relation_details(&self) -> Vec<(RelationId, u32, u8)> {
        self.world
            .dirty()
            .dirty_relations()
            .map(|d: DirtyRelation| (d.relation(), d.relation_kind(), d.reason().kind().code()))
            .collect()
    }

    /// The dirty subjects in detail, as `(subject, dirty_code, has_cause)`.
    pub fn dirty_subject_details(&self) -> Vec<(EntityHandle, u8, bool)> {
        self.world
            .dirty()
            .dirty_subjects()
            .map(|d: DirtySubject| {
                (
                    d.subject(),
                    d.reason().kind().code(),
                    d.reason().cause().is_some(),
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests;
