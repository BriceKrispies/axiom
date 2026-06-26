//! Facade tests (a child of `facade`, so `use super::*` sees its private items).

use super::*;
use crate::transfer::TransferMode;

#[test]
fn new_and_default_are_empty() {
    let api = SimCoreApi::new();
    assert!(api.is_empty());
    assert_eq!(api.fact_count(), 0);
    assert_eq!(api.relation_count(), 0);
    assert_eq!(api.process_count(), 0);
    assert_eq!(api.scheduled_process_count(), 0);
    assert_eq!(api.definition_count(), 0);
    assert_eq!(api.causal_event_count(), 0);
    assert_eq!(SimCoreApi::default().fact_count(), 0);
    assert!(format!("{api:?}").contains("SimCoreApi"));
}

#[test]
fn reference_getters_and_iteration_are_reachable() {
    let mut reg = EntityRegistry::new();
    let a = reg.spawn_handle();
    let b = reg.spawn_handle();
    let mut api = SimCoreApi::new();
    let f = api
        .add_fact(
            &reg,
            7,
            a,
            api.value_signed(-2),
            Some(api.cause_command()),
            3,
        )
        .unwrap();
    // Rich fields are readable through the returned reference.
    let fact = api.fact(f).unwrap();
    assert_eq!(fact.kind().code(), 7);
    assert_eq!(fact.subject(), a);
    assert_eq!(fact.value(), FactValue::Signed(-2));
    assert_eq!(fact.cause(), Some(CauseRef::Command));
    assert_eq!(fact.tick(), 3);
    assert!(!api.is_empty());
    assert!(api.fact(FactId::from_raw(99)).is_none());

    let r = api
        .add_relation(
            &reg,
            5,
            vec![api.endpoint_entity(a), api.endpoint_entity(b)],
            Some(4),
            None,
        )
        .unwrap();
    let relation = api.relation(r).unwrap();
    assert_eq!(relation.kind().code(), 5);
    assert_eq!(relation.strength(), Some(4));
    assert_eq!(relation.endpoints().len(), 2);

    let p = api.schedule_process(&reg, 9, a, 2, 6, None).unwrap();
    let process = api.process(p).unwrap();
    assert_eq!(process.kind().code(), 9);
    assert_eq!(process.state().code(), 2);

    let e = api.append_causal_event(
        11,
        1,
        (Some(a), Some(b)),
        None,
        88,
        Some(api.value_bool(true)),
    );
    let event = api.causal_event(e).unwrap();
    assert_eq!(event.kind().code(), 11);
    assert_eq!(event.secondary(), Some(b));
    assert_eq!(event.code(), 88);
    assert_eq!(event.payload(), Some(FactValue::Bool(true)));

    let d = api
        .register_definition(SimCoreApi::KIND_TISSUE, "muscle", &["soft"], &[])
        .unwrap();
    let definition = api.definition(d).unwrap();
    assert_eq!(definition.name(), "muscle");
    assert!(definition.tags().contains("soft"));
    assert_eq!(definition.properties().len(), 0);
    assert_eq!(api.definition_by_name("muscle").unwrap().id(), d);
    assert!(api.definition_by_name("absent").is_none());

    // Deterministic iteration through the facade.
    assert_eq!(api.all_fact_ids(), vec![f]);
    assert_eq!(api.all_relation_ids(), vec![r]);
    assert_eq!(api.all_process_ids(), vec![p]);
    assert_eq!(api.all_causal_event_ids(), vec![e]);
    assert_eq!(api.all_definition_ids(), vec![d]);
    assert_eq!(api.scheduled_process_count(), 1);
}

