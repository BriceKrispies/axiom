//! The internal simulation state: the five stores + deterministic effect application.

use axiom_ecs::EntityRegistry;

use crate::body::BodyStore;
use crate::body_plan::BodyPlanRegistry;
use crate::causal::{CausalEventKind, CausalJournal};
use crate::cause::CauseRef;
use crate::definition::DefinitionRegistry;
use crate::effect::{EffectBatch, EffectReport, EffectResult};
use crate::fact::{FactStore, FactValue};
use crate::ids::{BodyId, BodyPlanId, WoundId};
use crate::ids::{DefinitionId, FactId, ProcessId, RelationId, ResidueId};
use crate::interaction::{InteractionRecord, InteractionStore};
use crate::material::MaterialCatalog;
use crate::material_effect::{MaterialEffectResult, MaterialEffectRuleStore};
use crate::process::{ProcessKind, ProcessQueue};
use crate::quantity::{Quantity, QuantityUnit};
use crate::relation::RelationStore;
use crate::residue::{ResidueLocation, ResidueState, ResidueStore};
use crate::tissue::TissueRegistry;
use crate::transfer::{TransferOutcome, TransferResult, TransferRule};
use crate::wound::{WoundSpec, WoundStore};
use axiom_ecs::EntityHandle;

use crate::dirty_set::{DirtyKind, DirtySet};
use crate::process_dependency::{DependencyKind, ProcessDependency};
use crate::process_handler::{HandlerSpec, ProcessContext, ProcessHandler};
use crate::process_lifecycle::{ProcessExecutionRecord, ProcessStatus};
use crate::scheduler::{
    ProcessExecutionOrder, ProcessScheduler, SchedulerBoundary, SchedulerStep, SchedulerStepResult,
};
use crate::sim_tick::SimTick;
use crate::wake_reason::WakeReason;

/// The owned simulation state: a [`FactStore`], [`RelationStore`],
/// [`DefinitionRegistry`], [`ProcessQueue`], and [`CausalJournal`].
/// `SimWorld` references ECS entity handles inside its facts/relations/processes,
/// but it does **not** own the ECS — liveness is checked against an
/// [`EntityRegistry`] passed in at the mutation boundary. All mutation that comes
/// from processes/rules/commands flows through [`Self::apply_effects`], never by
/// touching the stores directly.
#[derive(Debug, Clone, Default)]
pub(crate) struct SimWorld {
    facts: FactStore,
    relations: RelationStore,
    definitions: DefinitionRegistry,
    processes: ProcessQueue,
    journal: CausalJournal,
    catalog: MaterialCatalog,
    residues: ResidueStore,
    interactions: InteractionStore,
    transfers: crate::transfer::TransferRuleStore,
    effect_rules: MaterialEffectRuleStore,
    body_plans: BodyPlanRegistry,
    tissues: TissueRegistry,
    bodies: BodyStore,
    wounds: WoundStore,
    dirty: DirtySet,
    scheduler: ProcessScheduler,
}

impl SimWorld {
    /// Create an empty sim world.
    pub(crate) fn new() -> Self {
        SimWorld {
            facts: FactStore::new(),
            relations: RelationStore::new(),
            definitions: DefinitionRegistry::new(),
            processes: ProcessQueue::new(),
            journal: CausalJournal::new(),
            catalog: MaterialCatalog::new(),
            residues: ResidueStore::new(),
            interactions: InteractionStore::new(),
            transfers: crate::transfer::TransferRuleStore::new(),
            effect_rules: MaterialEffectRuleStore::new(),
            body_plans: BodyPlanRegistry::new(),
            tissues: TissueRegistry::new(),
            bodies: BodyStore::new(),
            wounds: WoundStore::new(),
            dirty: DirtySet::new(),
            scheduler: ProcessScheduler::new(),
        }
    }

    /// Whether every store is empty.
    pub(crate) fn is_empty(&self) -> bool {
        self.facts.is_empty()
            & self.relations.is_empty()
            & self.definitions.is_empty()
            & self.processes.is_empty()
            & self.journal.is_empty()
            & self.catalog.is_empty()
            & self.residues.is_empty()
            & self.interactions.is_empty()
            & self.transfers.is_empty()
            & self.effect_rules.is_empty()
            & self.body_plans.is_empty()
            & self.tissues.is_empty()
            & self.bodies.is_empty()
            & self.wounds.is_empty()
            & self.dirty.is_empty()
            & self.scheduler.is_empty()
    }

