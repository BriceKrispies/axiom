//! The generic effect model: proposed mutations applied at an explicit boundary.

use axiom_ecs::EntityHandle;

use crate::cause::CauseRef;
use crate::fact::FactValue;
use crate::ids::{FactId, ProcessId, RelationId};
use crate::relation::RelationEndpoint;

/// The kind of mutation an [`Effect`] proposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EffectKind {
    /// Add a new fact.
    AddFact,
    /// Update an existing fact's value.
    UpdateFact,
    /// Remove a fact.
    RemoveFact,
    /// Add a new relation.
    AddRelation,
    /// Remove a relation.
    RemoveRelation,
    /// Schedule a new process.
    ScheduleProcess,
    /// Cancel a process.
    CancelProcess,
    /// Emit a causal event.
    EmitCausalEvent,
}

// Tag values — the index of each kind, matching `KINDS` and the apply table in
// `sim_world`. Keep all three in lock-step.
pub(crate) const ADD_FACT: u8 = 0;
pub(crate) const UPDATE_FACT: u8 = 1;
pub(crate) const REMOVE_FACT: u8 = 2;
pub(crate) const ADD_RELATION: u8 = 3;
pub(crate) const REMOVE_RELATION: u8 = 4;
pub(crate) const SCHEDULE_PROCESS: u8 = 5;
pub(crate) const CANCEL_PROCESS: u8 = 6;
pub(crate) const EMIT_CAUSAL_EVENT: u8 = 7;

const KINDS: [EffectKind; 8] = [
    EffectKind::AddFact,
    EffectKind::UpdateFact,
    EffectKind::RemoveFact,
    EffectKind::AddRelation,
    EffectKind::RemoveRelation,
    EffectKind::ScheduleProcess,
    EffectKind::CancelProcess,
    EffectKind::EmitCausalEvent,
];

/// A single proposed mutation, flattened into one tagged value so application can
/// dispatch by `tag` without pattern matching. Fields not used by a given kind
/// hold defaults; the typed builder methods on [`EffectBatch`] are the only way to
/// construct one, so each effect is well-formed by construction.
#[derive(Debug, Clone)]
pub struct Effect {
    tag: u8,
    target_id: Option<u64>,
    kind_code: u32,
    subject: Option<EntityHandle>,
    secondary: Option<EntityHandle>,
    value: Option<FactValue>,
    endpoints: Vec<RelationEndpoint>,
    strength: Option<i64>,
    state: u32,
    wake: u64,
    cause: Option<CauseRef>,
    tick: u64,
    code: u64,
    payload: Option<FactValue>,
}

impl Effect {
    /// The base effect with all fields defaulted; builders fill the ones they use.
    fn empty(tag: u8) -> Self {
        Effect {
            tag,
            target_id: None,
            kind_code: 0,
            subject: None,
            secondary: None,
            value: None,
            endpoints: Vec::new(),
            strength: None,
            state: 0,
            wake: 0,
            cause: None,
            tick: 0,
            code: 0,
            payload: None,
        }
    }

    /// The kind of mutation this effect proposes.
    pub fn effect_kind(&self) -> EffectKind {
        KINDS[self.tag as usize]
    }

    pub(crate) fn tag(&self) -> u8 {
        self.tag
    }
    pub(crate) fn target_id(&self) -> Option<u64> {
        self.target_id
    }
    pub(crate) fn kind_code(&self) -> u32 {
        self.kind_code
    }
    pub(crate) fn subject(&self) -> Option<EntityHandle> {
        self.subject
    }
    pub(crate) fn secondary(&self) -> Option<EntityHandle> {
        self.secondary
    }
    pub(crate) fn value(&self) -> Option<FactValue> {
        self.value
    }
    pub(crate) fn endpoints(&self) -> &[RelationEndpoint] {
        &self.endpoints
    }
    pub(crate) fn strength(&self) -> Option<i64> {
        self.strength
    }
    pub(crate) fn state(&self) -> u32 {
        self.state
    }
    pub(crate) fn wake(&self) -> u64 {
        self.wake
    }
    pub(crate) fn cause(&self) -> Option<CauseRef> {
        self.cause
    }
    pub(crate) fn tick(&self) -> u64 {
        self.tick
    }
    pub(crate) fn code(&self) -> u64 {
        self.code
    }
    pub(crate) fn payload(&self) -> Option<FactValue> {
        self.payload
    }
}