#[test]
fn value_cause_and_endpoint_constructors() {
    let mut reg = EntityRegistry::new();
    let h = reg.spawn_handle();
    let api = SimCoreApi::new();
    assert_eq!(api.value_signed(-1), FactValue::Signed(-1));
    assert_eq!(api.value_unsigned(2), FactValue::Unsigned(2));
    assert_eq!(api.value_symbol(3), FactValue::Symbol(3));
    assert_eq!(api.value_bool(true), FactValue::Bool(true));
    assert_eq!(api.value_entity(h), FactValue::Entity(h));
    assert_eq!(api.cause_command(), CauseRef::Command);
    assert_eq!(
        api.cause_event(CausalEventId::from_raw(1)),
        CauseRef::Event(CausalEventId::from_raw(1))
    );
    assert_eq!(
        api.cause_process(ProcessId::from_raw(1)),
        CauseRef::Process(ProcessId::from_raw(1))
    );
    assert_eq!(
        api.cause_rule(RuleId::from_raw(1)),
        CauseRef::Rule(RuleId::from_raw(1))
    );
    assert_eq!(api.endpoint_entity(h), RelationEndpoint::entity(h));
    assert_eq!(api.endpoint_symbol(5), RelationEndpoint::symbol(5));
}

#[test]
fn definitions_through_the_facade() {
    let mut api = SimCoreApi::new();
    let id = api
        .register_definition(
            SimCoreApi::KIND_MATERIAL,
            "iron",
            &["metal", "conductive"],
            &[("hardness", api.value_unsigned(5))],
        )
        .unwrap();
    assert_eq!(api.definition_count(), 1);
    assert_eq!(api.definition_id("iron"), Some(id));
    assert_eq!(
        api.definition_kind_code(id),
        Some(SimCoreApi::KIND_MATERIAL)
    );
    assert!(api.definition_has_tag(id, "metal"));
    assert!(!api.definition_has_tag(id, "brittle"));
    assert_eq!(
        api.definition_property(id, "hardness"),
        Some(FactValue::Unsigned(5))
    );
    assert_eq!(api.definition_property(id, "missing"), None);
    // Query by named property value: the matching value finds it; a different
    // value or an unknown name finds nothing.
    assert_eq!(
        api.definitions_by_property("hardness", api.value_unsigned(5)),
        vec![id]
    );
    assert!(api
        .definitions_by_property("hardness", api.value_unsigned(9))
        .is_empty());
    assert!(api
        .definitions_by_property("missing", api.value_unsigned(5))
        .is_empty());
    // Duplicate name rejected; out-of-range kind code rejected.
    assert!(api
        .register_definition(SimCoreApi::KIND_SUBSTANCE, "iron", &[], &[])
        .is_none());
    assert!(api.register_definition(250, "tungsten", &[], &[]).is_none());
    // Generic kind round-trips its code.
    let g = api
        .register_definition(SimCoreApi::KIND_GENERIC, "thing", &[], &[])
        .unwrap();
    assert_eq!(api.definition_kind_code(g), Some(SimCoreApi::KIND_GENERIC));
    assert_eq!(api.definition_kind_code(DefinitionId::from_raw(0)), None);
}

#[test]
fn facts_and_relations_reject_stale_entities() {
    let mut reg = EntityRegistry::new();
    let a = reg.spawn_handle();
    let dead = reg.spawn_handle();
    reg.despawn_handle(dead);
    let mut api = SimCoreApi::new();

    assert!(api
        .add_fact(&reg, 1, dead, api.value_bool(true), None, 0)
        .is_none());
    let f = api
        .add_fact(&reg, 1, a, api.value_unsigned(1), None, 0)
        .unwrap();
    assert_eq!(api.fact_value(f), Some(FactValue::Unsigned(1)));
    assert!(api.update_fact(f, api.value_unsigned(2), 1));
    assert_eq!(api.fact_value(f), Some(FactValue::Unsigned(2)));
    assert_eq!(api.facts_by_kind(1), vec![f]);
    assert_eq!(api.facts_by_subject(a), vec![f]);
    assert!(api.remove_fact(f));
    assert!(!api.remove_fact(f));

    assert!(api
        .add_relation(&reg, 1, vec![api.endpoint_entity(dead)], None, None)
        .is_none());
    let r = api
        .add_relation(
            &reg,
            1,
            vec![api.endpoint_entity(a), api.endpoint_symbol(9)],
            Some(2),
            None,
        )
        .unwrap();
    assert_eq!(api.relations_by_kind(1), vec![r]);
    assert_eq!(api.relations_by_endpoint(api.endpoint_entity(a)), vec![r]);
    assert_eq!(api.relation_count(), 1);
    assert!(api.remove_relation(r));
}