    pub(crate) fn facts(&self) -> &FactStore {
        &self.facts
    }
    pub(crate) fn facts_mut(&mut self) -> &mut FactStore {
        &mut self.facts
    }
    pub(crate) fn relations(&self) -> &RelationStore {
        &self.relations
    }
    pub(crate) fn relations_mut(&mut self) -> &mut RelationStore {
        &mut self.relations
    }
    pub(crate) fn definitions(&self) -> &DefinitionRegistry {
        &self.definitions
    }
    pub(crate) fn definitions_mut(&mut self) -> &mut DefinitionRegistry {
        &mut self.definitions
    }
    pub(crate) fn processes(&self) -> &ProcessQueue {
        &self.processes
    }
    pub(crate) fn processes_mut(&mut self) -> &mut ProcessQueue {
        &mut self.processes
    }
    pub(crate) fn journal(&self) -> &CausalJournal {
        &self.journal
    }
    pub(crate) fn journal_mut(&mut self) -> &mut CausalJournal {
        &mut self.journal
    }
    pub(crate) fn catalog(&self) -> &MaterialCatalog {
        &self.catalog
    }
    pub(crate) fn catalog_mut(&mut self) -> &mut MaterialCatalog {
        &mut self.catalog
    }
    pub(crate) fn residues(&self) -> &ResidueStore {
        &self.residues
    }
    pub(crate) fn residues_mut(&mut self) -> &mut ResidueStore {
        &mut self.residues
    }
    pub(crate) fn interactions(&self) -> &InteractionStore {
        &self.interactions
    }
    pub(crate) fn interactions_mut(&mut self) -> &mut InteractionStore {
        &mut self.interactions
    }
    pub(crate) fn transfers(&self) -> &crate::transfer::TransferRuleStore {
        &self.transfers
    }
    pub(crate) fn transfers_mut(&mut self) -> &mut crate::transfer::TransferRuleStore {
        &mut self.transfers
    }
    pub(crate) fn effect_rules(&self) -> &MaterialEffectRuleStore {
        &self.effect_rules
    }
    pub(crate) fn effect_rules_mut(&mut self) -> &mut MaterialEffectRuleStore {
        &mut self.effect_rules
    }
    pub(crate) fn body_plans(&self) -> &BodyPlanRegistry {
        &self.body_plans
    }
    pub(crate) fn body_plans_mut(&mut self) -> &mut BodyPlanRegistry {
        &mut self.body_plans
    }
    pub(crate) fn tissues(&self) -> &TissueRegistry {
        &self.tissues
    }
    pub(crate) fn tissues_mut(&mut self) -> &mut TissueRegistry {
        &mut self.tissues
    }
    pub(crate) fn bodies(&self) -> &BodyStore {
        &self.bodies
    }
    pub(crate) fn bodies_mut(&mut self) -> &mut BodyStore {
        &mut self.bodies
    }
    pub(crate) fn wounds(&self) -> &WoundStore {
        &self.wounds
    }
    pub(crate) fn wounds_mut(&mut self) -> &mut WoundStore {
        &mut self.wounds
    }
}

impl SimWorld {
    /// Instantiate a body from a registered plan for an optional owner entity.
    /// Returns `None` if the plan id is unknown or the owner handle is stale.
    pub(crate) fn instantiate_body(
        &mut self,
        plan: BodyPlanId,
        owner: Option<EntityHandle>,
        registry: &EntityRegistry,
        cause: Option<CauseRef>,
        tick: u64,
    ) -> Option<BodyId> {
        let owner_ok = owner.is_none_or(|handle| registry.is_current(handle));
        owner_ok
            .then(|| self.body_plans.get(plan).cloned())
            .flatten()
            .map(|plan| self.bodies.instantiate(&plan, owner, cause, tick))
    }

