//! Body/anatomy facade tests (child of `facade::anatomy`, sees its private items).

use crate::SimCoreApi;
use axiom_ecs::{EntityHandle, EntityRegistry};

/// Build an API with a tissue, a core+extremity+mouth plan, and an instantiated
/// body owned by a freshly spawned entity. Returns `(api, registry, body, owner)`.
fn built() -> (SimCoreApi, EntityRegistry, crate::ids::BodyId, EntityHandle) {
    let mut ereg = EntityRegistry::new();
    let owner = ereg.spawn_handle();
    let mut api = SimCoreApi::new();
    let covering = api
        .register_tissue(
            SimCoreApi::TISSUE_COVERING,
            "test-covering",
            &[SimCoreApi::TTAG_CAN_HOLD_RESIDUE],
            &[(1, 5)],
        )
        .unwrap();
    let draft = api.begin_body_plan();
    // core: one outer surface, one covering layer.
    api.add_body_plan_part(
        draft,
        "test-core",
        (SimCoreApi::PART_CORE, 0, 0),
        &[],
        &[(covering, 0)],
        &[(SimCoreApi::SURFACE_OUTER, true)],
    )
    .unwrap();
    // extremity: bilateral, grouped, a capability + a covering layer, outer surface.
    api.add_body_plan_part(
        draft,
        "test-extremity",
        (SimCoreApi::PART_EXTREMITY, 1, 1), // bilateral, group
        &[10],
        &[(covering, 0)],
        &[(SimCoreApi::SURFACE_OUTER, true)],
    )
    .unwrap();
    // mouth: one mouth surface.
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
    api.connect_body_plan_parts(draft, 0, 2);
    let plan = api.finish_body_plan(draft, "test-plan").unwrap();
    let body = api
        .instantiate_body(plan, Some(owner), &ereg, Some(api.cause_command()), 0)
        .unwrap();
    (api, ereg, body, owner)
}

#[test]
fn empty_api_has_no_bodies() {
    let api = SimCoreApi::new();
    assert_eq!(api.body_count(), 0);
    assert_eq!(api.tissue_count(), 0);
    assert_eq!(api.body_plan_count(), 0);
    assert_eq!(api.wound_count(), 0);
}

#[test]
fn tissues_through_the_facade() {
    let mut api = SimCoreApi::new();
    let id = api
        .register_tissue(
            SimCoreApi::TISSUE_MUSCLE,
            "test-muscle",
            &[SimCoreApi::TTAG_STRUCTURAL, SimCoreApi::TTAG_PAIN_SENSITIVE],
            &[(1, 7), (2, 3)],
        )
        .unwrap();
    // Duplicate name and out-of-range kind code rejected.
    assert!(api
        .register_tissue(SimCoreApi::TISSUE_BONE, "test-muscle", &[], &[])
        .is_none());
    assert!(api.register_tissue(250, "test-other", &[], &[]).is_none());
    assert_eq!(api.tissue_count(), 1);
    assert_eq!(api.tissue_id("test-muscle"), Some(id));
    assert_eq!(api.tissue_kind_code(id), Some(SimCoreApi::TISSUE_MUSCLE));
    assert!(api.tissue_has_tag(id, SimCoreApi::TTAG_STRUCTURAL));
    assert!(!api.tissue_has_tag(id, SimCoreApi::TTAG_VITAL));
    assert_eq!(api.tissue_property(id, 1), Some(7));
    assert_eq!(api.tissue_property(id, 99), None);
    assert_eq!(api.tissues_by_tag(SimCoreApi::TTAG_STRUCTURAL), vec![id]);
    assert_eq!(api.tissues_by_tag(SimCoreApi::TTAG_BLEEDS).len(), 0);
    assert_eq!(api.tissues_by_property(1, 7), vec![id]);
    assert_eq!(api.tissues_by_property(1, 99).len(), 0);
    assert_eq!(api.all_tissue_ids(), vec![id]);
    assert_eq!(
        api.tissue_definition_by_name("test-muscle").unwrap().id(),
        id
    );
    assert!(api.tissue_definition_by_name("absent").is_none());
    assert_eq!(
        api.tissue_definition(id).unwrap().kind().code(),
        SimCoreApi::TISSUE_MUSCLE
    );
    assert!(api
        .tissue_definition(crate::ids::TissueId::from_raw(0))
        .is_none());
}

