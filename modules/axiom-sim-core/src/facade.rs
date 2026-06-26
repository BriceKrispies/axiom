//! The single public facade of `axiom-sim-core`.

use axiom_ecs::{EntityHandle, EntityRegistry};

use crate::causal::CausalEvent;
use crate::cause::CauseRef;
use crate::definition::{DefinitionKind, PropertySet, TagSet};
use crate::effect::{EffectBatch, EffectReport};
use crate::fact::{Fact, FactKind, FactValue};
use crate::ids::{CausalEventId, DefinitionId, FactId, ProcessId, RelationId, RuleId};
use crate::process::{Process, WakeTick};
use crate::relation::{Relation, RelationEndpoint};
use crate::sim_world::SimWorld;

/// Forward map from a definition kind code to the rich kind (declaration order).
const DEFINITION_KINDS: [DefinitionKind; 11] = [
    DefinitionKind::Material,
    DefinitionKind::Substance,
    DefinitionKind::BodyPlan,
    DefinitionKind::Tissue,
    DefinitionKind::Behavior,
    DefinitionKind::Process,
    DefinitionKind::Effect,
    DefinitionKind::Job,
    DefinitionKind::Need,
    DefinitionKind::Thought,
    DefinitionKind::Generic,
];

/// The only public export of `axiom-sim-core`: a stateful handle to a generic
/// simulation world (facts, relations, definitions, processes, effects, causal
/// journal).
///
/// Every sim-core concept is constructed and accessed through this facade — the
/// internal types are never re-exported, so a consumer holds them only as opaque
/// values returned by these methods (e.g. a [`FactValue`] from
/// [`Self::value_unsigned`], passed straight back into [`Self::add_fact`]). The
/// world references ECS entity handles; methods that require a live entity take an
/// [`EntityRegistry`] and reject stale handles.
#[derive(Debug, Default)]
pub struct SimCoreApi {
    world: SimWorld,
}

impl SimCoreApi {
    /// Create an empty simulation world.
    pub fn new() -> Self {
        SimCoreApi {
            world: SimWorld::new(),
        }
    }

    // --- value constructors (opaque FactValue) ---

    /// A signed-integer fact value.
    pub fn value_signed(&self, value: i64) -> FactValue {
        FactValue::Signed(value)
    }
    /// An unsigned-integer fact value.
    pub fn value_unsigned(&self, value: u64) -> FactValue {
        FactValue::Unsigned(value)
    }
    /// A symbol-code fact value.
    pub fn value_symbol(&self, code: u64) -> FactValue {
        FactValue::Symbol(code)
    }
    /// A boolean fact value.
    pub fn value_bool(&self, value: bool) -> FactValue {
        FactValue::Bool(value)
    }
    /// An entity-reference fact value.
    pub fn value_entity(&self, entity: EntityHandle) -> FactValue {
        FactValue::Entity(entity)
    }

    // --- cause constructors (opaque CauseRef) ---

    /// A cause referencing a prior causal event.
    pub fn cause_event(&self, event: CausalEventId) -> CauseRef {
        CauseRef::Event(event)
    }
    /// A cause referencing a process.
    pub fn cause_process(&self, process: ProcessId) -> CauseRef {
        CauseRef::Process(process)
    }
    /// A cause referencing a rule.
    pub fn cause_rule(&self, rule: RuleId) -> CauseRef {
        CauseRef::Rule(rule)
    }
    /// A cause representing a direct external command.
    pub fn cause_command(&self) -> CauseRef {
        CauseRef::Command
    }

    // --- relation endpoint constructors (opaque RelationEndpoint) ---

    /// A relation endpoint referencing an ECS entity.
    pub fn endpoint_entity(&self, entity: EntityHandle) -> RelationEndpoint {
        RelationEndpoint::entity(entity)
    }
    /// A relation endpoint referencing an opaque symbol subject.
    pub fn endpoint_symbol(&self, code: u64) -> RelationEndpoint {
        RelationEndpoint::symbol(code)
    }
}

