//! End-to-end integration tests driving sim-core only through `SimCoreApi`,
//! with a tiny real ECS registry for entity references.

use axiom_ecs::EntityRegistry;
use axiom_sim_core::{InteractionId, SimCoreApi};

const FACT_KIND: u32 = 1;
const PROC_KIND: u32 = 1;
const EVENT_KIND: u32 = 1;
const EVENT_CODE: u64 = 0xC0DE;

/// Run the generic chain on a fresh world and return a comparable snapshot of the
/// resulting state. The chain:
///   entity A has fact X -> process P wakes -> P emits effect Y ->
///   effect Y updates fact X -> a causal event records that P caused Y.
fn run_chain() -> (Vec<u64>, Vec<u64>, u64) {
    let mut reg = EntityRegistry::new();
    let a = reg.spawn_handle();
    let mut api = SimCoreApi::new();

    let x = api
        .add_fact(&reg, FACT_KIND, a, api.value_unsigned(0), None, 0)
        .unwrap();
    let p = api
        .schedule_process(&reg, PROC_KIND, a, 0, 1, None)
        .unwrap();

    assert!(api.wake_due(0).is_empty());
    assert_eq!(api.wake_due(1), vec![p], "process P wakes at tick 1");

    let mut batch = api.new_effect_batch();
    batch.update_fact(x, api.value_unsigned(1), 1);
    batch.emit_causal_event(
        EVENT_KIND,
        1,
        (Some(a), None),
        Some(api.cause_process(p)),
        EVENT_CODE,
        Some(api.value_unsigned(1)),
    );
    let report = api.apply_effects(batch, &reg);
    assert_eq!(report.len(), 2, "both effects were applied at the boundary");

    assert_eq!(api.fact(x).unwrap().value(), api.value_unsigned(1));
    let caused = api.events_by_parent(api.cause_process(p));
    assert_eq!(caused.len(), 1, "exactly one event was caused by P");
    assert_eq!(api.causal_event(caused[0]).unwrap().subject(), Some(a));

    let fact_ids: Vec<u64> = api.all_fact_ids().into_iter().map(|f| f.raw()).collect();
    let event_ids: Vec<u64> = api
        .all_causal_event_ids()
        .into_iter()
        .map(|e| e.raw())
        .collect();
    let x_is_one = (api.fact(x).unwrap().value() == api.value_unsigned(1)) as u64;
    (fact_ids, event_ids, x_is_one)
}

#[test]
fn generic_chain_fact_process_effect_cause() {
    let (facts, events, x_is_one) = run_chain();
    assert_eq!(facts.len(), 1);
    assert_eq!(events.len(), 1);
    assert_eq!(x_is_one, 1);
}

#[test]
fn same_sequence_produces_identical_state() {
    assert_eq!(
        run_chain(),
        run_chain(),
        "same initial state + same effect/process sequence => identical final state"
    );
}

#[test]
fn effects_apply_only_at_the_boundary() {
    let mut reg = EntityRegistry::new();
    let a = reg.spawn_handle();
    let mut api = SimCoreApi::new();
    let x = api
        .add_fact(&reg, FACT_KIND, a, api.value_unsigned(0), None, 0)
        .unwrap();
    let mut batch = api.new_effect_batch();
    batch.update_fact(x, api.value_unsigned(7), 1);
    assert_eq!(api.fact(x).unwrap().value(), api.value_unsigned(0));
    api.apply_effects(batch, &reg);
    assert_eq!(api.fact(x).unwrap().value(), api.value_unsigned(7));
}

#[test]
fn stale_entity_references_are_rejected_through_the_facade() {
    let mut reg = EntityRegistry::new();
    let a = reg.spawn_handle();
    reg.despawn_handle(a);
    let mut api = SimCoreApi::new();
    assert!(api
        .add_fact(&reg, FACT_KIND, a, api.value_bool(true), None, 0)
        .is_none());
    assert!(api
        .schedule_process(&reg, PROC_KIND, a, 0, 1, None)
        .is_none());
    assert!(api
        .add_relation(&reg, 1, vec![api.endpoint_entity(a)], None, None)
        .is_none());
    assert!(api.is_empty(), "no stale-referencing state was created");
}

const SUBSTANCE_KIND: u32 = 1;
const FACT_X_KIND: u32 = 100;
const TRANSFER_EVENT_KIND: u32 = 200;
const EFFECT_EVENT_KIND: u32 = 201;
const ROUTE_TOUCH: u8 = 0;

