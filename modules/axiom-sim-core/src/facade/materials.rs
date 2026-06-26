//! The materials/substances facade surface of `SimCoreApi`: quantities, residue
//! locations, material/substance registration, residues, interactions, transfer
//! rules, material-effect rules, and their catalog accessors.
//!
//! A child module of `facade`, so it may use the private `world` field. Every
//! method routes through the same opaque-value discipline as the rest of the
//! facade: internal types are returned only as values or references, never named
//! by consumers.

use axiom_ecs::{EntityHandle, EntityRegistry};

use crate::cause::CauseRef;
use crate::definition::{Definition, DefinitionKind, PropertySet, TagSet};
use crate::effect::EffectBatch;
use crate::fact::FactValue;
use crate::ids::{
    DefinitionId, FactId, InteractionId, MaterialEffectRuleId, ResidueId, TransferRuleId,
};
use crate::interaction::{InteractionKind, InteractionParams, InteractionRecord, InteractionRoute};
use crate::material::{MaterialKind, MaterialProperty, SubstanceKind, SubstanceProperty};
use crate::material_effect::{
    MaterialEffectKind, MaterialEffectResult, MaterialEffectRule, MaterialEffectRuleParams,
};
use crate::quantity::{Quantity, QuantityUnit};
use crate::residue::{Residue, ResidueLocation, ResidueState};
use crate::transfer::{TransferMode, TransferResult, TransferRule};

use super::SimCoreApi;

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
            .inspect(|id| {
                let typed: Vec<(MaterialProperty, i64)> = properties
                    .iter()
                    .map(|(key, value)| (MaterialProperty::new(*key), *value))
                    .collect();
                self.world.catalog_mut().register_material(
                    *id,
                    MaterialKind::new(material_kind),
                    &typed,
                );
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
            .inspect(|id| {
                let typed: Vec<(SubstanceProperty, i64)> = properties
                    .iter()
                    .map(|(key, value)| (SubstanceProperty::new(*key), *value))
                    .collect();
                self.world.catalog_mut().register_substance(
                    *id,
                    SubstanceKind::new(substance_kind),
                    &typed,
                );
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
    /// of range (route validation). `parties` is `(primary, secondary)`;
    /// `substance` is `(material, residue, quantity, location)`; `provenance` is
    /// `(tick, cause)`.
    pub fn record_interaction(
        &mut self,
        kind_code: u32,
        route_code: u8,
        parties: (EntityHandle, Option<EntityHandle>),
        substance: (
            Option<DefinitionId>,
            Option<ResidueId>,
            Option<Quantity>,
            Option<ResidueLocation>,
        ),
        provenance: (u64, Option<CauseRef>),
    ) -> Option<InteractionId> {
        InteractionRoute::from_code(route_code).map(|route| {
            self.world.interactions_mut().create(InteractionParams {
                kind: InteractionKind::new(kind_code),
                route,
                primary: parties.0,
                secondary: parties.1,
                material: substance.0,
                residue: substance.1,
                quantity: substance.2,
                location: substance.3,
                tick: provenance.0,
                cause: provenance.1,
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
    /// out of range. `selector` is `(match_tag, route_code, effect_kind_code)`;
    /// `effect` is `(concept_code, value, relation_symbol, event_code)`; `process`
    /// is `(process_state, process_wake)`.
    pub fn register_material_effect_rule(
        &mut self,
        selector: (Option<&str>, u8, u8),
        effect: (u32, Option<FactValue>, u64, u64),
        process: (u32, u64),
    ) -> Option<MaterialEffectRuleId> {
        let (match_tag, route_code, effect_kind_code) = selector;
        let (concept_code, value, relation_symbol, event_code) = effect;
        let (process_state, process_wake) = process;
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
