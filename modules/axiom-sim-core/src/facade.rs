//! The single public facade of `axiom-sim-core`.

use axiom_ecs::{EntityHandle, EntityRegistry};

use crate::causal::CausalEvent;
use crate::cause::CauseRef;
use crate::definition::{Definition, DefinitionKind, PropertySet, TagSet};
use crate::effect::{EffectBatch, EffectReport};
use crate::fact::{Fact, FactKind, FactValue};
use crate::ids::{
    CausalEventId, DefinitionId, FactId, InteractionId, MaterialEffectRuleId, ProcessId,
    RelationId, ResidueId, RuleId, TransferRuleId,
};
use crate::interaction::{InteractionKind, InteractionParams, InteractionRecord, InteractionRoute};
use crate::material::{MaterialKind, MaterialProperty, SubstanceKind, SubstanceProperty};
use crate::material_effect::{
    MaterialEffectKind, MaterialEffectResult, MaterialEffectRule, MaterialEffectRuleParams,
};
use crate::process::{Process, WakeTick};
use crate::quantity::{Quantity, QuantityUnit};
use crate::relation::{Relation, RelationEndpoint};
use crate::residue::{Residue, ResidueLocation, ResidueState};
use crate::sim_world::SimWorld;
use crate::transfer::{TransferMode, TransferResult, TransferRule};

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

/// Forward map from a quantity-unit code to the unit (declaration order).
const QUANTITY_UNITS: [QuantityUnit; 5] = [
    QuantityUnit::Count,
    QuantityUnit::Mass,
    QuantityUnit::Volume,
    QuantityUnit::Dose,
    QuantityUnit::Arbitrary,
];

