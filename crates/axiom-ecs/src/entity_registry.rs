//! The set of live entities and the source of stable entity identity.

use std::collections::BTreeSet;

use axiom_kernel::EntityId;

/// Tracks which entities exist, independent of what components they carry.
///
/// Entities are minted monotonically from raw id 1 (0 is the kernel's
/// null/invalid id) and held in a `BTreeSet`, so iteration is ascending-id
/// deterministic. Components live in separate [`crate::ComponentColumn`]s keyed
/// by these ids — the registry is purely lifecycle.
#[derive(Debug, Clone)]
pub struct EntityRegistry {
    live: BTreeSet<EntityId>,
    next_id: u64,
}

impl EntityRegistry {
    /// Create an empty registry. The first spawned entity has raw id 1.
    pub fn new() -> Self {
        EntityRegistry {
            live: BTreeSet::new(),
            next_id: 1,
        }
    }

    /// Mint and register a new entity.
    pub fn spawn(&mut self) -> EntityId {
        let id = EntityId::from_raw(self.next_id);
        self.next_id += 1;
        self.live.insert(id);
        id
    }

    /// Remove an entity from the live set, returning whether it existed.
    /// Component columns are not touched — the consumer owns column cleanup.
    pub fn despawn(&mut self, entity: EntityId) -> bool {
        self.live.remove(&entity)
    }

    /// Whether the entity is live.
    pub fn contains(&self, entity: EntityId) -> bool {
        self.live.contains(&entity)
    }

    /// Iterate live entities in ascending id order.
    pub fn iter(&self) -> impl Iterator<Item = EntityId> + '_ {
        self.live.iter().copied()
    }

    /// The number of live entities.
    pub fn len(&self) -> usize {
        self.live.len()
    }

    /// Whether no entities are live.
    pub fn is_empty(&self) -> bool {
        self.live.is_empty()
    }
}

impl Default for EntityRegistry {
    fn default() -> Self {
        EntityRegistry::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_mints_monotonic_ids_from_one() {
        let mut reg = EntityRegistry::new();
        let a = reg.spawn();
        let b = reg.spawn();
        assert_eq!(a.raw(), 1);
        assert_eq!(b.raw(), 2);
        assert_eq!(reg.len(), 2);
        assert!(!reg.is_empty());
    }

    #[test]
    fn default_registry_is_empty() {
        let reg = EntityRegistry::default();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
    }

    #[test]
    fn despawn_present_and_absent() {
        let mut reg = EntityRegistry::new();
        let a = reg.spawn();
        assert!(reg.contains(a));
        assert!(reg.despawn(a));
        assert!(!reg.contains(a));
        assert!(!reg.despawn(a));
    }

    #[test]
    fn iter_is_ascending_by_id() {
        let mut reg = EntityRegistry::new();
        let a = reg.spawn();
        let b = reg.spawn();
        let c = reg.spawn();
        let ids: Vec<u64> = reg.iter().map(|id| id.raw()).collect();
        assert_eq!(ids, vec![a.raw(), b.raw(), c.raw()]);
        assert_eq!(ids, vec![1, 2, 3]);
    }
}
