//! Generic material/substance effect rules that produce Phase-2 effects.
//!
//! A rule matches an interaction by the involved material's tag and the route,
//! then *produces* Phase-2 [`crate::EffectBatch`] entries — it never mutates
//! state itself. The produced batch is applied at the normal effect boundary.
//! This is the substrate for later toxicology/reaction systems, with no domain
//! behavior baked in.

use std::collections::BTreeMap;

use crate::cause::CauseRef;
use crate::definition::DefinitionRegistry;
use crate::effect::EffectBatch;
use crate::fact::FactValue;
use crate::ids::{FactId, MaterialEffectRuleId};
use crate::interaction::{InteractionRecord, InteractionRoute};
use crate::relation::RelationEndpoint;

const ADD_FACT: u8 = 0;
const UPDATE_FACT: u8 = 1;
const REMOVE_FACT: u8 = 2;
const ADD_RELATION: u8 = 3;
const EMIT_CAUSAL_EVENT: u8 = 4;
const SCHEDULE_PROCESS: u8 = 5;

/// The kind of Phase-2 effect a [`MaterialEffectRule`] produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MaterialEffectKind {
    /// Produce an add-fact effect on the interaction's primary subject.
    AddFact,
    /// Produce an update-fact effect on the context fact.
    UpdateFact,
    /// Produce a remove-fact effect on the context fact.
    RemoveFact,
    /// Produce an add-relation effect between the interaction's subjects.
    AddRelation,
    /// Produce an emit-causal-event effect.
    EmitCausalEvent,
    /// Produce a schedule-process effect on the primary subject.
    ScheduleProcess,
}

const KINDS: [MaterialEffectKind; 6] = [
    MaterialEffectKind::AddFact,
    MaterialEffectKind::UpdateFact,
    MaterialEffectKind::RemoveFact,
    MaterialEffectKind::AddRelation,
    MaterialEffectKind::EmitCausalEvent,
    MaterialEffectKind::ScheduleProcess,
];

/// Parameters describing one material effect rule (match criteria + templated
/// effect). Built by the facade from primitive codes.
#[derive(Debug, Clone)]
pub struct MaterialEffectRuleParams {
    /// A tag the interaction's material must carry to match (`None` = any).
    pub match_tag: Option<String>,
    /// The route the interaction must be on to match.
    pub match_route: InteractionRoute,
    /// Which effect to produce.
    pub kind: MaterialEffectKind,
    /// Fact/relation/event/process kind code (used per `kind`).
    pub concept_code: u32,
    /// The fact/event value/payload (used by add/update/emit).
    pub value: Option<FactValue>,
    /// The symbol code for the relation's second endpoint when there is no
    /// secondary entity (add-relation only).
    pub relation_symbol: u64,
    /// Event code (emit only).
    pub event_code: u64,
    /// Process state + wake (schedule only).
    pub process_state: u32,
    /// Process wake tick (schedule only).
    pub process_wake: u64,
}

/// A registered material effect rule.
#[derive(Debug, Clone)]
pub struct MaterialEffectRule {
    id: MaterialEffectRuleId,
    tag: u8,
    params: MaterialEffectRuleParams,
}

impl MaterialEffectRule {
    /// This rule's stable id.
    pub fn id(&self) -> MaterialEffectRuleId {
        self.id
    }

    /// The effect kind this rule produces.
    pub fn effect_kind(&self) -> MaterialEffectKind {
        KINDS[self.tag as usize]
    }

    /// Whether this rule matches `interaction` (route matches and, if a tag is
    /// required, the interaction's material carries it).
    fn matches(&self, interaction: &InteractionRecord, definitions: &DefinitionRegistry) -> bool {
        let tag_ok = self.params.match_tag.as_deref().is_none_or(|tag| {
            interaction
                .material()
                .and_then(|def| definitions.get(def))
                .is_some_and(|definition| definition.has_tag(tag))
        });
        (self.params.match_route == interaction.route()) & tag_ok
    }

    /// Append this rule's templated effect to `batch`.
    fn produce(
        &self,
        batch: &mut EffectBatch,
        interaction: &InteractionRecord,
        context_fact: Option<FactId>,
        cause: Option<CauseRef>,
    ) {
        let primary = interaction.primary();
        let tick = interaction.tick();
        let p = &self.params;
        (self.tag == ADD_FACT).then(|| {
            p.value
                .map(|value| batch.add_fact(p.concept_code, primary, value, cause, tick))
        });
        (self.tag == UPDATE_FACT).then(|| {
            context_fact
                .zip(p.value)
                .map(|(fact, value)| batch.update_fact(fact, value, tick))
        });
        (self.tag == REMOVE_FACT).then(|| context_fact.map(|fact| batch.remove_fact(fact)));
        (self.tag == ADD_RELATION).then(|| {
            let secondary = interaction.secondary().map_or(
                RelationEndpoint::symbol(p.relation_symbol),
                RelationEndpoint::entity,
            );
            batch.add_relation(
                p.concept_code,
                vec![RelationEndpoint::entity(primary), secondary],
                None,
                cause,
            );
        });
        (self.tag == EMIT_CAUSAL_EVENT).then(|| {
            batch.emit_causal_event(
                p.concept_code,
                tick,
                (Some(primary), interaction.secondary()),
                cause,
                p.event_code,
                p.value,
            )
        });
        (self.tag == SCHEDULE_PROCESS).then(|| {
            batch.schedule_process(
                p.concept_code,
                primary,
                p.process_state,
                p.process_wake,
                cause,
            )
        });
    }
}