    /// Create a wound, validating that the body, the part (belonging to that
    /// body), and the tissue (if given) exist. On success a causal event is
    /// appended (subject = the body's owner entity, if any). Returns `None` for
    /// invalid references.
    pub(crate) fn create_wound(
        &mut self,
        spec: WoundSpec,
        event_kind: u32,
        event_code: u64,
    ) -> Option<WoundId> {
        let body = spec.body;
        let part = spec.part;
        let tissue = spec.tissue;
        let cause = spec.cause;
        let tick = spec.tick;
        let body_ok = self.bodies.get(body).is_some();
        let part_ok = self
            .bodies
            .part(part)
            .map(|p| p.body() == body)
            .unwrap_or(false);
        let tissue_ok = tissue.is_none_or(|t| self.tissues.get(t).is_some());
        let subject = self.bodies.get(body).and_then(crate::body::Body::owner);
        (body_ok & part_ok & tissue_ok).then(|| {
            let id = self.wounds.create(spec);
            self.journal.append(
                CausalEventKind::new(event_kind),
                tick,
                (subject, None),
                cause,
                event_code,
                None,
            );
            id
        })
    }

    /// Apply a transfer rule that consumes `interaction`, moving quantity from
    /// the interaction's source residue to `target_location`. Conserves quantity
    /// (deposits the moved amount) unless the rule is lossy. Emits a causal event
    /// on success. All checks are side-effect-free; mutation happens only when the
    /// outcome is [`TransferOutcome::Applied`].
    pub(crate) fn apply_transfer(
        &mut self,
        rule: TransferRule,
        interaction: &InteractionRecord,
        target_location: ResidueLocation,
        event_kind: u32,
        event_code: u64,
        tick: u64,
    ) -> TransferResult {
        let route_ok = rule.route() == interaction.route();
        let source = interaction.residue().and_then(|sid| {
            self.residues
                .get(sid)
                .map(|residue| (sid, residue.quantity(), residue.definition()))
        });
        let (sid, src_q, def) = source.unwrap_or((
            ResidueId::from_raw(0),
            Quantity::zero(QuantityUnit::Count),
            DefinitionId::from_raw(0),
        ));
        let desired = rule.mode().compute(src_q.amount());
        let moved_q = Quantity::new(src_q.unit(), desired);
        let new_source = moved_q.and_then(|m| src_q.sub(m));
        let existing = self
            .residues
            .by_location(target_location)
            .filter(|residue| residue.definition() == def)
            .map(|residue| (residue.id(), residue.quantity()))
            .next();
        let target_sum = existing.and_then(|(_, tq)| moved_q.and_then(|m| tq.add(m)));
        let target_id = existing.map(|(tid, _)| tid);
        let units_ok = rule.lossy() | existing.is_none_or(|_| target_sum.is_some());

        let outcome = (!route_ok)
            .then_some(TransferOutcome::RouteMismatch)
            .or(source.is_none().then_some(TransferOutcome::InvalidSource))
            .or(new_source
                .is_none()
                .then_some(TransferOutcome::InsufficientQuantity))
            .or((!units_ok).then_some(TransferOutcome::IncompatibleUnits))
            .unwrap_or(TransferOutcome::Applied);
        let applied = outcome == TransferOutcome::Applied;

        applied.then(|| new_source.map(|ns| self.residues.set_quantity(sid, ns, tick)));
        let deposit = applied & !rule.lossy();
        let updated_opt = deposit.then(|| {
            target_id
                .zip(target_sum)
                .map(|(tid, sum)| self.residues.set_quantity(tid, sum, tick))
                .is_some()
        });
        let updated = updated_opt.unwrap_or(false);
        (deposit & !updated).then(|| {
            moved_q.map(|m| {
                self.residues.create(
                    def,
                    m,
                    target_location,
                    ResidueState::new(0),
                    interaction.cause(),
                    tick,
                )
            })
        });
        applied.then(|| {
            self.journal.append(
                CausalEventKind::new(event_kind),
                tick,
                (Some(interaction.primary()), interaction.secondary()),
                interaction.cause(),
                event_code,
                Some(FactValue::Signed(desired)),
            )
        });
        TransferResult::new(outcome, applied.then_some(moved_q).flatten())
    }

