//! The effect-apply dispatch for `SimWorld`: the `APPLY` table indexed by an
//! effect's tag (see `effect.rs`) and the per-effect-kind apply functions.
//!
//! A child module of `sim_world`, so its functions read and mutate the private
//! `SimWorld` fields directly. Kept in its own file for the file-size budget.

use axiom_ecs::EntityRegistry;

use crate::causal::CausalEventKind;
use crate::dirty_set::DirtyKind;
use crate::effect::{Effect, EffectResult};
use crate::fact::FactKind;
use crate::ids::{FactId, ProcessId, RelationId};
use crate::process::{ProcessKind, ProcessState, WakeTick};
use crate::relation::RelationKind;

use super::SimWorld;

/// The apply dispatch table, indexed by an effect's tag (see `effect.rs`).
pub(super) const APPLY: [fn(&mut SimWorld, Effect, &EntityRegistry) -> EffectResult; 8] = [
    apply_add_fact,
    apply_update_fact,
    apply_remove_fact,
    apply_add_relation,
    apply_remove_relation,
    apply_schedule_process,
    apply_cancel_process,
    apply_emit_causal_event,
];

fn apply_add_fact(world: &mut SimWorld, effect: Effect, registry: &EntityRegistry) -> EffectResult {
    effect
        .subject()
        .zip(effect.value())
        .map_or(EffectResult::Failed, |(subject, value)| {
            let live = registry.is_current(subject);
            live.then(|| {
                let id = world.facts.insert(
                    FactKind::new(effect.kind_code()),
                    subject,
                    value,
                    effect.cause(),
                    effect.tick(),
                );
                world
                    .dirty
                    .mark_fact(id, effect.kind_code(), DirtyKind::Added, effect.cause());
                world
                    .dirty
                    .mark_subject(subject, DirtyKind::Added, effect.cause());
            });
            [EffectResult::Skipped, EffectResult::Applied][live as usize]
        })
}

fn apply_update_fact(
    world: &mut SimWorld,
    effect: Effect,
    _registry: &EntityRegistry,
) -> EffectResult {
    effect
        .target_id()
        .zip(effect.value())
        .map_or(EffectResult::Failed, |(raw, value)| {
            let id = FactId::from_raw(raw);
            let updated = world.facts.update(id, value, effect.tick());
            let touched = updated
                .then(|| {
                    world
                        .facts
                        .get(id)
                        .map(|fact| (fact.kind().code(), fact.subject()))
                })
                .flatten();
            touched.into_iter().for_each(|(kind_code, subject)| {
                world
                    .dirty
                    .mark_fact(id, kind_code, DirtyKind::Updated, effect.cause());
                world
                    .dirty
                    .mark_subject(subject, DirtyKind::Updated, effect.cause());
            });
            [EffectResult::Failed, EffectResult::Applied][updated as usize]
        })
}

fn apply_remove_fact(
    world: &mut SimWorld,
    effect: Effect,
    _registry: &EntityRegistry,
) -> EffectResult {
    effect.target_id().map_or(EffectResult::Failed, |raw| {
        let id = FactId::from_raw(raw);
        let removed = world.facts.remove(id);
        let was = removed.is_some();
        removed.into_iter().for_each(|fact| {
            world
                .dirty
                .mark_fact(id, fact.kind().code(), DirtyKind::Removed, fact.cause());
            world
                .dirty
                .mark_subject(fact.subject(), DirtyKind::Removed, fact.cause());
        });
        [EffectResult::Failed, EffectResult::Applied][was as usize]
    })
}

fn apply_add_relation(
    world: &mut SimWorld,
    effect: Effect,
    registry: &EntityRegistry,
) -> EffectResult {
    let endpoints = effect.endpoints().to_vec();
    let live = endpoints.iter().all(|endpoint| {
        endpoint
            .as_entity()
            .is_none_or(|handle| registry.is_current(handle))
    });
    live.then(|| {
        let id = world.relations.insert(
            RelationKind::new(effect.kind_code()),
            endpoints,
            effect.strength(),
            effect.cause(),
        );
        world
            .dirty
            .mark_relation(id, effect.kind_code(), DirtyKind::Added, effect.cause());
    });
    [EffectResult::Skipped, EffectResult::Applied][live as usize]
}

fn apply_remove_relation(
    world: &mut SimWorld,
    effect: Effect,
    _registry: &EntityRegistry,
) -> EffectResult {
    effect.target_id().map_or(EffectResult::Failed, |raw| {
        let id = RelationId::from_raw(raw);
        let removed = world.relations.remove(id);
        let was = removed.is_some();
        removed.into_iter().for_each(|relation| {
            world.dirty.mark_relation(
                id,
                relation.kind().code(),
                DirtyKind::Removed,
                relation.cause(),
            );
        });
        [EffectResult::Failed, EffectResult::Applied][was as usize]
    })
}

fn apply_schedule_process(
    world: &mut SimWorld,
    effect: Effect,
    registry: &EntityRegistry,
) -> EffectResult {
    effect.subject().map_or(EffectResult::Failed, |subject| {
        let live = registry.is_current(subject);
        live.then(|| {
            world.processes.schedule(
                ProcessKind::new(effect.kind_code()),
                subject,
                ProcessState::new(effect.state()),
                WakeTick::new(effect.wake()),
                effect.cause(),
            )
        });
        [EffectResult::Skipped, EffectResult::Applied][live as usize]
    })
}

fn apply_cancel_process(
    world: &mut SimWorld,
    effect: Effect,
    _registry: &EntityRegistry,
) -> EffectResult {
    effect.target_id().map_or(EffectResult::Failed, |raw| {
        let cancelled = world.processes.cancel(ProcessId::from_raw(raw));
        [EffectResult::Failed, EffectResult::Applied][cancelled as usize]
    })
}

fn apply_emit_causal_event(
    world: &mut SimWorld,
    effect: Effect,
    _registry: &EntityRegistry,
) -> EffectResult {
    world.journal.append(
        CausalEventKind::new(effect.kind_code()),
        effect.tick(),
        (effect.subject(), effect.secondary()),
        effect.cause(),
        effect.code(),
        effect.payload(),
    );
    EffectResult::Applied
}