#[test]
fn processes_through_the_facade() {
    let mut reg = EntityRegistry::new();
    let a = reg.spawn_handle();
    let dead = reg.spawn_handle();
    reg.despawn_handle(dead);
    let mut api = SimCoreApi::new();
    assert!(api.schedule_process(&reg, 1, dead, 0, 5, None).is_none());
    let p = api.schedule_process(&reg, 1, a, 0, 5, None).unwrap();
    assert_eq!(api.process_wake(p), Some(5));
    assert!(api.wake_due(1).is_empty());
    assert!(api.reschedule_process(p, 1));
    assert_eq!(api.wake_due(1), vec![p]);
    // Re-arm and cancel.
    api.reschedule_process(p, 9);
    assert!(api.cancel_process(p));
    assert_eq!(api.process_count(), 0);
}

#[test]
fn causal_events_through_the_facade() {
    let mut reg = EntityRegistry::new();
    let a = reg.spawn_handle();
    let mut api = SimCoreApi::new();
    let root = api.append_causal_event(1, 0, (Some(a), None), None, 1, None);
    let child =
        api.append_causal_event(2, 1, (Some(a), None), Some(api.cause_event(root)), 2, None);
    assert_eq!(api.causal_event_count(), 2);
    assert_eq!(api.events_by_subject(a), vec![root, child]);
    assert_eq!(api.events_by_parent(api.cause_event(root)), vec![child]);
    assert_eq!(api.event_parent(child), Some(api.cause_event(root)));
    assert_eq!(api.event_parent(root), None);
}

// ---- Phase 3 ----

#[test]
fn quantity_construction_through_the_facade() {
    let api = SimCoreApi::new();
    let a = api.quantity(SimCoreApi::UNIT_VOLUME, 10).unwrap();
    let b = api.quantity(SimCoreApi::UNIT_VOLUME, 3).unwrap();
    assert_eq!(a.add(b).unwrap().amount(), 13);
    assert_eq!(a.sub(b).unwrap().amount(), 7);
    assert!(
        api.quantity(SimCoreApi::UNIT_MASS, -1).is_none(),
        "negative amount rejected"
    );
    assert!(
        api.quantity(250, 5).is_none(),
        "out-of-range unit code rejected"
    );
    // Incompatible units cannot be combined.
    let mass = api.quantity(SimCoreApi::UNIT_MASS, 1).unwrap();
    assert!(a.add(mass).is_none());
}

#[test]
fn materials_and_substances_through_the_facade() {
    let mut api = SimCoreApi::new();
    let iron = api
        .register_material(
            "iron",
            1,
            &[SimCoreApi::TAG_SOLID, SimCoreApi::TAG_FLAMMABLE],
            &[(1, 5)],
        )
        .unwrap();
    let water = api
        .register_substance(
            "water",
            2,
            &[SimCoreApi::TAG_LIQUID, SimCoreApi::TAG_DRINKABLE],
            &[(2, 0)],
        )
        .unwrap();
    // Duplicate names rejected.
    assert!(api.register_material("iron", 9, &[], &[]).is_none());
    assert_eq!(api.material_kind_code(iron), Some(1));
    assert_eq!(
        api.material_kind_code(water),
        None,
        "a substance is not a material"
    );
    assert_eq!(api.substance_kind_code(water), Some(2));
    assert_eq!(api.material_property(iron, 1), Some(5));
    assert_eq!(api.material_property(iron, 99), None);
    assert_eq!(api.substance_property(water, 2), Some(0));
    assert!(api.is_cataloged(iron) && api.is_cataloged(water));
    assert_eq!(api.cataloged_count(), 2);
    assert_eq!(api.all_cataloged_definition_ids().len(), 2);
    // Tag queries (generic registry).
    let solids = api.definitions_by_tag(SimCoreApi::TAG_SOLID);
    assert_eq!(solids, vec![iron]);
    assert_eq!(api.definitions_by_tag(SimCoreApi::TAG_GAS).len(), 0);
    // The Phase-2 generic definition surface still works for materials.
    assert!(api.definition_has_tag(iron, SimCoreApi::TAG_SOLID));
    assert_eq!(
        api.definition_kind_code(iron),
        Some(SimCoreApi::KIND_MATERIAL)
    );
}