    /// Produce the effects of every material effect rule matching `interaction`
    /// into a fresh batch (does not apply them).
    pub(crate) fn produce_material_effects(
        &self,
        interaction: &InteractionRecord,
        context_fact: Option<FactId>,
        cause: Option<CauseRef>,
    ) -> EffectBatch {
        let mut batch = EffectBatch::new();
        self.effect_rules.produce_into(
            &mut batch,
            interaction,
            context_fact,
            cause,
            &self.definitions,
        );
        batch
    }

    /// Produce and apply material effects for `interaction` at this boundary,
    /// returning how many rules matched and how many effects applied.
    pub(crate) fn apply_material_effects(
        &mut self,
        interaction: InteractionRecord,
        context_fact: Option<FactId>,
        cause: Option<CauseRef>,
        registry: &EntityRegistry,
    ) -> MaterialEffectResult {
        let mut batch = EffectBatch::new();
        let matched = self.effect_rules.produce_into(
            &mut batch,
            &interaction,
            context_fact,
            cause,
            &self.definitions,
        );
        let report = self.apply_effects(batch, registry);
        MaterialEffectResult::new(matched, report.len())
    }

    pub(crate) fn dirty(&self) -> &DirtySet {
        &self.dirty
    }
    pub(crate) fn scheduler(&self) -> &ProcessScheduler {
        &self.scheduler
    }

    /// Append a process-lifecycle causal event (parented to the process, so it is
    /// queryable by process; subject = the process's subject entity).
    fn journal_sched_event(
        &mut self,
        code: u32,
        process: ProcessId,
        subject: EntityHandle,
        tick: SimTick,
    ) {
        self.journal.append(
            CausalEventKind::new(code),
            tick.raw(),
            (Some(subject), None),
            Some(CauseRef::Process(process)),
            process.raw(),
            Some(FactValue::Unsigned(process.raw())),
        );
    }

    /// Append a dirty-invalidation causal event (parented to the supplied cause).
    fn journal_invalidation(
        &mut self,
        code: u32,
        cause: Option<CauseRef>,
        tick: SimTick,
        payload: u64,
    ) {
        self.journal.append(
            CausalEventKind::new(code),
            tick.raw(),
            (None, None),
            cause,
            payload,
            Some(FactValue::Unsigned(payload)),
        );
    }

    /// Append a `woke` event carrying the wake reason as its payload.
    fn journal_woke(
        &mut self,
        process: ProcessId,
        subject: EntityHandle,
        tick: SimTick,
        reason: WakeReason,
    ) {
        self.journal.append(
            CausalEventKind::new(SCHED_PROCESS_WOKE),
            tick.raw(),
            (Some(subject), None),
            Some(CauseRef::Process(process)),
            process.raw(),
            Some(FactValue::Unsigned(u64::from(reason.code()))),
        );
    }

    /// Register a scheduler process (status `Scheduled`); journals `scheduled`.
    pub(crate) fn register_scheduler_process(
        &mut self,
        kind: u32,
        subject: EntityHandle,
        spec: HandlerSpec,
        tick: SimTick,
    ) -> ProcessId {
        let id = self
            .scheduler
            .register(ProcessKind::new(kind), subject, spec);
        self.journal_sched_event(SCHED_PROCESS_SCHEDULED, id, subject, tick);
        id
    }

    /// Schedule a process's wake at `tick`.
    pub(crate) fn schedule_scheduler_wake(
        &mut self,
        process: ProcessId,
        tick: SimTick,
        reason: WakeReason,
    ) -> bool {
        self.scheduler.schedule_wake(process, tick, reason)
    }

    /// Cancel a scheduler process; journals `canceled` on success.
    pub(crate) fn cancel_scheduler_process(&mut self, process: ProcessId, tick: SimTick) -> bool {
        let subject = self.scheduler.subject(process);
        let canceled = self.scheduler.cancel(process, tick);
        canceled
            .then_some(subject)
            .flatten()
            .into_iter()
            .for_each(|s| self.journal_sched_event(SCHED_PROCESS_CANCELED, process, s, tick));
        canceled
    }

    /// Subscribe a process to a dependency.
    pub(crate) fn subscribe_dependency(
        &mut self,
        process: ProcessId,
        kind: DependencyKind,
        key: u64,
    ) -> bool {
        self.scheduler
            .subscribe(process, ProcessDependency::new(kind, key))
    }