#[test]
fn body_plan_building_and_queries() {
    let mut api = SimCoreApi::new();
    let draft = api.begin_body_plan();
    let core = api
        .add_body_plan_part(draft, "c", (SimCoreApi::PART_CORE, 0, 0), &[], &[], &[])
        .unwrap();
    let limb = api
        .add_body_plan_part(draft, "l", (SimCoreApi::PART_LIMB, 1, 2), &[5, 6], &[], &[])
        .unwrap();
    assert_eq!((core, limb), (0, 1));
    // Invalid arms: duplicate name, bad kind code, bad symmetry code, bad surface code, unknown draft.
    assert!(api
        .add_body_plan_part(draft, "c", (SimCoreApi::PART_HEAD, 0, 0), &[], &[], &[])
        .is_none());
    assert!(api
        .add_body_plan_part(draft, "x", (250, 0, 0), &[], &[], &[])
        .is_none());
    assert!(api
        .add_body_plan_part(draft, "y", (SimCoreApi::PART_CORE, 250, 0), &[], &[], &[])
        .is_none());
    assert!(api
        .add_body_plan_part(
            draft,
            "z",
            (SimCoreApi::PART_CORE, 0, 0),
            &[],
            &[],
            &[(250, true)]
        )
        .is_none());
    assert!(api
        .add_body_plan_part(9999, "w", (SimCoreApi::PART_CORE, 0, 0), &[], &[], &[])
        .is_none());
    assert!(api.connect_body_plan_parts(draft, core, limb));
    assert!(!api.connect_body_plan_parts(draft, core, 99));
    let plan = api.finish_body_plan(draft, "p").unwrap();
    // finish of a consumed/unknown draft fails.
    assert!(api.finish_body_plan(draft, "p2").is_none());
    assert_eq!(api.body_plan_id("p"), Some(plan));
    assert_eq!(api.body_plan_part_count(plan), Some(2));
    assert_eq!(
        api.body_plan_parts_by_kind(plan, SimCoreApi::PART_LIMB),
        vec![1]
    );
    assert_eq!(api.body_plan_parts_by_kind(plan, 250).len(), 0);
    assert_eq!(api.body_plan_parts_by_capability(plan, 5), vec![1]);
    assert_eq!(api.body_plan_parts_by_capability(plan, 99).len(), 0);
    assert_eq!(api.body_plan_count(), 1);
    assert_eq!(api.all_body_plan_ids(), vec![plan]);
    assert_eq!(api.body_plan(plan).unwrap().parts().len(), 2);
    // unknown plan queries are empty/None.
    let absent = crate::ids::BodyPlanId::from_raw(0);
    assert!(api.body_plan_part_count(absent).is_none());
    assert!(api
        .body_plan_parts_by_kind(absent, SimCoreApi::PART_CORE)
        .is_empty());
    assert!(api.body_plan_parts_by_capability(absent, 5).is_empty());
}

#[test]
fn body_instantiation_and_part_queries() {
    let (mut api, ereg, body, owner) = built();
    assert_eq!(api.body_count(), 1);
    assert_eq!(api.all_body_ids(), vec![body]);
    assert_eq!(api.body_owner(body), Some(owner));
    assert_eq!(api.body_by_owner(owner), Some(body));
    assert!(api.body_plan_of(body).is_some());
    assert_eq!(api.body(body).unwrap().id(), body);

    let parts = api.body_parts(body);
    assert_eq!(parts.len(), 3);
    let cores = api.body_parts_by_kind(body, SimCoreApi::PART_CORE);
    assert_eq!(cores.len(), 1);
    assert_eq!(api.body_parts_by_kind(body, 250).len(), 0);
    assert_eq!(
        api.body_part_kind_code(cores[0]),
        Some(SimCoreApi::PART_CORE)
    );
    // core connects to extremity and mouth.
    assert_eq!(api.connected_parts(body, cores[0]).len(), 2);
    assert!(api.body_part(cores[0]).is_some());
    // part state get/set.
    assert_eq!(api.body_part_state(cores[0]), Some(0));
    assert!(api.set_body_part_state(cores[0], 4));
    assert_eq!(api.body_part_state(cores[0]), Some(4));
    assert!(!api.set_body_part_state(crate::ids::BodyPartId::from_raw(9999), 1));

    // Stale owner is rejected at instantiation.
    let mut e2 = EntityRegistry::new();
    let dead = e2.spawn_handle();
    e2.despawn_handle(dead);
    let plan = api.body_plan_id("test-plan").unwrap();
    assert!(api
        .instantiate_body(plan, Some(dead), &e2, None, 0)
        .is_none());
    // Unknown plan rejected.
    assert!(api
        .instantiate_body(crate::ids::BodyPlanId::from_raw(0), None, &ereg, None, 0)
        .is_none());
}

