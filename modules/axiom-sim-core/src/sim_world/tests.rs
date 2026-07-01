//! Behavioral tests for `SimWorld` effect application and material transfer.
//! A child module of `sim_world`, kept in its own file for the file-size budget.

use super::*;
use crate::fact::{FactKind, FactValue};
use crate::relation::RelationEndpoint;

fn batch() -> EffectBatch {
    EffectBatch::new()
}

#[test]
fn add_fact_applies_for_live_and_skips_for_dead_subjects() {
    let mut reg = EntityRegistry::new();
    let live = reg.spawn_handle();
    let dead = reg.spawn_handle();
    reg.despawn_handle(dead);
    let mut world = SimWorld::new();

    let mut b = batch();
    b.add_fact(1, live, FactValue::Unsigned(7), None, 0);
    b.add_fact(1, dead, FactValue::Unsigned(8), None, 0);
    let report = world.apply_effects(b, &reg);
    assert_eq!(report.result(0), Some(EffectResult::Applied));
    assert_eq!(report.result(1), Some(EffectResult::Skipped));
    assert_eq!(
        world.facts().len(),
        1,
        "the dead-subject fact was not added"
    );
}

#[test]
fn update_and_remove_fact_fail_cleanly_for_invalid_ids() {
    let mut reg = EntityRegistry::new();
    let a = reg.spawn_handle();
    let mut world = SimWorld::new();
    let id = world
        .facts_mut()
        .insert(FactKind::new(1), a, FactValue::Unsigned(1), None, 0);

    let mut b = batch();
    b.update_fact(id, FactValue::Unsigned(2), 1);
    b.update_fact(FactId::from_raw(999), FactValue::Unsigned(3), 1);
    b.remove_fact(id);
    b.remove_fact(FactId::from_raw(999));
    let report = world.apply_effects(b, &reg);
    assert_eq!(report.result(0), Some(EffectResult::Applied));
    assert_eq!(report.result(1), Some(EffectResult::Failed));
    assert_eq!(report.result(2), Some(EffectResult::Applied));
    assert_eq!(report.result(3), Some(EffectResult::Failed));
    assert!(world.facts().is_empty());
}

#[test]
fn relation_effects_apply_skip_and_fail() {
    let mut reg = EntityRegistry::new();
    let a = reg.spawn_handle();
    let dead = reg.spawn_handle();
    reg.despawn_handle(dead);
    let mut world = SimWorld::new();

    let mut b = batch();
    b.add_relation(
        1,
        vec![RelationEndpoint::entity(a), RelationEndpoint::symbol(9)],
        None,
        None,
    );
    b.add_relation(1, vec![RelationEndpoint::entity(dead)], None, None);
    let report = world.apply_effects(b, &reg);
    assert_eq!(report.result(0), Some(EffectResult::Applied));
    assert_eq!(report.result(1), Some(EffectResult::Skipped));
    assert_eq!(world.relations().len(), 1);

    let live_id = world.relations().iter().next().unwrap().id();
    let mut b2 = batch();
    b2.remove_relation(live_id);
    b2.remove_relation(RelationId::from_raw(999));
    let report2 = world.apply_effects(b2, &reg);
    assert_eq!(report2.result(0), Some(EffectResult::Applied));
    assert_eq!(report2.result(1), Some(EffectResult::Failed));
}

#[test]
fn process_effects_apply_skip_and_fail() {
    let mut reg = EntityRegistry::new();
    let a = reg.spawn_handle();
    let dead = reg.spawn_handle();
    reg.despawn_handle(dead);
    let mut world = SimWorld::new();

    let mut b = batch();
    b.schedule_process(1, a, 0, 5, None);
    b.schedule_process(1, dead, 0, 5, None);
    let report = world.apply_effects(b, &reg);
    assert_eq!(report.result(0), Some(EffectResult::Applied));
    assert_eq!(report.result(1), Some(EffectResult::Skipped));
    assert_eq!(world.processes().len(), 1);

    let pid = world.processes().iter().next().unwrap().id();
    let mut b2 = batch();
    b2.cancel_process(pid);
    b2.cancel_process(ProcessId::from_raw(999));
    let report2 = world.apply_effects(b2, &reg);
    assert_eq!(report2.result(0), Some(EffectResult::Applied));
    assert_eq!(report2.result(1), Some(EffectResult::Failed));
}