/// The summary of producing/applying material effects for one interaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MaterialEffectResult {
    matched: usize,
    applied: usize,
}

impl MaterialEffectResult {
    /// Build a result.
    pub(crate) fn new(matched: usize, applied: usize) -> Self {
        MaterialEffectResult { matched, applied }
    }

    /// How many rules matched the interaction.
    pub const fn matched(&self) -> usize {
        self.matched
    }

    /// How many produced effects were applied at the boundary.
    pub const fn applied(&self) -> usize {
        self.applied
    }
}

/// A deterministic store of material effect rules, keyed/iterated by ascending id.
#[derive(Debug, Clone, Default)]
pub struct MaterialEffectRuleStore {
    rules: BTreeMap<MaterialEffectRuleId, MaterialEffectRule>,
    next: u64,
}

const KIND_TAGS: [u8; 6] = [
    ADD_FACT,
    UPDATE_FACT,
    REMOVE_FACT,
    ADD_RELATION,
    EMIT_CAUSAL_EVENT,
    SCHEDULE_PROCESS,
];

impl MaterialEffectRuleStore {
    /// Create an empty store. The first rule has id 1.
    pub fn new() -> Self {
        MaterialEffectRuleStore {
            rules: BTreeMap::new(),
            next: 1,
        }
    }

    /// Register a rule, minting and returning its deterministic id.
    pub fn register(&mut self, params: MaterialEffectRuleParams) -> MaterialEffectRuleId {
        let id = MaterialEffectRuleId::from_raw(self.next);
        self.next += 1;
        let tag = KIND_TAGS[params.kind as usize];
        self.rules
            .insert(id, MaterialEffectRule { id, tag, params });
        id
    }

    /// Borrow a rule by id, if present.
    pub fn get(&self, id: MaterialEffectRuleId) -> Option<&MaterialEffectRule> {
        self.rules.get(&id)
    }

    /// Append the effects of every rule matching `interaction` to `batch`,
    /// returning how many rules matched. Rules are evaluated in ascending id
    /// order, so produced effects are deterministic.
    pub fn produce_into(
        &self,
        batch: &mut EffectBatch,
        interaction: &InteractionRecord,
        context_fact: Option<FactId>,
        cause: Option<CauseRef>,
        definitions: &DefinitionRegistry,
    ) -> usize {
        self.rules
            .values()
            .filter(|rule| rule.matches(interaction, definitions))
            .map(|rule| rule.produce(batch, interaction, context_fact, cause))
            .count()
    }

    /// All rules, in ascending id order.
    pub fn iter(&self) -> impl Iterator<Item = &MaterialEffectRule> {
        self.rules.values()
    }

    /// The number of rules.
    pub fn len(&self) -> usize {
        self.rules.len()
    }

    /// Whether the store holds no rules.
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::definition::{DefinitionKind, PropertySet, TagSet};
    use crate::interaction::{InteractionKind, InteractionParams, InteractionStore};
    use axiom_ecs::EntityRegistry;

    fn rule_params(
        tag: &str,
        route: InteractionRoute,
        kind: MaterialEffectKind,
    ) -> MaterialEffectRuleParams {
        MaterialEffectRuleParams {
            match_tag: Some(tag.to_string()),
            match_route: route,
            kind,
            concept_code: 1,
            value: Some(FactValue::Unsigned(1)),
            relation_symbol: 0,
            event_code: 0,
            process_state: 0,
            process_wake: 0,
        }
    }

    #[test]
    fn new_and_default_are_empty() {
        assert!(MaterialEffectRuleStore::new().is_empty());
        assert_eq!(MaterialEffectRuleStore::new().len(), 0);
        assert!(MaterialEffectRuleStore::default().is_empty());
    }

