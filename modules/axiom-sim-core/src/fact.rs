//! The generic fact model: typed assertions about the simulated world.

use std::collections::BTreeMap;

use axiom_ecs::EntityHandle;

use crate::cause::CauseRef;
use crate::ids::FactId;

/// The domain-defined *kind* of a fact, as an opaque deterministic code.
///
/// sim-core assigns no meaning to a kind — later phases map codes to concepts
/// (has-material, has-temperature, …). It is just a stable, comparable tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FactKind(u32);

impl FactKind {
    /// A fact kind from a deterministic code.
    pub const fn new(code: u32) -> Self {
        FactKind(code)
    }

    /// The raw code.
    pub const fn code(self) -> u32 {
        self.0
    }
}

/// The value carried by a fact: a small, boring, fully-comparable union.
///
/// Deliberately closed and `Eq`/`Hash`-able (no floats, no dynamic maps, no
/// JSON-like blobs) so the entire fact model is totally deterministic. A
/// quantity-like scalar is intentionally absent — see `PHASE_2_DEFERRED.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FactValue {
    /// A signed integer.
    Signed(i64),
    /// An unsigned integer.
    Unsigned(u64),
    /// An interned symbol code (the caller owns the code↔meaning mapping).
    Symbol(u64),
    /// A boolean.
    Bool(bool),
    /// A reference to an ECS entity.
    Entity(EntityHandle),
}

/// A typed assertion about a subject entity at a logical tick.
///
/// A fact records *that* something is true (its [`FactKind`] + [`FactValue`])
/// about a [`subject`](Self::subject) entity, optionally *why* ([`cause`](Self::cause)),
/// and *when* in logical time ([`tick`](Self::tick)). Its [`FactId`] is the stable
/// deterministic ordering key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fact {
    id: FactId,
    kind: FactKind,
    subject: EntityHandle,
    value: FactValue,
    cause: Option<CauseRef>,
    tick: u64,
}

impl Fact {
    /// This fact's stable id (its deterministic ordering key).
    pub const fn id(&self) -> FactId {
        self.id
    }

    /// The fact's kind.
    pub const fn kind(&self) -> FactKind {
        self.kind
    }

    /// The subject entity this fact asserts about.
    pub const fn subject(&self) -> EntityHandle {
        self.subject
    }

    /// The fact's current value.
    pub const fn value(&self) -> FactValue {
        self.value
    }

    /// What caused this fact, if recorded.
    pub const fn cause(&self) -> Option<CauseRef> {
        self.cause
    }

    /// The logical tick this fact was last asserted/updated at.
    pub const fn tick(&self) -> u64 {
        self.tick
    }
}

/// A deterministic store of facts, keyed and iterated by ascending [`FactId`].
#[derive(Debug, Clone, Default)]
pub struct FactStore {
    facts: BTreeMap<FactId, Fact>,
    next: u64,
}

impl FactStore {
    /// Create an empty fact store. The first inserted fact has id 1.
    pub fn new() -> Self {
        FactStore {
            facts: BTreeMap::new(),
            next: 1,
        }
    }

    /// Insert a new fact, minting and returning its deterministic id.
    pub fn insert(
        &mut self,
        kind: FactKind,
        subject: EntityHandle,
        value: FactValue,
        cause: Option<CauseRef>,
        tick: u64,
    ) -> FactId {
        let id = FactId::from_raw(self.next);
        self.next += 1;
        self.facts.insert(
            id,
            Fact {
                id,
                kind,
                subject,
                value,
                cause,
                tick,
            },
        );
        id
    }

    /// Borrow a fact by id, if present.
    pub fn get(&self, id: FactId) -> Option<&Fact> {
        self.facts.get(&id)
    }

    /// Remove a fact by id, returning it if present (a clean `None` if absent).
    pub fn remove(&mut self, id: FactId) -> Option<Fact> {
        self.facts.remove(&id)
    }

    /// Update a fact's value (and the tick it changed on). Returns whether the
    /// fact existed; a missing id is a clean `false`.
    pub fn update(&mut self, id: FactId, value: FactValue, tick: u64) -> bool {
        self.facts
            .get_mut(&id)
            .map(|fact| {
                fact.value = value;
                fact.tick = tick;
            })
            .is_some()
    }