#[test]
fn emit_causal_event_always_applies() {
    let reg = EntityRegistry::new();
    let mut world = SimWorld::new();
    let mut b = batch();
    b.emit_causal_event(1, 3, (None, None), None, 42, None);
    let report = world.apply_effects(b, &reg);
    assert_eq!(report.result(0), Some(EffectResult::Applied));
    assert_eq!(world.journal().len(), 1);
}

#[test]
fn empty_batch_applies_nothing() {
    let reg = EntityRegistry::new();
    let mut world = SimWorld::new();
    let report = world.apply_effects(batch(), &reg);
    assert!(report.is_empty());
}


use crate::definition::{DefinitionKind, PropertySet, TagSet};
use crate::interaction::{InteractionKind, InteractionParams, InteractionRoute};
use crate::transfer::{TransferMode, TransferOutcome};

fn vol(amount: i64) -> Quantity {
    Quantity::new(QuantityUnit::Volume, amount).unwrap()
}

/// Fresh world with substance-x, a source residue of 10 Volume, and a touch
/// interaction referencing it. Returns (world, interaction, source, dst).
fn transfer_setup() -> (SimWorld, InteractionRecord, ResidueId, ResidueLocation) {
    let mut reg = EntityRegistry::new();
    let a = reg.spawn_handle();
    let mut world = SimWorld::new();
    let sub = world
        .definitions_mut()
        .register(
            DefinitionKind::Substance,
            "substance-x",
            TagSet::new(),
            PropertySet::new(),
        )
        .unwrap();
    let src_loc = ResidueLocation::symbol(1);
    let dst = ResidueLocation::symbol(2);
    let source =
        world
            .residues_mut()
            .create(sub, vol(10), src_loc, ResidueState::new(0), None, 0);
    let id = world.interactions_mut().create(InteractionParams {
        kind: InteractionKind::new(1),
        route: InteractionRoute::Touch,
        primary: a,
        secondary: None,
        material: Some(sub),
        residue: Some(source),
        quantity: None,
        location: Some(dst),
        tick: 0,
        cause: Some(CauseRef::Command),
    });
    let interaction = *world.interactions().get(id).unwrap();
    (world, interaction, source, dst)
}

fn rule(
    world: &mut SimWorld,
    mode: TransferMode,
    route: InteractionRoute,
    lossy: bool,
) -> TransferRule {
    let id = world.transfers_mut().register(mode, route, lossy).unwrap();
    *world.transfers().get(id).unwrap()
}

#[test]
fn transfer_applies_and_conserves_quantity() {
    let (mut world, interaction, source, dst) = transfer_setup();
    let r = rule(
        &mut world,
        TransferMode::fixed(4),
        InteractionRoute::Touch,
        false,
    );
    let result = world.apply_transfer(r, &interaction, dst, 1, 0xABC, 5);
    assert_eq!(result.outcome(), TransferOutcome::Applied);
    assert_eq!(result.moved(), Some(vol(4)));
    assert_eq!(world.residues().get(source).unwrap().quantity().amount(), 6);
    let deposited: i64 = world
        .residues()
        .by_location(dst)
        .map(|res| res.quantity().amount())
        .sum();
    assert_eq!(deposited, 4);
    assert_eq!(6 + deposited, 10, "quantity conserved");
    assert_eq!(world.journal().len(), 1, "transfer emitted a causal event");
}

