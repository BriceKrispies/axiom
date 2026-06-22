//! Deterministic dirty-fact / relation / subject invalidation tracking.
//!
//! Records *what changed* since the last boundary, with the changed item's kind
//! code (so kind-level dependency subscriptions can match) and an optional causal
//! reason. Cleared at an explicit boundary — never by wall-clock time.

use std::collections::BTreeMap;

use axiom_ecs::EntityHandle;

use crate::cause::CauseRef;
use crate::ids::{FactId, RelationId};

/// The nature of a dirty change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DirtyKind {
    /// The item was added.
    Added,
    /// The item's value was updated.
    Updated,
    /// The item was removed.
    Removed,
    /// The item was touched (marked changed without a value change).
    Touched,
    /// The item was invalidated because a dependency of it changed.
    DependencyInvalidated,
}

const DIRTY_KINDS: [DirtyKind; 5] = [
    DirtyKind::Added,
    DirtyKind::Updated,
    DirtyKind::Removed,
    DirtyKind::Touched,
    DirtyKind::DependencyInvalidated,
];

impl DirtyKind {
    /// Construct from a code, `None` if out of range.
    pub fn from_code(code: u8) -> Option<DirtyKind> {
        DIRTY_KINDS.get(code as usize).copied()
    }

    /// The kind's deterministic code.
    pub fn code(self) -> u8 {
        self as u8
    }
}

/// Why an item was invalidated: its dirty kind and an optional cause.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidationReason {
    kind: DirtyKind,
    cause: Option<CauseRef>,
}

impl InvalidationReason {
    /// An invalidation reason.
    pub const fn new(kind: DirtyKind, cause: Option<CauseRef>) -> Self {
        InvalidationReason { kind, cause }
    }
    /// The dirty kind.
    pub const fn kind(&self) -> DirtyKind {
        self.kind
    }
    /// The cause, if recorded.
    pub const fn cause(&self) -> Option<CauseRef> {
        self.cause
    }
}

/// A dirty fact: its id, the changed fact's kind code, and the reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirtyFact {
    fact: FactId,
    fact_kind: u32,
    reason: InvalidationReason,
}

impl DirtyFact {
    /// The dirty fact id.
    pub const fn fact(&self) -> FactId {
        self.fact
    }
    /// The fact's kind code (for kind-level subscriptions).
    pub const fn fact_kind(&self) -> u32 {
        self.fact_kind
    }
    /// The invalidation reason.
    pub const fn reason(&self) -> InvalidationReason {
        self.reason
    }
}

/// A dirty relation: its id, the changed relation's kind code, and the reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirtyRelation {
    relation: RelationId,
    relation_kind: u32,
    reason: InvalidationReason,
}

impl DirtyRelation {
    /// The dirty relation id.
    pub const fn relation(&self) -> RelationId {
        self.relation
    }
    /// The relation's kind code.
    pub const fn relation_kind(&self) -> u32 {
        self.relation_kind
    }
    /// The invalidation reason.
    pub const fn reason(&self) -> InvalidationReason {
        self.reason
    }
}

/// A dirty subject: the entity and the reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirtySubject {
    subject: EntityHandle,
    reason: InvalidationReason,
}

impl DirtySubject {
    /// The dirty subject entity.
    pub const fn subject(&self) -> EntityHandle {
        self.subject
    }
    /// The invalidation reason.
    pub const fn reason(&self) -> InvalidationReason {
        self.reason
    }
}

/// The accumulated dirty set since the last [`clear`](Self::clear).
#[derive(Debug, Clone, Default)]
pub struct DirtySet {
    facts: BTreeMap<FactId, (u32, InvalidationReason)>,
    relations: BTreeMap<RelationId, (u32, InvalidationReason)>,
    subjects: BTreeMap<EntityHandle, InvalidationReason>,
}

impl DirtySet {
    /// Create an empty dirty set.
    pub fn new() -> Self {
        DirtySet {
            facts: BTreeMap::new(),
            relations: BTreeMap::new(),
            subjects: BTreeMap::new(),
        }
    }

    /// Mark a fact dirty (last write per fact within a window wins).
    pub fn mark_fact(
        &mut self,
        fact: FactId,
        fact_kind: u32,
        kind: DirtyKind,
        cause: Option<CauseRef>,
    ) {
        self.facts
            .insert(fact, (fact_kind, InvalidationReason::new(kind, cause)));
    }

