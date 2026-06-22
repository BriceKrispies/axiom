//! The generic relation model: typed links between simulation subjects.

use std::collections::BTreeMap;

use axiom_ecs::EntityHandle;

use crate::cause::CauseRef;
use crate::ids::RelationId;

/// The domain-defined *kind* of a relation, as an opaque deterministic code.
///
/// sim-core assigns no meaning — later phases map codes to concepts (inside,
/// owns, adjacent-to, caused-by, …). It is just a stable, comparable tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RelationKind(u32);

impl RelationKind {
    /// A relation kind from a deterministic code.
    pub const fn new(code: u32) -> Self {
        RelationKind(code)
    }

    /// The raw code.
    pub const fn code(self) -> u32 {
        self.0
    }
}

/// One endpoint of a relation: an ECS entity, or an opaque symbol subject (for
/// non-entity subjects like cells or events the domain encodes as codes).
///
/// A tagged value (not an enum) so `as_entity`/`as_symbol` read branchlessly via
/// `then_some`. Exactly one arm is meaningful, selected by `is_entity`; the
/// constructors are the only way to build a value, so that invariant holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RelationEndpoint {
    is_entity: bool,
    entity: EntityHandle,
    symbol: u64,
}

/// The placeholder handle stored in a symbol endpoint's unused entity slot (the
/// kernel null entity id); never read, because `is_entity` is false.
const NULL_ENTITY: EntityHandle = EntityHandle::new(axiom_kernel::EntityId::from_raw(0), 0);

impl RelationEndpoint {
    /// An endpoint referencing an ECS entity.
    pub const fn entity(handle: EntityHandle) -> Self {
        RelationEndpoint {
            is_entity: true,
            entity: handle,
            symbol: 0,
        }
    }

    /// An endpoint referencing an opaque symbol subject.
    pub const fn symbol(code: u64) -> Self {
        RelationEndpoint {
            is_entity: false,
            entity: NULL_ENTITY,
            symbol: code,
        }
    }

    /// The entity this endpoint references, if it is an entity endpoint.
    pub fn as_entity(self) -> Option<EntityHandle> {
        self.is_entity.then_some(self.entity)
    }

    /// The symbol code this endpoint references, if it is a symbol endpoint.
    pub fn as_symbol(self) -> Option<u64> {
        (!self.is_entity).then_some(self.symbol)
    }
}

/// A typed link connecting two or more ordered subjects.
///
/// The endpoint order is significant (e.g. `inside(item, container)`), so it is
/// preserved exactly. A relation may carry an optional integer
/// [`strength`](Self::strength) and an optional [`cause`](Self::cause); its
/// [`RelationId`] is the deterministic ordering key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relation {
    id: RelationId,
    kind: RelationKind,
    endpoints: Vec<RelationEndpoint>,
    strength: Option<i64>,
    cause: Option<CauseRef>,
}

impl Relation {
    /// This relation's stable id (its deterministic ordering key).
    pub const fn id(&self) -> RelationId {
        self.id
    }

    /// The relation's kind.
    pub const fn kind(&self) -> RelationKind {
        self.kind
    }

    /// The ordered endpoints.
    pub fn endpoints(&self) -> &[RelationEndpoint] {
        &self.endpoints
    }

    /// The optional strength/weight.
    pub const fn strength(&self) -> Option<i64> {
        self.strength
    }

    /// What caused this relation, if recorded.
    pub const fn cause(&self) -> Option<CauseRef> {
        self.cause
    }
}

/// A deterministic store of relations, keyed and iterated by ascending id.
#[derive(Debug, Clone, Default)]
pub struct RelationStore {
    relations: BTreeMap<RelationId, Relation>,
    next: u64,
}

impl RelationStore {
    /// Create an empty relation store. The first inserted relation has id 1.
    pub fn new() -> Self {
        RelationStore {
            relations: BTreeMap::new(),
            next: 1,
        }
    }

    /// Insert a new relation, minting and returning its deterministic id.
    pub fn insert(
        &mut self,
        kind: RelationKind,
        endpoints: Vec<RelationEndpoint>,
        strength: Option<i64>,
        cause: Option<CauseRef>,
    ) -> RelationId {
        let id = RelationId::from_raw(self.next);
        self.next += 1;
        self.relations.insert(
            id,
            Relation {
                id,
                kind,
                endpoints,
                strength,
                cause,
            },
        );
        id
    }