/// Run the abstract material chain and return a comparable snapshot of the final
/// state. No domain concepts — only `actor-a`, `surface-a`, `substance-x`, etc.
fn run_material_chain() -> (Vec<u64>, Vec<i64>, Vec<u64>, u64) {
    let mut reg = EntityRegistry::new();
    let actor_a = reg.spawn_handle();
    let mut api = SimCoreApi::new();

    let substance_x = api
        .register_substance(
            "substance-x",
            SUBSTANCE_KIND,
            &[SimCoreApi::TAG_CONTACT_TRANSFERABLE],
            &[],
        )
        .unwrap();
    let fact_x = api
        .add_fact(&reg, FACT_X_KIND, actor_a, api.value_unsigned(0), None, 0)
        .unwrap();

    let surface_a = api.residue_location_symbol(1);
    let target_location = api.residue_location_symbol(2);
    let ten = api.quantity(SimCoreApi::UNIT_VOLUME, 10).unwrap();
    let residue_x = api.create_residue(substance_x, ten, surface_a, 0, None, 0);

    let interaction = api
        .record_interaction(
            1,
            ROUTE_TOUCH,
            (actor_a, None),
            (
                Some(substance_x),
                Some(residue_x),
                None,
                Some(target_location),
            ),
            (1, Some(api.cause_command())),
        )
        .unwrap();

    let rule = api.register_transfer_fixed(4, ROUTE_TOUCH, false).unwrap();
    let transfer = api
        .apply_transfer(
            rule,
            interaction,
            target_location,
            TRANSFER_EVENT_KIND,
            0xABC,
            1,
        )
        .unwrap();

    assert_eq!(
        transfer.moved(),
        Some(api.quantity(SimCoreApi::UNIT_VOLUME, 4).unwrap())
    );
    assert_eq!(
        api.residue(residue_x).unwrap().quantity(),
        api.quantity(SimCoreApi::UNIT_VOLUME, 6).unwrap()
    );
    let deposited: i64 = api
        .residues_by_location(target_location)
        .into_iter()
        .filter_map(|id| api.residue(id))
        .map(|residue| residue.quantity().amount())
        .sum();
    assert_eq!(deposited, 4);

    let transfer_event = api.all_causal_event_ids()[0];

    api.register_material_effect_rule(
        (
            Some(SimCoreApi::TAG_CONTACT_TRANSFERABLE),
            ROUTE_TOUCH,
            SimCoreApi::EFFECT_UPDATE_FACT,
        ),
        (0, Some(api.value_unsigned(1)), 0, 0),
        (0, 0),
    )
    .unwrap();
    api.register_material_effect_rule(
        (
            Some(SimCoreApi::TAG_CONTACT_TRANSFERABLE),
            ROUTE_TOUCH,
            SimCoreApi::EFFECT_EMIT_CAUSAL_EVENT,
        ),
        (EFFECT_EVENT_KIND, None, 0, 0x77),
        (0, 0),
    )
    .unwrap();

    let preview = api
        .produce_material_effects(
            interaction,
            Some(fact_x),
            Some(api.cause_event(transfer_event)),
        )
        .unwrap();
    assert_eq!(preview.len(), 2, "both matching rules produced an effect");
    assert_eq!(
        api.fact_value(fact_x),
        Some(api.value_unsigned(0)),
        "producing effects does not apply them"
    );
    assert!(api
        .produce_material_effects(InteractionId::from_raw(9999), None, None)
        .is_none());

    let cause = api.cause_event(transfer_event);
    let outcome = api
        .apply_material_effects(interaction, Some(fact_x), Some(cause), &reg)
        .unwrap();
    assert_eq!(outcome.matched(), 2, "both rules matched the tag + route");
    assert_eq!(outcome.applied(), 2);

    assert_eq!(api.fact_value(fact_x), Some(api.value_unsigned(1)));

    let effect_children = api.events_by_parent(cause);
    assert_eq!(
        effect_children.len(),
        1,
        "the emitted effect event is a child of the transfer event"
    );

    let fact_ids: Vec<u64> = api.all_fact_ids().into_iter().map(|f| f.raw()).collect();
    let residue_amounts: Vec<i64> = api
        .all_residue_ids()
        .into_iter()
        .filter_map(|id| api.residue(id))
        .map(|residue| residue.quantity().amount())
        .collect();
    let event_ids: Vec<u64> = api
        .all_causal_event_ids()
        .into_iter()
        .map(|e| e.raw())
        .collect();
    let fact_x_is_one = (api.fact_value(fact_x) == Some(api.value_unsigned(1))) as u64;
    (fact_ids, residue_amounts, event_ids, fact_x_is_one)
}