    /// Mark a relation dirty.
    pub fn mark_relation(
        &mut self,
        relation: RelationId,
        relation_kind: u32,
        kind: DirtyKind,
        cause: Option<CauseRef>,
    ) {
        self.relations.insert(
            relation,
            (relation_kind, InvalidationReason::new(kind, cause)),
        );
    }

    /// Mark a subject dirty.
    pub fn mark_subject(
        &mut self,
        subject: EntityHandle,
        kind: DirtyKind,
        cause: Option<CauseRef>,
    ) {
        self.subjects
            .insert(subject, InvalidationReason::new(kind, cause));
    }

    /// The dirty facts, ascending by id.
    pub fn dirty_facts(&self) -> impl Iterator<Item = DirtyFact> + '_ {
        self.facts
            .iter()
            .map(|(fact, (fact_kind, reason))| DirtyFact {
                fact: *fact,
                fact_kind: *fact_kind,
                reason: *reason,
            })
    }

    /// The dirty relations, ascending by id.
    pub fn dirty_relations(&self) -> impl Iterator<Item = DirtyRelation> + '_ {
        self.relations
            .iter()
            .map(|(relation, (relation_kind, reason))| DirtyRelation {
                relation: *relation,
                relation_kind: *relation_kind,
                reason: *reason,
            })
    }

    /// The dirty subjects, ascending by entity id.
    pub fn dirty_subjects(&self) -> impl Iterator<Item = DirtySubject> + '_ {
        self.subjects.iter().map(|(subject, reason)| DirtySubject {
            subject: *subject,
            reason: *reason,
        })
    }

    /// Clear all dirty state (the explicit boundary).
    pub fn clear(&mut self) {
        self.facts.clear();
        self.relations.clear();
        self.subjects.clear();
    }

    /// The total number of dirty entries.
    pub fn len(&self) -> usize {
        self.facts.len() + self.relations.len() + self.subjects.len()
    }

    /// Whether nothing is dirty.
    pub fn is_empty(&self) -> bool {
        self.facts.is_empty() & self.relations.is_empty() & self.subjects.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(raw: u64) -> EntityHandle {
        EntityHandle::new(axiom_kernel::EntityId::from_raw(raw), 0)
    }

    #[test]
    fn dirty_kind_codes_round_trip() {
        assert_eq!(DirtyKind::from_code(0), Some(DirtyKind::Added));
        assert_eq!(
            DirtyKind::from_code(4),
            Some(DirtyKind::DependencyInvalidated)
        );
        assert_eq!(DirtyKind::from_code(5), None);
        assert_eq!(DirtyKind::Updated.code(), 1);
    }

    #[test]
    fn mark_query_and_clear_facts() {
        let mut set = DirtySet::new();
        assert!(set.is_empty());
        set.mark_fact(
            FactId::from_raw(2),
            7,
            DirtyKind::Updated,
            Some(CauseRef::Command),
        );
        set.mark_fact(FactId::from_raw(1), 7, DirtyKind::Added, None);
        // Last write per fact wins.
        set.mark_fact(FactId::from_raw(1), 7, DirtyKind::Removed, None);
        let facts: Vec<(u64, u32, DirtyKind)> = set
            .dirty_facts()
            .map(|d| (d.fact().raw(), d.fact_kind(), d.reason().kind()))
            .collect();
        assert_eq!(
            facts,
            vec![(1, 7, DirtyKind::Removed), (2, 7, DirtyKind::Updated)]
        );
        assert_eq!(set.len(), 2);
        set.clear();
        assert!(set.is_empty());
        assert_eq!(set.dirty_facts().count(), 0);
    }

    #[test]
    fn mark_relations_and_subjects() {
        let mut set = DirtySet::new();
        set.mark_relation(RelationId::from_raw(5), 3, DirtyKind::Added, None);
        set.mark_subject(e(1), DirtyKind::Touched, Some(CauseRef::Command));
        let relations: Vec<(u64, u32)> = set
            .dirty_relations()
            .map(|d| (d.relation().raw(), d.relation_kind()))
            .collect();
        assert_eq!(relations, vec![(5, 3)]);
        let subjects: Vec<DirtyKind> = set.dirty_subjects().map(|d| d.reason().kind()).collect();
        assert_eq!(subjects, vec![DirtyKind::Touched]);
        assert_eq!(
            set.dirty_subjects().next().unwrap().reason().cause(),
            Some(CauseRef::Command)
        );
        assert_eq!(set.len(), 2);
    }
}