/// Forward map from a material-effect-kind code to the kind (declaration order).
const MATERIAL_EFFECT_KINDS: [MaterialEffectKind; 6] = [
    MaterialEffectKind::AddFact,
    MaterialEffectKind::UpdateFact,
    MaterialEffectKind::RemoveFact,
    MaterialEffectKind::AddRelation,
    MaterialEffectKind::EmitCausalEvent,
    MaterialEffectKind::ScheduleProcess,
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
                .map_or(true, |handle| registry.is_current(handle))
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
    pub fn append_causal_event(
        &mut self,
        kind_code: u32,
        tick: u64,
        subject: Option<EntityHandle>,
        secondary: Option<EntityHandle>,
        parent: Option<CauseRef>,
        code: u64,
        payload: Option<FactValue>,
    ) -> CausalEventId {
        self.world.journal_mut().append(
            crate::causal::CausalEventKind::new(kind_code),
            tick,
            subject,
            secondary,
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

impl SimCoreApi {
    /// Quantity-unit code: a discrete count.
    pub const UNIT_COUNT: u8 = 0;
    /// Quantity-unit code: mass.
    pub const UNIT_MASS: u8 = 1;
    /// Quantity-unit code: volume.
    pub const UNIT_VOLUME: u8 = 2;
    /// Quantity-unit code: dose.
    pub const UNIT_DOSE: u8 = 3;
    /// Quantity-unit code: an arbitrary simulation unit.
    pub const UNIT_ARBITRARY: u8 = 4;

    /// Construct a quantity from a unit code and amount, `None` if the code is out
    /// of range or the amount is negative. Arithmetic lives on the returned value.
    pub fn quantity(&self, unit_code: u8, amount: i64) -> Option<Quantity> {
        QUANTITY_UNITS
            .get(unit_code as usize)
            .and_then(|unit| Quantity::new(*unit, amount))
    }

    /// A residue location on an ECS entity.
    pub fn residue_location_entity(&self, handle: EntityHandle) -> ResidueLocation {
        ResidueLocation::entity(handle)
    }

    /// A residue location named by an opaque symbol code.
    pub fn residue_location_symbol(&self, code: u64) -> ResidueLocation {
        ResidueLocation::symbol(code)
    }
}

impl SimCoreApi {
    /// Canonical material/substance tag: liquid.
    pub const TAG_LIQUID: &'static str = "liquid";
    /// Canonical material/substance tag: solid.
    pub const TAG_SOLID: &'static str = "solid";
    /// Canonical material/substance tag: gas.
    pub const TAG_GAS: &'static str = "gas";
    /// Canonical material/substance tag: edible.
    pub const TAG_EDIBLE: &'static str = "edible";
    /// Canonical material/substance tag: drinkable.
    pub const TAG_DRINKABLE: &'static str = "drinkable";
    /// Canonical material/substance tag: intoxicant.
    pub const TAG_INTOXICANT: &'static str = "intoxicant";
    /// Canonical material/substance tag: toxic.
    pub const TAG_TOXIC: &'static str = "toxic";
    /// Canonical material/substance tag: flammable.
    pub const TAG_FLAMMABLE: &'static str = "flammable";
    /// Canonical material/substance tag: absorbent.
    pub const TAG_ABSORBENT: &'static str = "absorbent";
    /// Canonical material/substance tag: residue-capable.
    pub const TAG_RESIDUE_CAPABLE: &'static str = "residue-capable";
    /// Canonical material/substance tag: contact-transferable.
    pub const TAG_CONTACT_TRANSFERABLE: &'static str = "contact-transferable";

    /// Register a material definition (name, classifier code, tags, typed numeric
    /// properties). Returns its id, or `None` on a duplicate name.
    pub fn register_material(
        &mut self,
        name: &str,
        material_kind: u32,
        tags: &[&str],
        properties: &[(u32, i64)],
    ) -> Option<DefinitionId> {
        let tag_set = tags.iter().fold(TagSet::new(), |set, tag| set.with(tag));
        self.world
            .definitions_mut()
            .register(DefinitionKind::Material, name, tag_set, PropertySet::new())
            .map(|id| {
                let typed: Vec<(MaterialProperty, i64)> = properties
                    .iter()
                    .map(|(key, value)| (MaterialProperty::new(*key), *value))
                    .collect();
                self.world.catalog_mut().register_material(
                    id,
                    MaterialKind::new(material_kind),
                    &typed,
                );
                id
            })
    }

    /// Register a substance definition. Returns its id, or `None` on a duplicate.
    pub fn register_substance(
        &mut self,
        name: &str,
        substance_kind: u32,
        tags: &[&str],
        properties: &[(u32, i64)],
    ) -> Option<DefinitionId> {
        let tag_set = tags.iter().fold(TagSet::new(), |set, tag| set.with(tag));
        self.world
            .definitions_mut()
            .register(DefinitionKind::Substance, name, tag_set, PropertySet::new())
            .map(|id| {
                let typed: Vec<(SubstanceProperty, i64)> = properties
                    .iter()
                    .map(|(key, value)| (SubstanceProperty::new(*key), *value))
                    .collect();
                self.world.catalog_mut().register_substance(
                    id,
                    SubstanceKind::new(substance_kind),
                    &typed,
                );
                id
            })
    }

    /// The material classifier code of a definition, if it is a material.
    pub fn material_kind_code(&self, definition: DefinitionId) -> Option<u32> {
        self.world
            .catalog()
            .material_kind(definition)
            .map(MaterialKind::code)
    }

    /// The substance classifier code of a definition, if it is a substance.
    pub fn substance_kind_code(&self, definition: DefinitionId) -> Option<u32> {
        self.world
            .catalog()
            .substance_kind(definition)
            .map(SubstanceKind::code)
    }

    /// A typed material property value, if present.
    pub fn material_property(&self, definition: DefinitionId, key: u32) -> Option<i64> {
        self.world
            .catalog()
            .material_property(definition, MaterialProperty::new(key))
    }

    /// A typed substance property value, if present.
    pub fn substance_property(&self, definition: DefinitionId, key: u32) -> Option<i64> {
        self.world
            .catalog()
            .substance_property(definition, SubstanceProperty::new(key))
    }

    /// Definitions carrying a tag, in ascending id order.
    pub fn definitions_by_tag(&self, tag: &str) -> Vec<DefinitionId> {
        self.world
            .definitions()
            .by_tag(tag)
            .map(Definition::id)
            .collect()
    }

    /// Definitions whose named property equals a value, in ascending id order.
    pub fn definitions_by_property(&self, name: &str, value: FactValue) -> Vec<DefinitionId> {
        self.world
            .definitions()
            .by_property(name, value)
            .map(Definition::id)
            .collect()
    }
}

impl SimCoreApi {
    /// Create a residue of a definition with a quantity at a location.
    pub fn create_residue(
        &mut self,
        definition: DefinitionId,
        quantity: Quantity,
        location: ResidueLocation,
        state_code: u32,
        cause: Option<CauseRef>,
        tick: u64,
    ) -> ResidueId {
        self.world.residues_mut().create(
            definition,
            quantity,
            location,
            ResidueState::new(state_code),
            cause,
            tick,
        )
    }

    /// Borrow a residue by id (quantity/location/definition readable through it).
    pub fn residue(&self, id: ResidueId) -> Option<&Residue> {
        self.world.residues().get(id)
    }

    /// Remove a residue. Returns whether it existed.
    pub fn remove_residue(&mut self, id: ResidueId) -> bool {
        self.world.residues_mut().remove(id).is_some()
    }

    /// Residue ids at a location, ascending.
    pub fn residues_by_location(&self, location: ResidueLocation) -> Vec<ResidueId> {
        self.world
            .residues()
            .by_location(location)
            .map(Residue::id)
            .collect()
    }

    /// Residue ids of a definition, ascending.
    pub fn residues_by_definition(&self, definition: DefinitionId) -> Vec<ResidueId> {
        self.world
            .residues()
            .by_definition(definition)
            .map(Residue::id)
            .collect()
    }

    /// The number of residues.
    pub fn residue_count(&self) -> usize {
        self.world.residues().len()
    }
}

impl SimCoreApi {
    /// Record an interaction. Returns its id, or `None` if the route code is out
    /// of range (route validation).
    pub fn record_interaction(
        &mut self,
        kind_code: u32,
        route_code: u8,
        primary: EntityHandle,
        secondary: Option<EntityHandle>,
        material: Option<DefinitionId>,
        residue: Option<ResidueId>,
        quantity: Option<Quantity>,
        location: Option<ResidueLocation>,
        tick: u64,
        cause: Option<CauseRef>,
    ) -> Option<InteractionId> {
        InteractionRoute::from_code(route_code).map(|route| {
            self.world.interactions_mut().create(InteractionParams {
                kind: InteractionKind::new(kind_code),
                route,
                primary,
                secondary,
                material,
                residue,
                quantity,
                location,
                tick,
                cause,
            })
        })
    }

    /// Borrow an interaction record by id.
    pub fn interaction(&self, id: InteractionId) -> Option<&InteractionRecord> {
        self.world.interactions().get(id)
    }

    /// Interaction ids whose primary subject is `subject`, ascending.
    pub fn interactions_by_subject(&self, subject: EntityHandle) -> Vec<InteractionId> {
        self.world
            .interactions()
            .by_subject(subject)
            .map(InteractionRecord::id)
            .collect()
    }

    /// Interaction ids on a route code, ascending (empty for an invalid code).
    pub fn interactions_by_route(&self, route_code: u8) -> Vec<InteractionId> {
        InteractionRoute::from_code(route_code).map_or_else(Vec::new, |route| {
            self.world
                .interactions()
                .by_route(route)
                .map(InteractionRecord::id)
                .collect()
        })
    }

    /// The number of interaction records.
    pub fn interaction_count(&self) -> usize {
        self.world.interactions().len()
    }
}

impl SimCoreApi {
    /// Register a fixed-amount transfer rule on a route. `None` if the route code
    /// is out of range or the rule is invalid.
    pub fn register_transfer_fixed(
        &mut self,
        amount: i64,
        route_code: u8,
        lossy: bool,
    ) -> Option<TransferRuleId> {
        InteractionRoute::from_code(route_code).and_then(|route| {
            self.world
                .transfers_mut()
                .register(TransferMode::fixed(amount), route, lossy)
        })
    }

    /// Register a percentage (basis-points) transfer rule on a route.
    pub fn register_transfer_percentage(
        &mut self,
        basis_points: i64,
        route_code: u8,
        lossy: bool,
    ) -> Option<TransferRuleId> {
        InteractionRoute::from_code(route_code).and_then(|route| {
            self.world.transfers_mut().register(
                TransferMode::percentage(basis_points),
                route,
                lossy,
            )
        })
    }

    /// Register an all-available-up-to-max transfer rule on a route.
    pub fn register_transfer_all_up_to(
        &mut self,
        max: i64,
        route_code: u8,
        lossy: bool,
    ) -> Option<TransferRuleId> {
        InteractionRoute::from_code(route_code).and_then(|route| {
            self.world
                .transfers_mut()
                .register(TransferMode::all_up_to(max), route, lossy)
        })
    }

    /// Register a no-op transfer rule on a route.
    pub fn register_transfer_none(
        &mut self,
        route_code: u8,
        lossy: bool,
    ) -> Option<TransferRuleId> {
        InteractionRoute::from_code(route_code).and_then(|route| {
            self.world
                .transfers_mut()
                .register(TransferMode::none(), route, lossy)
        })
    }

    /// Apply a transfer rule consuming an interaction, depositing at
    /// `target_location`. Emits a causal event. `None` if the rule or interaction
    /// id is unknown; otherwise a structured [`TransferResult`].
    pub fn apply_transfer(
        &mut self,
        rule_id: TransferRuleId,
        interaction_id: InteractionId,
        target_location: ResidueLocation,
        event_kind: u32,
        event_code: u64,
        tick: u64,
    ) -> Option<TransferResult> {
        let rule = self.world.transfers().get(rule_id).copied();
        let interaction = self.world.interactions().get(interaction_id).copied();
        rule.zip(interaction).map(|(rule, interaction)| {
            self.world.apply_transfer(
                rule,
                &interaction,
                target_location,
                event_kind,
                event_code,
                tick,
            )
        })
    }

    /// The number of transfer rules.
    pub fn transfer_rule_count(&self) -> usize {
        self.world.transfers().len()
    }
}

impl SimCoreApi {
    /// Material-effect-rule kind code: add fact.
    pub const EFFECT_ADD_FACT: u8 = 0;
    /// Material-effect-rule kind code: update fact.
    pub const EFFECT_UPDATE_FACT: u8 = 1;
    /// Material-effect-rule kind code: remove fact.
    pub const EFFECT_REMOVE_FACT: u8 = 2;
    /// Material-effect-rule kind code: add relation.
    pub const EFFECT_ADD_RELATION: u8 = 3;
    /// Material-effect-rule kind code: emit causal event.
    pub const EFFECT_EMIT_CAUSAL_EVENT: u8 = 4;
    /// Material-effect-rule kind code: schedule process.
    pub const EFFECT_SCHEDULE_PROCESS: u8 = 5;

    /// Register a material effect rule. `None` if the route or effect-kind code is
    /// out of range.
    pub fn register_material_effect_rule(
        &mut self,
        match_tag: Option<&str>,
        route_code: u8,
        effect_kind_code: u8,
        concept_code: u32,
        value: Option<FactValue>,
        relation_symbol: u64,
        event_code: u64,
        process_state: u32,
        process_wake: u64,
    ) -> Option<MaterialEffectRuleId> {
        let route = InteractionRoute::from_code(route_code);
        let kind = MATERIAL_EFFECT_KINDS
            .get(effect_kind_code as usize)
            .copied();
        route.zip(kind).map(|(route, kind)| {
            self.world
                .effect_rules_mut()
                .register(MaterialEffectRuleParams {
                    match_tag: match_tag.map(str::to_string),
                    match_route: route,
                    kind,
                    concept_code,
                    value,
                    relation_symbol,
                    event_code,
                    process_state,
                    process_wake,
                })
        })
    }

    /// Produce (without applying) the effects of material rules matching an
    /// interaction into a fresh batch. `None` if the interaction id is unknown.
    pub fn produce_material_effects(
        &self,
        interaction_id: InteractionId,
        context_fact: Option<FactId>,
        cause: Option<CauseRef>,
    ) -> Option<EffectBatch> {
        self.world
            .interactions()
            .get(interaction_id)
            .copied()
            .map(|interaction| {
                self.world
                    .produce_material_effects(&interaction, context_fact, cause)
            })
    }

    /// Produce and apply material effects for an interaction at this boundary.
    /// `None` if the interaction id is unknown.
    pub fn apply_material_effects(
        &mut self,
        interaction_id: InteractionId,
        context_fact: Option<FactId>,
        cause: Option<CauseRef>,
        registry: &EntityRegistry,
    ) -> Option<MaterialEffectResult> {
        let interaction = self.world.interactions().get(interaction_id).copied();
        interaction.map(|interaction| {
            self.world
                .apply_material_effects(interaction, context_fact, cause, registry)
        })
    }

    /// The number of material effect rules.
    pub fn material_effect_rule_count(&self) -> usize {
        self.world.effect_rules().len()
    }
}

impl SimCoreApi {
    /// Whether a definition is cataloged as a material or substance.
    pub fn is_cataloged(&self, definition: DefinitionId) -> bool {
        self.world.catalog().contains(definition)
    }

    /// The number of cataloged material/substance definitions.
    pub fn cataloged_count(&self) -> usize {
        self.world.catalog().len()
    }

    /// All cataloged definition ids, ascending.
    pub fn all_cataloged_definition_ids(&self) -> Vec<DefinitionId> {
        self.world.catalog().iter().collect()
    }

    /// All residue ids, ascending.
    pub fn all_residue_ids(&self) -> Vec<ResidueId> {
        self.world.residues().iter().map(Residue::id).collect()
    }

    /// All interaction ids, ascending.
    pub fn all_interaction_ids(&self) -> Vec<InteractionId> {
        self.world
            .interactions()
            .iter()
            .map(InteractionRecord::id)
            .collect()
    }

    /// Borrow a transfer rule by id (mode/route/lossy readable through it).
    pub fn transfer_rule(&self, id: TransferRuleId) -> Option<&TransferRule> {
        self.world.transfers().get(id)
    }

    /// Remove a transfer rule. Returns whether it existed.
    pub fn remove_transfer_rule(&mut self, id: TransferRuleId) -> bool {
        self.world.transfers_mut().remove(id).is_some()
    }

    /// Transfer-rule ids on a route code, ascending (empty for an invalid code).
    pub fn transfer_rules_by_route(&self, route_code: u8) -> Vec<TransferRuleId> {
        InteractionRoute::from_code(route_code).map_or_else(Vec::new, |route| {
            self.world
                .transfers()
                .by_route(route)
                .map(TransferRule::id)
                .collect()
        })
    }

    /// All transfer-rule ids, ascending.
    pub fn all_transfer_rule_ids(&self) -> Vec<TransferRuleId> {
        self.world
            .transfers()
            .iter()
            .map(TransferRule::id)
            .collect()
    }

    /// Borrow a material effect rule by id (its effect kind is readable through it).
    pub fn material_effect_rule(&self, id: MaterialEffectRuleId) -> Option<&MaterialEffectRule> {
        self.world.effect_rules().get(id)
    }

    /// All material-effect-rule ids, ascending.
    pub fn all_material_effect_rule_ids(&self) -> Vec<MaterialEffectRuleId> {
        self.world
            .effect_rules()
            .iter()
            .map(MaterialEffectRule::id)
            .collect()
    }
}

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