#[test]
fn generic_material_chain_runs_end_to_end() {
    let (facts, residue_amounts, events, fact_x_is_one) = run_material_chain();
    assert_eq!(facts.len(), 1);
    assert_eq!(
        residue_amounts,
        vec![6, 4],
        "source residue 6, target residue 4"
    );
    assert_eq!(events.len(), 2, "transfer event + effect event");
    assert_eq!(fact_x_is_one, 1);
}

#[test]
fn material_chain_is_deterministic() {
    assert_eq!(
        run_material_chain(),
        run_material_chain(),
        "same initial state + same operations => identical final state"
    );
}

#[test]
fn ecs_entity_references_are_deterministic_across_runs() {
    let build = || {
        let mut reg = EntityRegistry::new();
        let a = reg.spawn_handle();
        let mut api = SimCoreApi::new();
        let f = api
            .add_fact(&reg, FACT_KIND, a, api.value_entity(a), None, 0)
            .unwrap();
        api.fact(f).unwrap().value() == api.value_entity(a)
    };
    assert!(build());
    assert!(build());
}

const SURFACE_A_LOCATION: u64 = 9000;

/// Run the abstract body-interaction chain and return a comparable snapshot:
/// `(source_remaining, surface_deposited, wound_count, causal_event_count)`.
fn run_body_chain() -> (i64, i64, usize, usize) {
    let mut ereg = EntityRegistry::new();
    let actor_a = ereg.spawn_handle();
    let mut api = SimCoreApi::new();

    let substance_x = api
        .register_substance(
            "substance-x",
            1,
            &[SimCoreApi::TAG_CONTACT_TRANSFERABLE],
            &[],
        )
        .unwrap();
    let covering = api
        .register_tissue(
            SimCoreApi::TISSUE_COVERING,
            "test-covering",
            &[SimCoreApi::TTAG_CAN_HOLD_RESIDUE],
            &[],
        )
        .unwrap();

    let draft = api.begin_body_plan();
    api.add_body_plan_part(
        draft,
        "test-core",
        (SimCoreApi::PART_CORE, 0, 0),
        &[],
        &[(covering, 0)],
        &[(SimCoreApi::SURFACE_OUTER, true)],
    )
    .unwrap();
    api.add_body_plan_part(
        draft,
        "test-extremity",
        (SimCoreApi::PART_EXTREMITY, 0, 0),
        &[],
        &[(covering, 0)],
        &[(SimCoreApi::SURFACE_OUTER, true)],
    )
    .unwrap();
    api.add_body_plan_part(
        draft,
        "test-mouth",
        (SimCoreApi::PART_MOUTH, 0, 0),
        &[],
        &[],
        &[(SimCoreApi::SURFACE_MOUTH, true)],
    )
    .unwrap();
    api.connect_body_plan_parts(draft, 0, 1);
    let plan = api.finish_body_plan(draft, "test-body").unwrap();

    let body = api
        .instantiate_body(plan, Some(actor_a), &ereg, Some(api.cause_command()), 0)
        .unwrap();
    let extremity = api.body_parts_by_kind(body, SimCoreApi::PART_EXTREMITY)[0];
    let surface = api.part_surfaces(extremity)[0];

    let source_location = api.residue_location_symbol(SURFACE_A_LOCATION);
    let ten = api.quantity(SimCoreApi::UNIT_VOLUME, 10).unwrap();
    let source = api.create_residue(substance_x, ten, source_location, 0, None, 0);

    let interaction = api
        .record_surface_interaction(
            (1, ROUTE_TOUCH),
            (actor_a, surface),
            (Some(substance_x), Some(source), None),
            (10, 0xB0, 1, Some(api.cause_command())),
        )
        .unwrap();

    let target = api.residue_location_for_surface(surface);
    let rule = api.register_transfer_fixed(4, ROUTE_TOUCH, false).unwrap();
    let result = api
        .apply_transfer(rule, interaction, target, 20, 0xB1, 1)
        .unwrap();
    assert_eq!(
        result.moved(),
        Some(api.quantity(SimCoreApi::UNIT_VOLUME, 4).unwrap())
    );

    let on_surface = api.residues_on_surface(surface);
    assert_eq!(
        on_surface.len(),
        1,
        "a residue now sits on the target body surface"
    );
    assert!(api.residues_on_part(extremity).contains(&on_surface[0]));
    assert!(api.residues_on_body(body).contains(&on_surface[0]));
    let surface_deposited = api.residue(on_surface[0]).unwrap().quantity().amount();
    let source_remaining = api.residue(source).unwrap().quantity().amount();
    assert_eq!((source_remaining, surface_deposited), (6, 4));

    let chained = api.events_by_parent(api.cause_command());
    assert!(
        chained.len() >= 2,
        "interaction + transfer events recorded under the cause"
    );

    let wound = api
        .create_wound(
            (body, extremity, Some(covering)),
            (SimCoreApi::DAMAGE_CUT, 5),
            (None, None),
            &[(covering, 5)],
            (30, 0xB2, 2, Some(api.cause_command())),
        )
        .unwrap();
    assert_eq!(api.wounds_by_body(body), vec![wound]);
    assert_eq!(api.wounds_by_part(extremity), vec![wound]);
    assert_eq!(api.wounds_by_mode(SimCoreApi::DAMAGE_CUT), vec![wound]);
    assert_eq!(api.wounds_by_severity(5), vec![wound]);

    (
        source_remaining,
        surface_deposited,
        api.wound_count(),
        api.causal_event_count(),
    )
}