impl SimCoreApi {
    /// Definition-kind code: material.
    pub const KIND_MATERIAL: u8 = 0;
    /// Definition-kind code: substance.
    pub const KIND_SUBSTANCE: u8 = 1;
    /// Definition-kind code: body plan.
    pub const KIND_BODY_PLAN: u8 = 2;
    /// Definition-kind code: tissue.
    pub const KIND_TISSUE: u8 = 3;
    /// Definition-kind code: behavior.
    pub const KIND_BEHAVIOR: u8 = 4;
    /// Definition-kind code: process.
    pub const KIND_PROCESS: u8 = 5;
    /// Definition-kind code: effect.
    pub const KIND_EFFECT: u8 = 6;
    /// Definition-kind code: job.
    pub const KIND_JOB: u8 = 7;
    /// Definition-kind code: need.
    pub const KIND_NEED: u8 = 8;
    /// Definition-kind code: thought.
    pub const KIND_THOUGHT: u8 = 9;
    /// Definition-kind code: generic.
    pub const KIND_GENERIC: u8 = 10;

    /// Register a definition with a kind code (see the `KIND_*` constants), a
    /// durable name, string tags, and named properties. Returns its
    /// [`DefinitionId`], or `None` if the name is a duplicate or the kind code is
    /// out of range.
    pub fn register_definition(
        &mut self,
        kind_code: u8,
        name: &str,
        tags: &[&str],
        properties: &[(&str, FactValue)],
    ) -> Option<DefinitionId> {
        let tag_set = tags.iter().fold(TagSet::new(), |set, tag| set.with(tag));
        let property_set = properties
            .iter()
            .fold(PropertySet::new(), |set, (name, value)| {
                set.with(name, *value)
            });
        DEFINITION_KINDS
            .get(kind_code as usize)
            .copied()
            .and_then(|kind| {
                self.world
                    .definitions_mut()
                    .register(kind, name, tag_set, property_set)
            })
    }

    /// The id registered for a durable name, if any.
    pub fn definition_id(&self, name: &str) -> Option<DefinitionId> {
        self.world.definitions().id_of(name)
    }

    /// The kind code of a registered definition, if present.
    pub fn definition_kind_code(&self, id: DefinitionId) -> Option<u8> {
        self.world
            .definitions()
            .get(id)
            .map(|definition| definition.kind() as u8)
    }

    /// Whether a registered definition carries `tag`.
    pub fn definition_has_tag(&self, id: DefinitionId, tag: &str) -> bool {
        self.world
            .definitions()
            .get(id)
            .is_some_and(|definition| definition.has_tag(tag))
    }

    /// A named property of a registered definition, if present.
    pub fn definition_property(&self, id: DefinitionId, name: &str) -> Option<FactValue> {
        self.world
            .definitions()
            .get(id)
            .and_then(|definition| definition.property(name))
    }

    /// The number of registered definitions.
    pub fn definition_count(&self) -> usize {
        self.world.definitions().len()
    }
}

impl SimCoreApi {
    /// Add a fact about a live subject entity. Returns its [`FactId`], or `None`
    /// if the subject handle is stale/dead.
    pub fn add_fact(
        &mut self,
        registry: &EntityRegistry,
        kind_code: u32,
        subject: EntityHandle,
        value: FactValue,
        cause: Option<CauseRef>,
        tick: u64,
    ) -> Option<FactId> {
        registry.is_current(subject).then(|| {
            self.world
                .facts_mut()
                .insert(FactKind::new(kind_code), subject, value, cause, tick)
        })
    }

    /// The current value of a fact, if present.
    pub fn fact_value(&self, id: FactId) -> Option<FactValue> {
        self.world.facts().get(id).map(Fact::value)
    }

    /// Update a fact's value at a logical tick. Returns whether it existed.
    pub fn update_fact(&mut self, id: FactId, value: FactValue, tick: u64) -> bool {
        self.world.facts_mut().update(id, value, tick)
    }

    /// Remove a fact. Returns whether it existed.
    pub fn remove_fact(&mut self, id: FactId) -> bool {
        self.world.facts_mut().remove(id).is_some()
    }

    /// The ids of facts of a given kind, in ascending order.
    pub fn facts_by_kind(&self, kind_code: u32) -> Vec<FactId> {
        self.world
            .facts()
            .by_kind(FactKind::new(kind_code))
            .map(Fact::id)
            .collect()
    }

    /// The ids of facts about a given subject, in ascending order.
    pub fn facts_by_subject(&self, subject: EntityHandle) -> Vec<FactId> {
        self.world
            .facts()
            .by_subject(subject)
            .map(Fact::id)
            .collect()
    }