    #[test]
    fn matching_produces_effects_only_for_tag_and_route() {
        let mut reg = EntityRegistry::new();
        let a = reg.spawn_handle();
        let mut definitions = DefinitionRegistry::new();
        let liquid = definitions
            .register(
                DefinitionKind::Substance,
                "test-liquid",
                TagSet::new().with("contact-transferable"),
                PropertySet::new(),
            )
            .unwrap();

        let mut store = MaterialEffectRuleStore::new();
        let r = store.register(rule_params(
            "contact-transferable",
            InteractionRoute::Touch,
            MaterialEffectKind::AddFact,
        ));
        assert_eq!(
            store.get(r).unwrap().effect_kind(),
            MaterialEffectKind::AddFact
        );

        // Interaction with the matching material + route.
        let mut interactions = InteractionStore::new();
        let mut p = InteractionParams {
            kind: InteractionKind::new(1),
            route: InteractionRoute::Touch,
            primary: a,
            secondary: None,
            material: Some(liquid),
            residue: None,
            quantity: None,
            location: None,
            tick: 0,
            cause: None,
        };
        let matching = interactions.create(p);
        p.route = InteractionRoute::Adjacent;
        let wrong_route = interactions.create(p);

        let mut batch = EffectBatch::new();
        let matched = store.produce_into(
            &mut batch,
            interactions.get(matching).unwrap(),
            None,
            Some(CauseRef::Command),
            &definitions,
        );
        assert_eq!(matched, 1, "tag + route matched");
        assert_eq!(batch.len(), 1, "one effect produced");

        let mut batch2 = EffectBatch::new();
        let matched2 = store.produce_into(
            &mut batch2,
            interactions.get(wrong_route).unwrap(),
            None,
            None,
            &definitions,
        );
        assert_eq!(matched2, 0, "route mismatch produced nothing");
        assert!(batch2.is_empty());
    }

    #[test]
    fn add_relation_and_schedule_process_rules_produce_their_effects() {
        use crate::effect::EffectKind;
        let mut reg = EntityRegistry::new();
        let a = reg.spawn_handle();
        let b = reg.spawn_handle();
        let mut definitions = DefinitionRegistry::new();
        let material = definitions
            .register(
                DefinitionKind::Substance,
                "test-bond",
                TagSet::new().with("bonds"),
                PropertySet::new(),
            )
            .unwrap();

        let mut store = MaterialEffectRuleStore::new();
        store.register(rule_params(
            "bonds",
            InteractionRoute::Touch,
            MaterialEffectKind::AddRelation,
        ));
        store.register(rule_params(
            "bonds",
            InteractionRoute::Touch,
            MaterialEffectKind::ScheduleProcess,
        ));

        let mut interactions = InteractionStore::new();
        let mut p = InteractionParams {
            kind: InteractionKind::new(1),
            route: InteractionRoute::Touch,
            primary: a,
            secondary: Some(b),
            material: Some(material),
            residue: None,
            quantity: None,
            location: None,
            tick: 0,
            cause: None,
        };
        let with_secondary = interactions.create(p);
        p.secondary = None;
        let without_secondary = interactions.create(p);

        let mut batch = EffectBatch::new();
        let matched = store.produce_into(
            &mut batch,
            interactions.get(with_secondary).unwrap(),
            None,
            Some(CauseRef::Command),
            &definitions,
        );
        assert_eq!(matched, 2);
        assert_eq!(batch.len(), 2);
        assert_eq!(batch.kind_at(0), Some(EffectKind::AddRelation));
        assert_eq!(batch.kind_at(1), Some(EffectKind::ScheduleProcess));

        let mut batch2 = EffectBatch::new();
        let matched2 = store.produce_into(
            &mut batch2,
            interactions.get(without_secondary).unwrap(),
            None,
            None,
            &definitions,
        );
        assert_eq!(matched2, 2);
        assert_eq!(batch2.kind_at(0), Some(EffectKind::AddRelation));
        assert_eq!(batch2.kind_at(1), Some(EffectKind::ScheduleProcess));
    }

    #[test]
    fn remove_fact_rule_removes_the_context_fact() {
        use crate::effect::EffectKind;
        let mut reg = EntityRegistry::new();
        let a = reg.spawn_handle();
        let mut definitions = DefinitionRegistry::new();
        let material = definitions
            .register(
                DefinitionKind::Substance,
                "test-solvent",
                TagSet::new().with("dissolves"),
                PropertySet::new(),
            )
            .unwrap();

        let mut store = MaterialEffectRuleStore::new();
        store.register(rule_params(
            "dissolves",
            InteractionRoute::Touch,
            MaterialEffectKind::RemoveFact,
        ));

        let mut interactions = InteractionStore::new();
        let p = InteractionParams {
            kind: InteractionKind::new(1),
            route: InteractionRoute::Touch,
            primary: a,
            secondary: None,
            material: Some(material),
            residue: None,
            quantity: None,
            location: None,
            tick: 0,
            cause: None,
        };
        let id = interactions.create(p);

        let mut batch = EffectBatch::new();
        let matched = store.produce_into(
            &mut batch,
            interactions.get(id).unwrap(),
            Some(FactId::from_raw(7)),
            None,
            &definitions,
        );
        assert_eq!(matched, 1);
        assert_eq!(batch.len(), 1);
        assert_eq!(batch.kind_at(0), Some(EffectKind::RemoveFact));
    }

    #[test]
    fn result_reports_matched_and_applied() {
        let result = MaterialEffectResult::new(2, 2);
        assert_eq!(result.matched(), 2);
        assert_eq!(result.applied(), 2);
    }
}