#[test]
fn transfer_into_existing_target_accumulates() {
    let (mut world, interaction, _source, dst) = transfer_setup();
    let r = rule(
        &mut world,
        TransferMode::fixed(3),
        InteractionRoute::Touch,
        false,
    );
    world.apply_transfer(r, &interaction, dst, 1, 0, 1);
    world.apply_transfer(r, &interaction, dst, 1, 0, 2);
    let deposited: i64 = world
        .residues()
        .by_location(dst)
        .map(|res| res.quantity().amount())
        .sum();
    assert_eq!(
        deposited, 6,
        "two fixed-3 transfers accumulate into one target residue"
    );
}

#[test]
fn lossy_transfer_does_not_deposit() {
    let (mut world, interaction, source, dst) = transfer_setup();
    let r = rule(
        &mut world,
        TransferMode::fixed(4),
        InteractionRoute::Touch,
        true,
    );
    let result = world.apply_transfer(r, &interaction, dst, 1, 0, 1);
    assert_eq!(result.outcome(), TransferOutcome::Applied);
    assert_eq!(world.residues().get(source).unwrap().quantity().amount(), 6);
    assert_eq!(
        world.residues().by_location(dst).count(),
        0,
        "lossy transfer destroys the moved amount"
    );
}

#[test]
fn transfer_route_mismatch_fails_cleanly() {
    let (mut world, interaction, source, dst) = transfer_setup();
    let r = rule(
        &mut world,
        TransferMode::fixed(4),
        InteractionRoute::Adjacent,
        false,
    );
    let result = world.apply_transfer(r, &interaction, dst, 1, 0, 1);
    assert_eq!(result.outcome(), TransferOutcome::RouteMismatch);
    assert_eq!(result.moved(), None);
    assert_eq!(
        world.residues().get(source).unwrap().quantity().amount(),
        10,
        "no change on mismatch"
    );
}

#[test]
fn transfer_insufficient_quantity_fails_cleanly() {
    let (mut world, interaction, source, dst) = transfer_setup();
    let r = rule(
        &mut world,
        TransferMode::fixed(99),
        InteractionRoute::Touch,
        false,
    );
    let result = world.apply_transfer(r, &interaction, dst, 1, 0, 1);
    assert_eq!(result.outcome(), TransferOutcome::InsufficientQuantity);
    assert_eq!(
        world.residues().get(source).unwrap().quantity().amount(),
        10
    );
}

#[test]
fn transfer_invalid_source_fails_cleanly() {
    let (mut world, mut interaction, _source, dst) = transfer_setup();
    let bad = world.interactions_mut().create(InteractionParams {
        kind: interaction.kind(),
        route: InteractionRoute::Touch,
        primary: interaction.primary(),
        secondary: None,
        material: interaction.material(),
        residue: Some(ResidueId::from_raw(9999)),
        quantity: None,
        location: Some(dst),
        tick: 0,
        cause: None,
    });
    interaction = *world.interactions().get(bad).unwrap();
    let r = rule(
        &mut world,
        TransferMode::fixed(1),
        InteractionRoute::Touch,
        false,
    );
    assert_eq!(
        world
            .apply_transfer(r, &interaction, dst, 1, 0, 1)
            .outcome(),
        TransferOutcome::InvalidSource
    );
}

#[test]
fn transfer_incompatible_units_fails_cleanly() {
    let (mut world, interaction, _source, dst) = transfer_setup();
    let sub = world.residues().get(_source).unwrap().definition();
    world.residues_mut().create(
        sub,
        Quantity::new(QuantityUnit::Mass, 1).unwrap(),
        dst,
        ResidueState::new(0),
        None,
        0,
    );
    let r = rule(
        &mut world,
        TransferMode::fixed(4),
        InteractionRoute::Touch,
        false,
    );
    assert_eq!(
        world
            .apply_transfer(r, &interaction, dst, 1, 0, 1)
            .outcome(),
        TransferOutcome::IncompatibleUnits
    );
}