#[test]
fn surfaces_and_residues_on_them() {
    let (mut api, _ereg, body, _owner) = built();
    let substance = api
        .register_substance("substance-x", 1, &[SimCoreApi::TAG_RESIDUE_CAPABLE], &[])
        .unwrap();

    let surfaces = api.body_surfaces(body);
    assert_eq!(surfaces.len(), 3); // core + extremity + mouth each have one
    let outer = api.body_surfaces_by_kind(body, SimCoreApi::SURFACE_OUTER);
    assert_eq!(outer.len(), 2);
    assert_eq!(
        api.body_surfaces_by_kind(body, SimCoreApi::SURFACE_MOUTH)
            .len(),
        1
    );
    assert_eq!(api.body_surfaces_by_kind(body, 250).len(), 0);
    let surface = outer[0];
    assert_eq!(
        api.surface_kind_code(surface),
        Some(SimCoreApi::SURFACE_OUTER)
    );
    let part = api.surface_part(surface).unwrap();
    assert_eq!(api.part_surfaces(part), vec![surface]);
    assert!(api.body_surface(surface).is_some());
    // surface state get/set.
    assert_eq!(api.surface_state(surface), Some(0));
    assert!(api.set_surface_state(surface, 2));
    assert_eq!(api.surface_state(surface), Some(2));
    assert!(!api.set_surface_state(crate::ids::BodySurfaceId::from_raw(9999), 1));

    // Residue on a surface, queryable by surface/part/body.
    let q = api.quantity(SimCoreApi::UNIT_VOLUME, 5).unwrap();
    let residue = api
        .create_residue_on_surface(substance, q, surface, 0, None, 0)
        .unwrap();
    assert_eq!(api.residues_on_surface(surface), vec![residue]);
    assert_eq!(api.residues_on_part(part), vec![residue]);
    assert!(api.residues_on_body(body).contains(&residue));
    // residue location bridge round-trips.
    let loc = api.residue_location_for_surface(surface);
    assert_eq!(loc.as_symbol(), Some(surface.raw()));
    // creating on an unknown surface fails cleanly.
    assert!(api
        .create_residue_on_surface(
            substance,
            q,
            crate::ids::BodySurfaceId::from_raw(9999),
            0,
            None,
            0
        )
        .is_none());
}

#[test]
fn body_routes_validate_and_record() {
    let (mut api, mut ereg, body, _owner) = built();
    let actor = ereg.spawn_handle();
    let outer = api.body_surfaces_by_kind(body, SimCoreApi::SURFACE_OUTER)[0];
    let mouth = api.body_surfaces_by_kind(body, SimCoreApi::SURFACE_MOUTH)[0];

    // Route mapping + targeting rules.
    assert_eq!(
        api.body_route_from_interaction(0),
        Some(SimCoreApi::BODY_ROUTE_SURFACE_CONTACT)
    );
    assert_eq!(api.body_route_from_interaction(250), None);
    assert_eq!(
        api.body_route_can_target(
            SimCoreApi::BODY_ROUTE_SURFACE_CONTACT,
            SimCoreApi::SURFACE_OUTER
        ),
        Some(true)
    );
    assert_eq!(
        api.body_route_can_target(
            SimCoreApi::BODY_ROUTE_SURFACE_CONTACT,
            SimCoreApi::SURFACE_MOUTH
        ),
        Some(false)
    );
    assert_eq!(
        api.body_route_can_target(250, SimCoreApi::SURFACE_OUTER),
        None
    );

    // Touch (route 0 -> surface-contact) can target an outer surface.
    let before = api.causal_event_count();
    let interaction = api
        .record_surface_interaction(
            (1, 0),
            (actor, outer),
            (None, None, None),
            (1, 0xAB, 3, Some(api.cause_command())),
        )
        .unwrap();
    assert!(api.interaction(interaction).is_some());
    assert_eq!(
        api.causal_event_count(),
        before + 1,
        "routed interaction emits a causal event"
    );
    // Touch cannot target the mouth surface (surface-contact !-> mouth).
    assert!(api
        .record_surface_interaction((1, 0), (actor, mouth), (None, None, None), (1, 0, 3, None))
        .is_none());
    // Ingestion (route 1 -> ingestion-entry) can target the mouth surface.
    assert!(api
        .record_surface_interaction((1, 1), (actor, mouth), (None, None, None), (1, 0, 3, None))
        .is_some());
    // Invalid interaction-route code fails cleanly.
    assert!(api
        .record_surface_interaction(
            (1, 250),
            (actor, outer),
            (None, None, None),
            (1, 0, 3, None)
        )
        .is_none());
    // Unknown surface fails cleanly.
    assert!(api
        .record_surface_interaction(
            (1, 0),
            (actor, crate::ids::BodySurfaceId::from_raw(9999)),
            (None, None, None),
            (1, 0, 3, None)
        )
        .is_none());
}

