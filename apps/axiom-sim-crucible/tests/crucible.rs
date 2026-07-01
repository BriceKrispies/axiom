//! Behavioral tests for the Simulation Crucible proof app.
//!
//! Fixed deterministic ticks: contact at 2, grooming at 5, final at 8. Fixed
//! amounts: source 10 → 4 onto the paw → 3 from paw to mouth, leaving source 6,
//! paw 1, mouth 3 (quantity conserved).

use axiom_sim_crucible::action::ScenarioActionKind;
use axiom_sim_crucible::{replay, scenario, Crucible, ParentRef};

fn row_of_kind(
    rows: &[axiom_sim_crucible::CausalRow],
    kind: u32,
) -> Option<axiom_sim_crucible::CausalRow> {
    rows.iter().find(|r| r.kind == kind).cloned()
}

#[test]
fn scenario_initializes_deterministically() {
    let a = Crucible::new();
    let b = Crucible::new();
    for c in [&a, &b] {
        assert_eq!(c.paw_amount(), 0);
        assert_eq!(c.mouth_amount(), 0);
        assert!(!c.intox_active());
        assert!(!c.groomed());
        assert_eq!(c.grooming_woke_tick(), None);
    }
}

#[test]
fn body_plan_and_surfaces_exist() {
    // The captured extremity and mouth surfaces resolve; if the body plan or its
    // surfaces were missing, `scenario::build` would have panicked at setup.
    let crucible = Crucible::new();
    assert_eq!(crucible.paw_amount(), 0);
    assert_eq!(crucible.mouth_amount(), 0);
}

#[test]
fn source_residue_exists_at_initial_location() {
    let crucible = Crucible::new();
    assert_eq!(
        crucible.source_amount(),
        10,
        "the tavern-cell holds the source residue"
    );
}

#[test]
fn scenario_actions_run_at_deterministic_ticks() {
    let crucible = Crucible::new();
    let actions = crucible.actions();
    let ticks: Vec<u64> = actions.iter().map(|a| a.tick).collect();
    assert_eq!(
        ticks,
        vec![
            0,
            scenario::TICK_CONTACT,
            scenario::TICK_GROOM,
            scenario::TICK_GROOM
        ]
    );
    assert!(
        matches!(actions[0].kind, ScenarioActionKind::Process(_)),
        "first action schedules the grooming process"
    );
    assert!(
        matches!(actions[1].kind, ScenarioActionKind::SurfaceTransfer(_)),
        "contact is a surface transfer"
    );
    assert!(
        matches!(actions[2].kind, ScenarioActionKind::SurfaceTransfer(_)),
        "grooming consequence is a surface transfer"
    );
    assert!(
        matches!(actions[3].kind, ScenarioActionKind::EffectApplication(_)),
        "final step applies material effects"
    );
}

#[test]
fn scenario_schedule_is_deterministic() {
    let a = Crucible::new();
    let b = Crucible::new();
    assert_eq!(
        a.actions(),
        b.actions(),
        "the action schedule is identical across builds"
    );
}

#[test]
fn contact_transfers_residue_to_the_extremity_surface() {
    let mut crucible = Crucible::new();
    crucible.run_to(scenario::TICK_CONTACT);
    assert_eq!(crucible.paw_amount(), 4, "4 units moved onto the paw");
    assert_eq!(crucible.mouth_amount(), 0, "nothing on the mouth yet");
    assert_eq!(
        crucible.source_amount(),
        6,
        "source reduced by 4 (conserved)"
    );
    assert_eq!(
        crucible.grooming_woke_tick(),
        None,
        "grooming has not woken yet"
    );
}

#[test]
fn grooming_process_wakes_at_the_expected_tick() {
    let mut crucible = Crucible::new();
    crucible.run_to(scenario::TICK_GROOM - 1);
    assert_eq!(crucible.grooming_woke_tick(), None);
    crucible.run_to(scenario::TICK_GROOM);
    assert_eq!(crucible.grooming_woke_tick(), Some(scenario::TICK_GROOM));
}

#[test]
fn grooming_produces_effects_rather_than_mutating_directly() {
    let mut crucible = Crucible::new();
    crucible.run();
    assert!(
        crucible.groomed(),
        "groomed fact added via the effect boundary"
    );
    let rows = crucible.rows();
    assert!(rows.iter().any(|r| r.label == "process-produced-effects"));
    assert!(rows.iter().any(|r| r.label == "effects-applied"));
}