    /// Borrow a relation by id, if present.
    pub fn get(&self, id: RelationId) -> Option<&Relation> {
        self.relations.get(&id)
    }

    /// Remove a relation by id, returning it if present (clean `None` if absent).
    pub fn remove(&mut self, id: RelationId) -> Option<Relation> {
        self.relations.remove(&id)
    }

    /// Relations of a given kind, in ascending id order.
    pub fn by_kind(&self, kind: RelationKind) -> impl Iterator<Item = &Relation> {
        self.relations
            .values()
            .filter(move |relation| relation.kind == kind)
    }

    /// Relations touching a given endpoint, in ascending id order.
    pub fn by_endpoint(&self, endpoint: RelationEndpoint) -> impl Iterator<Item = &Relation> {
        self.relations
            .values()
            .filter(move |relation| relation.endpoints.contains(&endpoint))
    }

    /// All relations, in ascending id order.
    pub fn iter(&self) -> impl Iterator<Item = &Relation> {
        self.relations.values()
    }

    /// The number of stored relations.
    pub fn len(&self) -> usize {
        self.relations.len()
    }

    /// Whether the store holds no relations.
    pub fn is_empty(&self) -> bool {
        self.relations.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_ecs::EntityRegistry;

    #[test]
    fn new_and_default_are_empty() {
        assert!(RelationStore::new().is_empty());
        assert_eq!(RelationStore::new().len(), 0);
        assert!(RelationStore::default().is_empty());
    }

    #[test]
    fn endpoint_accessors_are_branchless_and_exclusive() {
        let mut reg = EntityRegistry::new();
        let h = reg.spawn_handle();
        let entity = RelationEndpoint::entity(h);
        assert_eq!(entity.as_entity(), Some(h));
        assert_eq!(entity.as_symbol(), None);
        let symbol = RelationEndpoint::symbol(42);
        assert_eq!(symbol.as_symbol(), Some(42));
        assert_eq!(symbol.as_entity(), None);
        assert_ne!(entity, symbol);
    }

    #[test]
    fn insert_get_remove_with_endpoints_and_strength() {
        let mut reg = EntityRegistry::new();
        let item = reg.spawn_handle();
        let container = reg.spawn_handle();
        let mut store = RelationStore::new();
        let endpoints = vec![
            RelationEndpoint::entity(item),
            RelationEndpoint::entity(container),
        ];
        let id = store.insert(RelationKind::new(1), endpoints, Some(3), None);
        assert_eq!(id.raw(), 1);
        let relation = store.get(id).unwrap();
        assert_eq!(relation.kind(), RelationKind::new(1));
        assert_eq!(relation.strength(), Some(3));
        assert_eq!(relation.cause(), None);
        assert_eq!(
            relation.endpoints(),
            &[
                RelationEndpoint::entity(item),
                RelationEndpoint::entity(container)
            ]
        );
        assert_eq!(store.remove(id).unwrap().id(), id);
        assert!(store.get(id).is_none());
        assert!(
            store.remove(id).is_none(),
            "removing a missing relation is a clean None"
        );
    }

    #[test]
    fn queries_by_kind_and_endpoint_are_ascending() {
        let mut reg = EntityRegistry::new();
        let a = reg.spawn_handle();
        let b = reg.spawn_handle();
        let mut store = RelationStore::new();
        let r1 = store.insert(
            RelationKind::new(1),
            vec![RelationEndpoint::entity(a)],
            None,
            None,
        );
        let _r2 = store.insert(
            RelationKind::new(2),
            vec![RelationEndpoint::symbol(99)],
            None,
            None,
        );
        let r3 = store.insert(
            RelationKind::new(1),
            vec![RelationEndpoint::entity(b), RelationEndpoint::entity(a)],
            None,
            None,
        );
        let by_kind: Vec<RelationId> = store
            .by_kind(RelationKind::new(1))
            .map(Relation::id)
            .collect();
        assert_eq!(by_kind, vec![r1, r3]);
        let by_endpoint: Vec<RelationId> = store
            .by_endpoint(RelationEndpoint::entity(a))
            .map(Relation::id)
            .collect();
        assert_eq!(by_endpoint, vec![r1, r3]);
        let by_symbol: Vec<RelationId> = store
            .by_endpoint(RelationEndpoint::symbol(99))
            .map(Relation::id)
            .collect();
        assert_eq!(by_symbol.len(), 1);
        let all: Vec<u64> = store.iter().map(|r| r.id().raw()).collect();
        assert_eq!(all, vec![1, 2, 3]);
    }
}