#[test]
fn wounds_through_the_facade() {
    let (mut api, _ereg, body, _owner) = built();
    let part = api.body_parts_by_kind(body, SimCoreApi::PART_EXTREMITY)[0];
    let tissue = api.tissue_id("test-covering").unwrap();
    let before = api.causal_event_count();
    let wound = api
        .create_wound(
            (body, part, Some(tissue)),
            (SimCoreApi::DAMAGE_CUT, 5),
            (None, None),
            &[(tissue, 5)],
            (1, 0xCC, 7, Some(api.cause_command())),
        )
        .unwrap();
    assert_eq!(
        api.causal_event_count(),
        before + 1,
        "wound creation emits a causal event"
    );
    assert_eq!(api.wound_count(), 1);
    assert!(api.wound(wound).is_some());
    assert_eq!(api.wounds_by_body(body), vec![wound]);
    assert_eq!(api.wounds_by_part(part), vec![wound]);
    assert_eq!(api.wounds_by_mode(SimCoreApi::DAMAGE_CUT), vec![wound]);
    assert_eq!(api.wounds_by_mode(SimCoreApi::DAMAGE_BURN).len(), 0);
    assert_eq!(api.wounds_by_mode(250).len(), 0);
    assert_eq!(api.wounds_by_severity(5), vec![wound]);
    assert_eq!(api.wounds_by_severity(9).len(), 0);
    assert_eq!(api.all_wound_ids(), vec![wound]);
    assert!(api.set_wound_state(wound, 1));
    assert!(!api.set_wound_state(crate::ids::WoundId::from_raw(9999), 1));

    // Invalid references fail cleanly.
    assert!(api
        .create_wound(
            (crate::ids::BodyId::from_raw(0), part, None),
            (SimCoreApi::DAMAGE_CUT, 1),
            (None, None),
            &[],
            (1, 0, 0, None)
        )
        .is_none());
    assert!(api
        .create_wound(
            (body, crate::ids::BodyPartId::from_raw(0), None),
            (SimCoreApi::DAMAGE_CUT, 1),
            (None, None),
            &[],
            (1, 0, 0, None)
        )
        .is_none());
    assert!(api
        .create_wound(
            (body, part, Some(crate::ids::TissueId::from_raw(0))),
            (SimCoreApi::DAMAGE_CUT, 1),
            (None, None),
            &[],
            (1, 0, 0, None)
        )
        .is_none());
    assert!(api
        .create_wound(
            (body, part, None),
            (250, 1),
            (None, None),
            &[],
            (1, 0, 0, None)
        )
        .is_none());
}

#[test]
fn connection_relations_are_recorded_and_queryable() {
    let (mut api, _ereg, body, _owner) = built();
    let recorded = api.record_body_connections_as_relations(body, 1);
    assert_eq!(recorded, 2, "core->extremity and core->mouth");
    let core = api.body_parts_by_kind(body, SimCoreApi::PART_CORE)[0];
    // The core part appears as an endpoint in both connection relations.
    assert_eq!(api.connection_relations_for_part(core).len(), 2);
    // An unknown body records nothing.
    assert_eq!(
        api.record_body_connections_as_relations(crate::ids::BodyId::from_raw(0), 1),
        0
    );
}

#[test]
fn body_substrate_is_deterministic() {
    let snapshot = || {
        let (api, _ereg, body, _owner) = built();
        (
            api.body_parts(body)
                .iter()
                .map(|p| p.raw())
                .collect::<Vec<_>>(),
            api.body_surfaces(body)
                .iter()
                .map(|s| s.raw())
                .collect::<Vec<_>>(),
            api.all_tissue_ids()
                .iter()
                .map(|t| t.raw())
                .collect::<Vec<_>>(),
        )
    };
    assert_eq!(snapshot(), snapshot());
}