    /// The number of facts.
    pub fn fact_count(&self) -> usize {
        self.world.facts().len()
    }
}

impl SimCoreApi {
    /// Add a relation over ordered endpoints. Returns its [`RelationId`], or
    /// `None` if any entity endpoint is stale/dead.
    pub fn add_relation(
        &mut self,
        registry: &EntityRegistry,
        kind_code: u32,
        endpoints: Vec<RelationEndpoint>,
        strength: Option<i64>,
        cause: Option<CauseRef>,
    ) -> Option<RelationId> {
        let live = endpoints.iter().all(|endpoint| {
            endpoint
                .as_entity()
                .is_none_or(|handle| registry.is_current(handle))
        });
        live.then(|| {
            self.world.relations_mut().insert(
                crate::relation::RelationKind::new(kind_code),
                endpoints,
                strength,
                cause,
            )
        })
    }

    /// Remove a relation. Returns whether it existed.
    pub fn remove_relation(&mut self, id: RelationId) -> bool {
        self.world.relations_mut().remove(id).is_some()
    }

    /// The ids of relations of a given kind, in ascending order.
    pub fn relations_by_kind(&self, kind_code: u32) -> Vec<RelationId> {
        self.world
            .relations()
            .by_kind(crate::relation::RelationKind::new(kind_code))
            .map(Relation::id)
            .collect()
    }

    /// The ids of relations touching a given endpoint, in ascending order.
    pub fn relations_by_endpoint(&self, endpoint: RelationEndpoint) -> Vec<RelationId> {
        self.world
            .relations()
            .by_endpoint(endpoint)
            .map(Relation::id)
            .collect()
    }

    /// The number of relations.
    pub fn relation_count(&self) -> usize {
        self.world.relations().len()
    }
}

impl SimCoreApi {
    /// Schedule a process for a live subject at a logical wake tick. Returns its
    /// [`ProcessId`], or `None` if the subject handle is stale/dead.
    pub fn schedule_process(
        &mut self,
        registry: &EntityRegistry,
        kind_code: u32,
        subject: EntityHandle,
        state_code: u32,
        wake: u64,
        cause: Option<CauseRef>,
    ) -> Option<ProcessId> {
        registry.is_current(subject).then(|| {
            self.world.processes_mut().schedule(
                crate::process::ProcessKind::new(kind_code),
                subject,
                crate::process::ProcessState::new(state_code),
                WakeTick::new(wake),
                cause,
            )
        })
    }

    /// Cancel a process. Returns whether it existed.
    pub fn cancel_process(&mut self, id: ProcessId) -> bool {
        self.world.processes_mut().cancel(id)
    }

    /// Move a process's next wake to `wake`. Returns whether it existed.
    pub fn reschedule_process(&mut self, id: ProcessId, wake: u64) -> bool {
        self.world
            .processes_mut()
            .reschedule(id, WakeTick::new(wake))
    }

    /// Wake every process due at or before `tick`, in deterministic order.
    pub fn wake_due(&mut self, tick: u64) -> Vec<ProcessId> {
        self.world.processes_mut().wake_due(tick)
    }

    /// The next wake tick of a process, if present.
    pub fn process_wake(&self, id: ProcessId) -> Option<u64> {
        self.world
            .processes()
            .get(id)
            .map(|process| process.wake().raw())
    }

    /// The number of live processes.
    pub fn process_count(&self) -> usize {
        self.world.processes().len()
    }
}

impl SimCoreApi {
    /// Create an empty effect batch to stage proposed mutations.
    pub fn new_effect_batch(&self) -> EffectBatch {
        EffectBatch::new()
    }

    /// Apply a staged batch at this explicit boundary, returning per-effect
    /// outcomes. Liveness for entity-referencing effects is checked against
    /// `registry`.
    pub fn apply_effects(&mut self, batch: EffectBatch, registry: &EntityRegistry) -> EffectReport {
        self.world.apply_effects(batch, registry)
    }

    /// Append a causal event directly (outside an effect batch). Returns its id.
    /// `parties` is `(subject, secondary)` — the primary and secondary entities.
    pub fn append_causal_event(
        &mut self,
        kind_code: u32,
        tick: u64,
        parties: (Option<EntityHandle>, Option<EntityHandle>),
        parent: Option<CauseRef>,
        code: u64,
        payload: Option<FactValue>,
    ) -> CausalEventId {
        self.world.journal_mut().append(
            crate::causal::CausalEventKind::new(kind_code),
            tick,
            parties,
            parent,
            code,
            payload,
        )
    }

