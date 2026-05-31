//! The deterministic entity → component-row container.

use std::collections::BTreeMap;

use axiom_kernel::EntityId;

/// A generic, deterministic store of entities and their component rows.
///
/// `R` is a consumer-defined component row (e.g. a struct of `Option`
/// components). The store knows nothing about what `R` contains — it only
/// provides stable identity, ordered iteration, and row access. Entities are
/// keyed by a [`EntityId`] minted monotonically from 1, and held in a
/// `BTreeMap`, so iteration is always in ascending-id order on every platform —
/// the determinism the rest of the engine relies on.
#[derive(Debug, Clone)]
pub struct EntityStore<R> {
    rows: BTreeMap<EntityId, R>,
    next_id: u64,
}

impl<R: Default> EntityStore<R> {
    /// Spawn a new entity with a default-constructed component row.
    pub fn spawn(&mut self) -> EntityId {
        let id = EntityId::from_raw(self.next_id);
        self.next_id += 1;
        self.rows.insert(id, R::default());
        id
    }
}

impl<R> EntityStore<R> {
    /// Create an empty store. The first spawned entity has raw id 1 (0 is the
    /// kernel's null/invalid id).
    pub fn new() -> Self {
        EntityStore {
            rows: BTreeMap::new(),
            next_id: 1,
        }
    }

    /// Borrow an entity's component row.
    pub fn get(&self, entity: EntityId) -> Option<&R> {
        self.rows.get(&entity)
    }

    /// Mutably borrow an entity's component row.
    pub fn get_mut(&mut self, entity: EntityId) -> Option<&mut R> {
        self.rows.get_mut(&entity)
    }

    /// Whether the entity exists in the store.
    pub fn contains(&self, entity: EntityId) -> bool {
        self.rows.contains_key(&entity)
    }

    /// Remove an entity, returning whether it existed.
    pub fn despawn(&mut self, entity: EntityId) -> bool {
        self.rows.remove(&entity).is_some()
    }

    /// Iterate `(entity, &row)` in ascending entity-id order.
    pub fn iter(&self) -> impl Iterator<Item = (EntityId, &R)> {
        self.rows.iter().map(|(id, row)| (*id, row))
    }

    /// The number of live entities.
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether the store has no entities.
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

impl<R> Default for EntityStore<R> {
    fn default() -> Self {
        EntityStore::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Default, PartialEq)]
    struct Row {
        value: i32,
    }

    #[test]
    fn spawn_mints_monotonic_ids_from_one() {
        let mut store: EntityStore<Row> = EntityStore::new();
        let a = store.spawn();
        let b = store.spawn();
        assert_eq!(a.raw(), 1);
        assert_eq!(b.raw(), 2);
        assert_eq!(store.len(), 2);
        assert!(!store.is_empty());
    }

    #[test]
    fn default_store_is_empty() {
        let store: EntityStore<Row> = EntityStore::default();
        assert!(store.is_empty());
        assert_eq!(store.len(), 0);
    }

    #[test]
    fn get_and_get_mut_present_and_absent() {
        let mut store: EntityStore<Row> = EntityStore::new();
        let e = store.spawn();
        assert_eq!(store.get(e), Some(&Row { value: 0 }));
        store.get_mut(e).unwrap().value = 7;
        assert_eq!(store.get(e).unwrap().value, 7);
        let absent = EntityId::from_raw(999);
        assert!(store.get(absent).is_none());
        assert!(store.get_mut(absent).is_none());
    }

    #[test]
    fn contains_and_despawn() {
        let mut store: EntityStore<Row> = EntityStore::new();
        let e = store.spawn();
        assert!(store.contains(e));
        assert!(store.despawn(e));
        assert!(!store.contains(e));
        // Despawning a missing entity reports false.
        assert!(!store.despawn(e));
    }

    #[test]
    fn iter_is_ascending_by_entity_id() {
        let mut store: EntityStore<Row> = EntityStore::new();
        let a = store.spawn();
        let b = store.spawn();
        let c = store.spawn();
        let ids: Vec<u64> = store.iter().map(|(id, _)| id.raw()).collect();
        assert_eq!(ids, vec![a.raw(), b.raw(), c.raw()]);
        assert_eq!(ids, vec![1, 2, 3]);
    }
}