#[test]
fn residue_transfers_from_extremity_to_mouth() {
    let mut crucible = Crucible::new();
    crucible.run();
    assert_eq!(crucible.mouth_amount(), 3, "3 units groomed onto the mouth");
    assert_eq!(crucible.paw_amount(), 1, "paw reduced from 4 to 1");
    assert_eq!(
        crucible.source_amount() + crucible.paw_amount() + crucible.mouth_amount(),
        10
    );
}

#[test]
fn ingestion_entry_interaction_is_recorded() {
    let mut crucible = Crucible::new();
    crucible.run();
    let ingestion =
        row_of_kind(&crucible.rows(), scenario::KIND_INGESTION).expect("ingestion event");
    assert_eq!(ingestion.route, Some(scenario::ROUTE_INGESTION));
    assert_eq!(ingestion.tick, scenario::TICK_GROOM);
}

#[test]
fn final_creature_fact_changes_through_a_generic_effect_rule() {
    let mut crucible = Crucible::new();
    crucible.run();
    assert!(
        crucible.intox_active(),
        "the intoxication fact was set by the effect rule"
    );
    let effect = row_of_kind(&crucible.rows(), scenario::KIND_INTOX_EFFECT).expect("effect event");
    assert_eq!(effect.substance, Some("beer"));
    assert_eq!(effect.route, Some(scenario::ROUTE_INGESTION));
}

#[test]
fn causal_journal_contains_the_full_parent_child_chain() {
    let mut crucible = Crucible::new();
    crucible.run();
    let rows = crucible.rows();
    let contact = row_of_kind(&rows, scenario::KIND_CONTACT_TRANSFER).expect("contact transfer");
    assert_eq!(contact.parent, ParentRef::Command);
    for kind in [
        scenario::KIND_GROOM_TRANSFER,
        scenario::KIND_INGESTION,
        scenario::KIND_INTOX_EFFECT,
    ] {
        let row = row_of_kind(&rows, kind).expect("grooming-caused event");
        assert!(
            matches!(row.parent, ParentRef::Process(_)),
            "kind {kind} should be process-caused"
        );
    }
    assert!(
        rows.iter().all(|r| r.parent != ParentRef::Unknown),
        "all events attributed"
    );
}

#[test]
fn causal_event_order_is_the_expected_canonical_chain() {
    let mut crucible = Crucible::new();
    crucible.run();
    let labels: Vec<&str> = crucible.rows().iter().map(|r| r.label).collect();
    // The canonical, deterministic causal order. Any intentional change to the
    // chain must update this list — it is the pinned contract of the proof.
    assert_eq!(
        labels,
        vec![
            "process-scheduled",
            "contact-interaction",
            "contact-transfer",
            "process-woke",
            "process-started",
            "process-produced-effects",
            "effects-applied",
            "process-completed",
            "ingestion-interaction",
            "groom-transfer",
            "intoxication-effect",
        ]
    );
}

#[test]
fn repeated_run_produces_identical_digest_and_causal_order() {
    let check = replay::verify();
    assert!(
        check.identical_digest,
        "structural digest is identical across runs"
    );
    assert!(
        check.identical_causal_order,
        "causal event order is identical across runs"
    );
    assert!(
        check.identical_state,
        "important fact/residue state is identical"
    );
    assert!(check.ok());
}

#[test]
fn same_scenario_actions_replay_to_the_same_digest() {
    let mut first = Crucible::new();
    first.run();
    let mut second = Crucible::new();
    second.run();
    assert_eq!(
        first.actions(),
        second.actions(),
        "same scenario actions drive both runs"
    );
    assert_eq!(
        first.digest(),
        second.digest(),
        "same final structural digest"
    );
    assert_eq!(
        first.causal_digest(),
        second.causal_digest(),
        "same causal-chain digest"
    );
}

#[test]
fn app_runs_headlessly_and_reports() {
    let report = axiom_sim_crucible::run_report();
    assert!(report.contains("cat-in-tavern"));
    assert!(report.contains("intoxicated=true"));
    assert!(report.contains("PASS"));
    assert!(report.contains("digest:"));
}