/// A FIFO collection of proposed mutations, applied to the sim world only at an
/// explicit boundary (`SimCoreApi::apply_effects`). Building never mutates the
/// world; the builder methods just stage effects in order.
#[derive(Debug, Clone, Default)]
pub struct EffectBatch {
    effects: Vec<Effect>,
}

impl EffectBatch {
    /// Create an empty batch.
    pub fn new() -> Self {
        EffectBatch {
            effects: Vec::new(),
        }
    }

    /// Stage an add-fact effect.
    pub fn add_fact(
        &mut self,
        kind_code: u32,
        subject: EntityHandle,
        value: FactValue,
        cause: Option<CauseRef>,
        tick: u64,
    ) {
        self.effects.push(Effect {
            kind_code,
            subject: Some(subject),
            value: Some(value),
            cause,
            tick,
            ..Effect::empty(ADD_FACT)
        });
    }

    /// Stage an update-fact effect.
    pub fn update_fact(&mut self, fact: FactId, value: FactValue, tick: u64) {
        self.effects.push(Effect {
            target_id: Some(fact.raw()),
            value: Some(value),
            tick,
            ..Effect::empty(UPDATE_FACT)
        });
    }

    /// Stage a remove-fact effect.
    pub fn remove_fact(&mut self, fact: FactId) {
        self.effects.push(Effect {
            target_id: Some(fact.raw()),
            ..Effect::empty(REMOVE_FACT)
        });
    }

    /// Stage an add-relation effect.
    pub fn add_relation(
        &mut self,
        kind_code: u32,
        endpoints: Vec<RelationEndpoint>,
        strength: Option<i64>,
        cause: Option<CauseRef>,
    ) {
        self.effects.push(Effect {
            kind_code,
            endpoints,
            strength,
            cause,
            ..Effect::empty(ADD_RELATION)
        });
    }

    /// Stage a remove-relation effect.
    pub fn remove_relation(&mut self, relation: RelationId) {
        self.effects.push(Effect {
            target_id: Some(relation.raw()),
            ..Effect::empty(REMOVE_RELATION)
        });
    }

    /// Stage a schedule-process effect.
    pub fn schedule_process(
        &mut self,
        kind_code: u32,
        subject: EntityHandle,
        state: u32,
        wake: u64,
        cause: Option<CauseRef>,
    ) {
        self.effects.push(Effect {
            kind_code,
            subject: Some(subject),
            state,
            wake,
            cause,
            ..Effect::empty(SCHEDULE_PROCESS)
        });
    }

    /// Stage a cancel-process effect.
    pub fn cancel_process(&mut self, process: ProcessId) {
        self.effects.push(Effect {
            target_id: Some(process.raw()),
            ..Effect::empty(CANCEL_PROCESS)
        });
    }

    /// Stage an emit-causal-event effect. `parties` is `(subject, secondary)` —
    /// the primary and secondary entities.
    pub fn emit_causal_event(
        &mut self,
        kind_code: u32,
        tick: u64,
        parties: (Option<EntityHandle>, Option<EntityHandle>),
        parent: Option<CauseRef>,
        code: u64,
        payload: Option<FactValue>,
    ) {
        self.effects.push(Effect {
            kind_code,
            subject: parties.0,
            secondary: parties.1,
            cause: parent,
            tick,
            code,
            payload,
            ..Effect::empty(EMIT_CAUSAL_EVENT)
        });
    }

    /// The number of staged effects.
    pub fn len(&self) -> usize {
        self.effects.len()
    }

    /// Whether the batch is empty.
    pub fn is_empty(&self) -> bool {
        self.effects.is_empty()
    }

    /// The kind of the staged effect at `index`, if any.
    pub fn kind_at(&self, index: usize) -> Option<EffectKind> {
        self.effects.get(index).map(Effect::effect_kind)
    }

    /// Consume the batch, yielding its effects in FIFO order (for application).
    pub(crate) fn into_effects(self) -> Vec<Effect> {
        self.effects
    }
}