    /// The ids of causal events whose primary subject is `subject`.
    pub fn events_by_subject(&self, subject: EntityHandle) -> Vec<CausalEventId> {
        self.world
            .journal()
            .by_subject(subject)
            .map(CausalEvent::id)
            .collect()
    }

    /// The ids of causal events whose parent cause is `cause`.
    pub fn events_by_parent(&self, cause: CauseRef) -> Vec<CausalEventId> {
        self.world
            .journal()
            .by_parent(cause)
            .map(CausalEvent::id)
            .collect()
    }

    /// The parent cause of a causal event, if present.
    pub fn event_parent(&self, id: CausalEventId) -> Option<CauseRef> {
        self.world.journal().get(id).and_then(CausalEvent::parent)
    }

    /// The number of recorded causal events.
    pub fn causal_event_count(&self) -> usize {
        self.world.journal().len()
    }
}

impl SimCoreApi {
    /// Whether every store is empty.
    pub fn is_empty(&self) -> bool {
        self.world.is_empty()
    }

    /// Borrow a fact by id, if present (its kind/subject/value/cause/tick are
    /// readable through the returned reference).
    pub fn fact(&self, id: FactId) -> Option<&Fact> {
        self.world.facts().get(id)
    }

    /// Borrow a relation by id, if present.
    pub fn relation(&self, id: RelationId) -> Option<&Relation> {
        self.world.relations().get(id)
    }

    /// Borrow a process by id, if present.
    pub fn process(&self, id: ProcessId) -> Option<&Process> {
        self.world.processes().get(id)
    }

    /// Borrow a causal event by id, if present.
    pub fn causal_event(&self, id: CausalEventId) -> Option<&CausalEvent> {
        self.world.journal().get(id)
    }

    /// Borrow a definition by id, if present.
    pub fn definition(&self, id: DefinitionId) -> Option<&crate::definition::Definition> {
        self.world.definitions().get(id)
    }

    /// Borrow a definition by durable name, if present.
    pub fn definition_by_name(&self, name: &str) -> Option<&crate::definition::Definition> {
        self.world.definitions().by_name(name)
    }
}

impl SimCoreApi {
    /// All fact ids, in deterministic ascending order.
    pub fn all_fact_ids(&self) -> Vec<FactId> {
        self.world.facts().iter().map(Fact::id).collect()
    }

    /// All relation ids, in deterministic ascending order.
    pub fn all_relation_ids(&self) -> Vec<RelationId> {
        self.world.relations().iter().map(Relation::id).collect()
    }

    /// All process ids, in deterministic ascending order.
    pub fn all_process_ids(&self) -> Vec<ProcessId> {
        self.world.processes().iter().map(Process::id).collect()
    }

    /// All causal-event ids, in deterministic ascending order.
    pub fn all_causal_event_ids(&self) -> Vec<CausalEventId> {
        self.world.journal().iter().map(CausalEvent::id).collect()
    }

    /// All definition ids, in deterministic ascending order.
    pub fn all_definition_ids(&self) -> Vec<DefinitionId> {
        self.world
            .definitions()
            .iter()
            .map(crate::definition::Definition::id)
            .collect()
    }

    /// The number of processes currently scheduled to wake.
    pub fn scheduled_process_count(&self) -> usize {
        self.world.processes().scheduled()
    }
}

/// The materials/substances facade methods on `SimCoreApi` (quantities, residue
/// locations, material/substance registration, residues, interactions, transfer
/// rules, material-effect rules, and catalog accessors). Kept in a child module
/// so this file stays within the file-size budget; child modules see the private
/// `world` field.
mod materials;

/// The body/anatomy facade methods on `SimCoreApi` (tissues, body plans, bodies,
/// surfaces, body routes, wounds). Kept in a child module so this file stays
/// within the file-size budget; child modules see the private `world` field.
mod anatomy;

/// The process-scheduler facade methods on `SimCoreApi` (tick model, process
/// registration/lifecycle, wake queue, dirty set, dependencies, scheduler step
/// and boundary). Kept in a child module for the same file-size reason.
mod scheduling;

#[cfg(test)]
mod tests;