#[test]
fn residues_through_the_facade() {
    let mut reg = EntityRegistry::new();
    let actor = reg.spawn_handle();
    let mut api = SimCoreApi::new();
    let sub = api.register_substance("substance-x", 1, &[], &[]).unwrap();
    let on_actor = api.residue_location_entity(actor);
    let on_surface = api.residue_location_symbol(7);
    let q = api.quantity(SimCoreApi::UNIT_VOLUME, 5).unwrap();
    let r1 = api.create_residue(sub, q, on_actor, 0, None, 0);
    let _r2 = api.create_residue(sub, q, on_surface, 0, None, 0);
    assert_eq!(api.residue(r1).unwrap().quantity(), q);
    assert_eq!(api.residue(r1).unwrap().definition(), sub);
    assert_eq!(api.residues_by_location(on_actor), vec![r1]);
    assert_eq!(api.residues_by_definition(sub).len(), 2);
    assert_eq!(api.all_residue_ids().len(), 2);
    assert_eq!(api.residue_count(), 2);
    assert!(api.remove_residue(r1));
    assert!(!api.remove_residue(r1));
}

#[test]
fn interactions_through_the_facade_validate_routes() {
    let mut reg = EntityRegistry::new();
    let a = reg.spawn_handle();
    let b = reg.spawn_handle();
    let mut api = SimCoreApi::new();
    let touch = 0u8; // InteractionRoute::Touch
    let i = api
        .record_interaction(
            1,
            touch,
            (a, Some(b)),
            (None, None, None, None),
            (0, Some(api.cause_command())),
        )
        .unwrap();
    assert_eq!(api.interaction(i).unwrap().primary(), a);
    assert_eq!(api.interactions_by_subject(a), vec![i]);
    assert_eq!(api.interactions_by_route(touch), vec![i]);
    assert_eq!(api.interaction_count(), 1);
    assert_eq!(api.all_interaction_ids(), vec![i]);
    // An out-of-range route code fails cleanly.
    assert!(api
        .record_interaction(1, 250, (a, None), (None, None, None, None), (0, None))
        .is_none());
    assert!(api.interactions_by_route(250).is_empty());
}

#[test]
fn transfer_rules_through_the_facade() {
    let mut api = SimCoreApi::new();
    let touch = 0u8;
    let fixed = api.register_transfer_fixed(4, touch, false).unwrap();
    let _pct = api
        .register_transfer_percentage(2500, touch, false)
        .unwrap();
    let _all = api.register_transfer_all_up_to(9, touch, true).unwrap();
    let _none = api.register_transfer_none(touch, false).unwrap();
    // Invalid percentage / invalid route fail cleanly.
    assert!(api
        .register_transfer_percentage(20_000, touch, false)
        .is_none());
    assert!(api.register_transfer_fixed(1, 250, false).is_none());
    assert_eq!(api.transfer_rule_count(), 4);
    assert_eq!(
        api.transfer_rule(fixed).unwrap().mode(),
        TransferMode::fixed(4)
    );
    assert_eq!(api.transfer_rules_by_route(touch).len(), 4);
    assert_eq!(api.all_transfer_rule_ids().len(), 4);
    assert!(api.remove_transfer_rule(fixed));
    assert_eq!(api.transfer_rule_count(), 3);
}

#[test]
fn material_effect_rules_through_the_facade() {
    let mut api = SimCoreApi::new();
    let touch = 0u8;
    let rule = api
        .register_material_effect_rule(
            (
                Some(SimCoreApi::TAG_CONTACT_TRANSFERABLE),
                touch,
                SimCoreApi::EFFECT_ADD_FACT,
            ),
            (1, Some(api.value_unsigned(1)), 0, 0),
            (0, 0),
        )
        .unwrap();
    assert_eq!(api.material_effect_rule_count(), 1);
    assert!(api.material_effect_rule(rule).is_some());
    assert_eq!(api.all_material_effect_rule_ids(), vec![rule]);
    // Invalid route / effect-kind code fail cleanly.
    assert!(api
        .register_material_effect_rule(
            (None, 250, SimCoreApi::EFFECT_ADD_FACT),
            (1, None, 0, 0),
            (0, 0)
        )
        .is_none());
    assert!(api
        .register_material_effect_rule((None, touch, 250), (1, None, 0, 0), (0, 0))
        .is_none());
}