/// The outcome of applying one effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EffectResult {
    /// The mutation was performed.
    Applied,
    /// The effect was well-formed but skipped (e.g. its subject entity is dead).
    Skipped,
    /// The effect referenced an invalid sim id and failed cleanly.
    Failed,
}

/// The per-effect outcomes of applying an [`EffectBatch`], in FIFO order.
#[derive(Debug, Clone, Default)]
pub struct EffectReport {
    results: Vec<EffectResult>,
}

impl EffectReport {
    /// Build a report from ordered outcomes.
    pub(crate) fn from_results(results: Vec<EffectResult>) -> Self {
        EffectReport { results }
    }

    /// The outcomes, in application order.
    pub fn results(&self) -> &[EffectResult] {
        &self.results
    }

    /// The outcome at position `index`, if any.
    pub fn result(&self, index: usize) -> Option<EffectResult> {
        self.results.get(index).copied()
    }

    /// The number of effects applied (outcomes recorded).
    pub fn len(&self) -> usize {
        self.results.len()
    }

    /// Whether no effects were applied.
    pub fn is_empty(&self) -> bool {
        self.results.is_empty()
    }

    /// How many outcomes equal `which`.
    pub fn count(&self, which: EffectResult) -> usize {
        self.results
            .iter()
            .filter(move |result| **result == which)
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_ecs::EntityRegistry;

    #[test]
    fn new_and_default_batches_are_empty() {
        assert!(EffectBatch::new().is_empty());
        assert_eq!(EffectBatch::new().len(), 0);
        assert!(EffectBatch::default().is_empty());
    }

    #[test]
    fn builders_stage_effects_in_fifo_order_with_kinds() {
        let mut reg = EntityRegistry::new();
        let a = reg.spawn_handle();
        let mut batch = EffectBatch::new();
        batch.add_fact(1, a, FactValue::Unsigned(1), None, 0);
        batch.update_fact(FactId::from_raw(1), FactValue::Unsigned(2), 1);
        batch.remove_fact(FactId::from_raw(1));
        batch.add_relation(2, vec![RelationEndpoint::entity(a)], Some(1), None);
        batch.remove_relation(RelationId::from_raw(1));
        batch.schedule_process(3, a, 0, 5, None);
        batch.cancel_process(ProcessId::from_raw(1));
        batch.emit_causal_event(4, 0, (Some(a), None), Some(CauseRef::Command), 7, None);
        assert_eq!(batch.len(), 8);
        let kinds: Vec<EffectKind> = batch
            .into_effects()
            .iter()
            .map(Effect::effect_kind)
            .collect();
        assert_eq!(
            kinds,
            vec![
                EffectKind::AddFact,
                EffectKind::UpdateFact,
                EffectKind::RemoveFact,
                EffectKind::AddRelation,
                EffectKind::RemoveRelation,
                EffectKind::ScheduleProcess,
                EffectKind::CancelProcess,
                EffectKind::EmitCausalEvent,
            ]
        );
    }

    #[test]
    fn kind_at_reports_staged_effect_kinds() {
        let mut reg = EntityRegistry::new();
        let a = reg.spawn_handle();
        let mut batch = EffectBatch::new();
        batch.add_fact(1, a, FactValue::Bool(true), None, 0);
        batch.remove_fact(FactId::from_raw(1));
        assert_eq!(batch.kind_at(0), Some(EffectKind::AddFact));
        assert_eq!(batch.kind_at(1), Some(EffectKind::RemoveFact));
        assert_eq!(batch.kind_at(9), None);
    }

    #[test]
    fn report_counts_and_indexes_outcomes() {
        let report = EffectReport::from_results(vec![
            EffectResult::Applied,
            EffectResult::Skipped,
            EffectResult::Applied,
            EffectResult::Failed,
        ]);
        assert_eq!(report.len(), 4);
        assert!(!report.is_empty());
        assert_eq!(report.result(0), Some(EffectResult::Applied));
        assert_eq!(report.result(9), None);
        assert_eq!(report.count(EffectResult::Applied), 2);
        assert_eq!(report.count(EffectResult::Skipped), 1);
        assert_eq!(report.count(EffectResult::Failed), 1);
        assert_eq!(report.results().len(), 4);
        assert!(EffectReport::default().is_empty());
    }
}