    /// Manually mark a fact / relation / subject dirty.
    pub(crate) fn mark_dirty_fact(
        &mut self,
        fact: FactId,
        fact_kind: u32,
        kind: DirtyKind,
        cause: Option<CauseRef>,
    ) {
        self.dirty.mark_fact(fact, fact_kind, kind, cause);
    }
    pub(crate) fn mark_dirty_relation(
        &mut self,
        relation: RelationId,
        relation_kind: u32,
        kind: DirtyKind,
        cause: Option<CauseRef>,
    ) {
        self.dirty
            .mark_relation(relation, relation_kind, kind, cause);
    }
    pub(crate) fn mark_dirty_subject(
        &mut self,
        subject: EntityHandle,
        kind: DirtyKind,
        cause: Option<CauseRef>,
    ) {
        self.dirty.mark_subject(subject, kind, cause);
    }

    /// Wake every process subscribed to a dirty change, then clear the dirty set.
    /// Returns how many processes were woken. Journals one invalidation event per
    /// dirty fact/relation and one `woken-by-dirty` event per woken process.
    pub(crate) fn apply_dirty_invalidations(
        &mut self,
        tick: SimTick,
        cause: Option<CauseRef>,
    ) -> usize {
        let facts: Vec<(FactId, u32)> = self
            .dirty
            .dirty_facts()
            .map(|d| (d.fact(), d.fact_kind()))
            .collect();
        let relations: Vec<(RelationId, u32)> = self
            .dirty
            .dirty_relations()
            .map(|d| (d.relation(), d.relation_kind()))
            .collect();
        let subjects: Vec<EntityHandle> =
            self.dirty.dirty_subjects().map(|d| d.subject()).collect();
        let mut woken = 0usize;
        facts.into_iter().for_each(|(fact, kind)| {
            self.journal_invalidation(SCHED_DIRTY_INVALIDATION, cause, tick, fact.raw());
            woken += self.wake_subscribers(DependencyKind::FactKindChanged, u64::from(kind), tick);
        });
        relations.into_iter().for_each(|(relation, kind)| {
            self.journal_invalidation(SCHED_DIRTY_INVALIDATION, cause, tick, relation.raw());
            woken +=
                self.wake_subscribers(DependencyKind::RelationKindChanged, u64::from(kind), tick);
        });
        subjects.into_iter().for_each(|subject| {
            woken +=
                self.wake_subscribers(DependencyKind::SubjectChanged, subject.id().raw(), tick);
        });
        self.dirty.clear();
        woken
    }

    /// Wake every subscriber of `(kind, key)` at `tick`, journaling each. Returns
    /// the number woken.
    fn wake_subscribers(&mut self, kind: DependencyKind, key: u64, tick: SimTick) -> usize {
        let subscribers = self.scheduler.subscribers_of(kind, key);
        subscribers
            .into_iter()
            .map(|process| {
                self.scheduler
                    .schedule_wake(process, tick, WakeReason::DirtyDependency);
                self.scheduler
                    .subject(process)
                    .into_iter()
                    .for_each(|subject| {
                        self.journal_sched_event(SCHED_WOKEN_BY_DIRTY, process, subject, tick);
                    });
            })
            .count()
    }

    /// Step the scheduler at `tick`: advance every due process to `Running`, run
    /// its handler, and stash the produced output for the boundary. Journals
    /// `woke` + `started` per process. Effects are NOT applied here.
    pub(crate) fn step_scheduler(&mut self, step: SchedulerStep) -> SchedulerStepResult {
        let tick = step.tick();
        let due = self.scheduler.take_due(tick);
        let order: Vec<ProcessId> = due.iter().map(|entry| entry.process()).collect();
        due.into_iter().for_each(|entry| {
            let context = ProcessContext::new(entry.subject(), tick);
            let output = entry.spec().run(&context);
            self.journal_woke(entry.process(), entry.subject(), tick, entry.reason());
            self.journal_sched_event(
                SCHED_PROCESS_STARTED,
                entry.process(),
                entry.subject(),
                tick,
            );
            self.scheduler.stash(entry.process(), output);
        });
        SchedulerStepResult::new(ProcessExecutionOrder::from_vec(order))
    }