    /// Facts of a given kind, in ascending id order.
    pub fn by_kind(&self, kind: FactKind) -> impl Iterator<Item = &Fact> {
        self.facts.values().filter(move |fact| fact.kind == kind)
    }

    /// Facts about a given subject, in ascending id order.
    pub fn by_subject(&self, subject: EntityHandle) -> impl Iterator<Item = &Fact> {
        self.facts
            .values()
            .filter(move |fact| fact.subject == subject)
    }

    /// All facts, in ascending id order.
    pub fn iter(&self) -> impl Iterator<Item = &Fact> {
        self.facts.values()
    }

    /// The number of stored facts.
    pub fn len(&self) -> usize {
        self.facts.len()
    }

    /// Whether the store holds no facts.
    pub fn is_empty(&self) -> bool {
        self.facts.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::ProcessId;
    use axiom_ecs::EntityRegistry;

    fn subject(reg: &mut EntityRegistry) -> EntityHandle {
        reg.spawn_handle()
    }

    #[test]
    fn new_and_default_are_empty() {
        let store = FactStore::new();
        assert!(store.is_empty());
        assert_eq!(store.len(), 0);
        assert!(FactStore::default().is_empty());
    }

    #[test]
    fn insert_get_update_remove() {
        let mut reg = EntityRegistry::new();
        let a = subject(&mut reg);
        let mut store = FactStore::new();
        let id = store.insert(FactKind::new(1), a, FactValue::Unsigned(10), None, 0);
        assert_eq!(id.raw(), 1);
        assert_eq!(store.len(), 1);
        assert_eq!(store.get(id).unwrap().value(), FactValue::Unsigned(10));
        assert_eq!(store.get(id).unwrap().subject(), a);
        assert_eq!(store.get(id).unwrap().kind(), FactKind::new(1));
        assert_eq!(store.get(id).unwrap().cause(), None);
        assert_eq!(store.get(id).unwrap().tick(), 0);
        assert!(store.update(id, FactValue::Unsigned(20), 5));
        assert_eq!(store.get(id).unwrap().value(), FactValue::Unsigned(20));
        assert_eq!(store.get(id).unwrap().tick(), 5);
        assert!(!store.update(FactId::from_raw(999), FactValue::Bool(true), 9));
        assert_eq!(store.remove(id).unwrap().id(), id);
        assert!(store.get(id).is_none());
        assert!(
            store.remove(id).is_none(),
            "removing a missing fact is a clean None"
        );
    }

    #[test]
    fn carries_cause_and_value_variants() {
        let mut reg = EntityRegistry::new();
        let a = subject(&mut reg);
        let mut store = FactStore::new();
        let cause = Some(CauseRef::Process(ProcessId::from_raw(3)));
        let id = store.insert(FactKind::new(2), a, FactValue::Signed(-4), cause, 7);
        let fact = store.get(id).unwrap();
        assert_eq!(fact.value(), FactValue::Signed(-4));
        assert_eq!(fact.cause(), cause);
        assert_ne!(FactValue::Symbol(1), FactValue::Unsigned(1));
        assert_ne!(FactValue::Entity(a), FactValue::Symbol(0));
    }

    #[test]
    fn queries_by_kind_and_subject_are_ascending() {
        let mut reg = EntityRegistry::new();
        let a = subject(&mut reg);
        let b = subject(&mut reg);
        let mut store = FactStore::new();
        let f1 = store.insert(FactKind::new(1), a, FactValue::Bool(true), None, 0);
        let _f2 = store.insert(FactKind::new(2), b, FactValue::Bool(false), None, 0);
        let f3 = store.insert(FactKind::new(1), a, FactValue::Unsigned(9), None, 0);
        let by_kind: Vec<FactId> = store.by_kind(FactKind::new(1)).map(Fact::id).collect();
        assert_eq!(by_kind, vec![f1, f3]);
        let by_subject: Vec<FactId> = store.by_subject(b).map(Fact::id).collect();
        assert_eq!(by_subject.len(), 1);
        let all: Vec<u64> = store.iter().map(|f| f.id().raw()).collect();
        assert_eq!(all, vec![1, 2, 3]);
    }
}