#[test]
fn generic_body_interaction_chain_runs_end_to_end() {
    let (source_remaining, surface_deposited, wounds, events) = run_body_chain();
    assert_eq!(source_remaining, 6);
    assert_eq!(surface_deposited, 4);
    assert_eq!(wounds, 1);
    assert!(events >= 3, "interaction + transfer + wound causal events");
}

#[test]
fn body_chain_is_deterministic() {
    assert_eq!(
        run_body_chain(),
        run_body_chain(),
        "same initial state + same operations => identical final state"
    );
}

const FX_KIND: u32 = 50;
const FY_KIND: u32 = 51;
const SCHED_PROC_KIND: u32 = 60;

/// Run the abstract scheduler chain and return a comparable snapshot:
/// `(fact_x, fact_y, status_code, causal_event_count, process_event_count)`.
fn run_scheduler_chain() -> (Option<u64>, Option<u64>, Option<u8>, usize, usize) {
    let mut reg = EntityRegistry::new();
    let subject_a = reg.spawn_handle();
    let mut api = SimCoreApi::new();

    let fact_x = api
        .add_fact(&reg, FX_KIND, subject_a, api.value_unsigned(0), None, 0)
        .unwrap();
    let fact_y = api
        .add_fact(&reg, FY_KIND, subject_a, api.value_unsigned(0), None, 0)
        .unwrap();

    let process_p = api.register_process_updating_fact(
        SCHED_PROC_KIND,
        subject_a,
        fact_y,
        api.value_unsigned(1),
        1,
        0,
    );
    api.subscribe_process(process_p, SimCoreApi::DEP_FACT_KIND, u64::from(FX_KIND));

    let mut batch = api.new_effect_batch();
    batch.update_fact(fact_x, api.value_unsigned(7), 1);
    api.apply_effects(batch, &reg);
    assert_eq!(api.dirty_fact_ids(), vec![fact_x]);

    let woken = api.apply_dirty_invalidations(1, Some(api.cause_command()));
    assert_eq!(woken, 1);

    assert_eq!(api.step_scheduler(1), vec![process_p]);
    assert_eq!(api.fact_value(fact_y), Some(api.value_unsigned(0)));

    api.apply_scheduler_boundary(1, &reg);
    assert_eq!(api.fact_value(fact_y), Some(api.value_unsigned(1)));

    let process_events = api.scheduler_events_for_process(process_p);
    assert!(process_events.len() >= 5);

    (
        match api.fact_value(fact_x) {
            Some(v) if v == api.value_unsigned(7) => Some(7),
            _ => None,
        },
        match api.fact_value(fact_y) {
            Some(v) if v == api.value_unsigned(1) => Some(1),
            _ => None,
        },
        api.process_status_code(process_p),
        api.causal_event_count(),
        process_events.len(),
    )
}

#[test]
fn generic_scheduler_chain_runs_end_to_end() {
    let (fact_x, fact_y, status, events, process_events) = run_scheduler_chain();
    assert_eq!(fact_x, Some(7));
    assert_eq!(fact_y, Some(1));
    assert_eq!(status, Some(SimCoreApi::STATUS_COMPLETED));
    assert!(events >= 6);
    assert!(process_events >= 5);
}

#[test]
fn scheduler_chain_is_deterministic() {
    assert_eq!(
        run_scheduler_chain(),
        run_scheduler_chain(),
        "same initial state + same scheduled processes + same dirty changes => identical final state"
    );
}