    /// Apply every stashed process output at this explicit boundary: apply its
    /// effects (marking dirty), resolve its lifecycle (failed effects force a
    /// `Failed` status), reschedule on a reschedule disposition, and journal
    /// `produced` + `applied` + the transition. Returns a [`SchedulerBoundary`].
    pub(crate) fn apply_scheduler_boundary(
        &mut self,
        tick: SimTick,
        registry: &EntityRegistry,
    ) -> SchedulerBoundary {
        let pending = self.scheduler.take_pending();
        let mut batches = 0usize;
        let mut effects = 0usize;
        let mut failed = 0usize;
        pending.into_iter().for_each(|(process, output)| {
            let subject = self.scheduler.subject(process);
            let disposition = output.disposition();
            let report = self.apply_effects(output.into_effects(), registry);
            let report_failed = report.count(EffectResult::Failed);
            let any_failed = report_failed > 0;
            batches += 1;
            effects += report.len();
            failed += report_failed;
            let target = [disposition.target_status(), ProcessStatus::Failed][any_failed as usize];
            let reschedule = (!any_failed).then(|| disposition.as_reschedule()).flatten();
            let transition = self.scheduler.finalize(process, target, reschedule, tick);
            let produced = report.len();
            transition.into_iter().for_each(|t| {
                self.scheduler
                    .record_execution(ProcessExecutionRecord::new(process, produced, t));
            });
            subject.into_iter().for_each(|s| {
                self.journal_sched_event(SCHED_PRODUCED_EFFECTS, process, s, tick);
                self.journal_sched_event(SCHED_EFFECTS_APPLIED, process, s, tick);
                self.journal_sched_event(TRANSITION_CODE[target.code() as usize], process, s, tick);
            });
        });
        SchedulerBoundary::new(batches, effects, failed)
    }

    /// Apply a batch of effects in FIFO order at this explicit boundary,
    /// returning the per-effect outcomes. Each effect is dispatched by its tag
    /// through [`effect_apply::APPLY`]; stale-entity effects are `Skipped`, invalid-id effects
    /// `Failed`, never a panic.
    pub(crate) fn apply_effects(
        &mut self,
        batch: EffectBatch,
        registry: &EntityRegistry,
    ) -> EffectReport {
        let results = batch
            .into_effects()
            .into_iter()
            .map(|effect| {
                let tag = effect.tag();
                effect_apply::APPLY[tag as usize](self, effect, registry)
            })
            .collect();
        EffectReport::from_results(results)
    }
}

// Causal-event kind codes the scheduler emits (sim-core-internal; opaque to
// domains). Exposed through the facade as `SimCoreApi::SCHED_EVENT_*`.
pub(crate) const SCHED_PROCESS_SCHEDULED: u32 = 0x5005_0000;
pub(crate) const SCHED_PROCESS_WOKE: u32 = 0x5005_0001;
pub(crate) const SCHED_PROCESS_STARTED: u32 = 0x5005_0002;
pub(crate) const SCHED_PROCESS_COMPLETED: u32 = 0x5005_0003;
pub(crate) const SCHED_PROCESS_SLEPT: u32 = 0x5005_0004;
pub(crate) const SCHED_PROCESS_CANCELED: u32 = 0x5005_0005;
pub(crate) const SCHED_PROCESS_FAILED: u32 = 0x5005_0006;
pub(crate) const SCHED_DIRTY_INVALIDATION: u32 = 0x5005_0007;
pub(crate) const SCHED_WOKEN_BY_DIRTY: u32 = 0x5005_0008;
pub(crate) const SCHED_PRODUCED_EFFECTS: u32 = 0x5005_0009;
pub(crate) const SCHED_EFFECTS_APPLIED: u32 = 0x5005_000A;

// Map a resolved process status to the transition causal-event code.
const TRANSITION_CODE: [u32; 7] = [
    SCHED_PROCESS_SCHEDULED, // Scheduled
    SCHED_PROCESS_SLEPT,     // Sleeping
    SCHED_PROCESS_WOKE,      // Ready
    SCHED_PROCESS_STARTED,   // Running
    SCHED_PROCESS_COMPLETED, // Completed
    SCHED_PROCESS_CANCELED,  // Canceled
    SCHED_PROCESS_FAILED,    // Failed
];

mod effect_apply;

#[cfg(test)]
mod tests;
