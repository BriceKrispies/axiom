//! The set of live entities and the source of stable, generational identity.

use std::collections::BTreeMap;

use axiom_kernel::{BinaryReader, BinaryWriter, EntityId, KernelResult, Reflect};

use crate::entity_handle::EntityHandle;

/// Tracks which entities exist, mints generational handles, and recycles slots.
///
/// Each live entity occupies a **slot** (an [`EntityId`], minted monotonically
/// from raw id 1 — 0 is the kernel's null id) carrying a **generation**. When an
/// entity is despawned its slot returns to a deterministic free list with its
/// generation bumped; a later spawn reuses that slot at the new generation. A
/// handle from before a despawn therefore names a stale `(slot, generation)` pair
/// that no longer matches the live generation, so it is detectably invalid.
///
/// Iteration over live entities is ascending by slot (a `BTreeMap`), so the
/// registry is replay-deterministic; the free list is a `Vec` reused in LIFO
/// order, also deterministic. The slot-returning [`spawn`](Self::spawn) /
/// [`despawn`](Self::despawn) API is preserved; generational identity is the
/// added [`spawn_handle`](Self::spawn_handle) / handle-checked surface.
#[derive(Debug, Clone)]
pub struct EntityRegistry {
    live: BTreeMap<EntityId, u32>,
    free: Vec<EntityHandle>,
    next_slot: u64,
}

impl EntityRegistry {
    /// Create an empty registry. The first spawned entity has slot id 1.
    pub fn new() -> Self {
        EntityRegistry {
            live: BTreeMap::new(),
            free: Vec::new(),
            next_slot: 1,
        }
    }

    /// Mint and register a new entity, returning its generational handle. Reuses a
    /// freed slot (at a bumped generation) when one is available, else mints a
    /// fresh slot at generation 0.
    pub fn spawn_handle(&mut self) -> EntityHandle {
        let reused = self.free.pop();
        let fresh = EntityHandle::new(EntityId::from_raw(self.next_slot), 0);
        self.next_slot += reused.is_none() as u64;
        let handle = reused.unwrap_or(fresh);
        self.live.insert(handle.id(), handle.generation());
        handle
    }

    /// Mint and register a new entity, returning its slot id. Equivalent to
    /// [`spawn_handle`](Self::spawn_handle) discarding the generation.
    pub fn spawn(&mut self) -> EntityId {
        self.spawn_handle().id()
    }

    /// Remove the entity in `slot` from the live set, returning whether it existed.
    /// A removed slot returns to the free list with its generation bumped, so its
    /// next occupant is distinguishable. Component cleanup is the world's
    /// (see [`crate::World::despawn`]).
    pub fn despawn(&mut self, slot: EntityId) -> bool {
        let generation = self.live.remove(&slot);
        generation
            .into_iter()
            .for_each(|g| self.free.push(EntityHandle::new(slot, g.wrapping_add(1))));
        generation.is_some()
    }

    /// Remove the entity named by `handle`, but only if the handle is current.
    /// Returns whether the entity was live and removed; a stale handle is a clean
    /// `false` no-op.
    pub fn despawn_handle(&mut self, handle: EntityHandle) -> bool {
        let current = self.is_current(handle);
        current.then(|| self.despawn(handle.id()));
        current
    }

    /// Whether `handle` names the entity currently occupying its slot (i.e. the
    /// slot is live and its generation matches the handle's).
    pub fn is_current(&self, handle: EntityHandle) -> bool {
        self.live.get(&handle.id()).copied() == Some(handle.generation())
    }

    /// Whether `handle` is stale — it does not name the current occupant of its
    /// slot (the slot is empty, or has been reused at a newer generation).
    pub fn is_stale(&self, handle: EntityHandle) -> bool {
        !self.is_current(handle)
    }

    /// Whether the slot is live, ignoring generation.
    pub fn contains(&self, slot: EntityId) -> bool {
        self.live.contains_key(&slot)
    }

    /// The live generation of `slot`, if it is live.
    pub fn generation(&self, slot: EntityId) -> Option<u32> {
        self.live.get(&slot).copied()
    }

    /// Iterate live entity slots in ascending id order.
    pub fn iter(&self) -> impl Iterator<Item = EntityId> + '_ {
        self.live.keys().copied()
    }

    /// Iterate live entities as handles, in ascending slot-id order.
    pub fn iter_handles(&self) -> impl Iterator<Item = EntityHandle> + '_ {
        self.live
            .iter()
            .map(|(slot, generation)| EntityHandle::new(*slot, *generation))
    }

    /// The number of live entities.
    pub fn len(&self) -> usize {
        self.live.len()
    }

    /// Whether no entities are live.
    pub fn is_empty(&self) -> bool {
        self.live.is_empty()
    }

    /// Serialize the registry's identity state: next slot, the live slots and
    /// their generations (ascending), and the free list — enough to reproduce
    /// future spawns exactly after a restore.
    pub fn serialize(&self, writer: &mut BinaryWriter) {
        EntityId::from_raw(self.next_slot).reflect_write(writer);
        writer.write_u32(self.live.len() as u32);
        self.live.iter().for_each(|(slot, generation)| {
            slot.reflect_write(writer);
            generation.reflect_write(writer);
        });
        writer.write_u32(self.free.len() as u32);
        self.free
            .iter()
            .for_each(|handle| handle.reflect_write(writer));
    }

    /// Reconstruct a registry from bytes produced by [`Self::serialize`].
    pub fn deserialize(reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
        EntityId::reflect_read(reader).and_then(|next| {
            read_live(reader).and_then(|live| {
                read_free(reader).map(|free| EntityRegistry {
                    live,
                    free,
                    next_slot: next.raw(),
                })
            })
        })
    }
}

/// Read the live `(slot, generation)` map: a count then that many pairs.
fn read_live(reader: &mut BinaryReader<'_>) -> KernelResult<BTreeMap<EntityId, u32>> {
    reader.read_u32().and_then(|count| {
        (0..count).try_fold(BTreeMap::new(), |mut live, _| {
            EntityId::reflect_read(reader).and_then(|slot| {
                u32::reflect_read(reader).map(|generation| {
                    live.insert(slot, generation);
                    live
                })
            })
        })
    })
}

/// Read the free list: a count then that many handles.
fn read_free(reader: &mut BinaryReader<'_>) -> KernelResult<Vec<EntityHandle>> {
    reader.read_u32().and_then(|count| {
        (0..count).try_fold(Vec::new(), |mut free, _| {
            EntityHandle::reflect_read(reader).map(|handle| {
                free.push(handle);
                free
            })
        })
    })
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

    #[test]
    fn spawn_handle_is_live_and_current() {
        let mut reg = EntityRegistry::new();
        let handle = reg.spawn_handle();
        assert_eq!(handle.id().raw(), 1);
        assert_eq!(handle.generation(), 0);
        assert!(reg.is_current(handle));
        assert!(!reg.is_stale(handle));
        assert_eq!(reg.generation(handle.id()), Some(0));
    }

    #[test]
    fn despawn_invalidates_the_exact_handle() {
        let mut reg = EntityRegistry::new();
        let handle = reg.spawn_handle();
        assert!(reg.despawn_handle(handle));
        assert!(reg.is_stale(handle), "the handle is stale after despawn");
        assert!(!reg.is_current(handle));
        assert!(!reg.despawn_handle(handle));
        assert_eq!(reg.generation(handle.id()), None);
    }

    #[test]
    fn reused_slot_bumps_generation_and_staling_old_handle() {
        let mut reg = EntityRegistry::new();
        let first = reg.spawn_handle();
        assert!(reg.despawn(first.id()));
        let second = reg.spawn_handle();
        assert_eq!(second.id(), first.id(), "slot is reused");
        assert_eq!(second.generation(), 1, "generation is bumped");
        assert!(reg.is_current(second));
        assert!(reg.is_stale(first), "the pre-reuse handle is stale");
        assert!(!reg.despawn_handle(first));
        assert!(
            reg.is_current(second),
            "new occupant survives a stale despawn"
        );
    }

    #[test]
    fn fresh_slots_minted_when_free_list_empty() {
        let mut reg = EntityRegistry::new();
        let a = reg.spawn_handle();
        let b = reg.spawn_handle();
        assert_eq!(a.id().raw(), 1);
        assert_eq!(
            b.id().raw(),
            2,
            "no reuse available, so a fresh slot is minted"
        );
    }

    #[test]
    fn iter_handles_is_ascending_with_generations() {
        let mut reg = EntityRegistry::new();
        reg.spawn_handle();
        let two = reg.spawn_handle();
        reg.despawn(two.id());
        let reused = reg.spawn_handle();
        let handles: Vec<(u64, u32)> = reg
            .iter_handles()
            .map(|h| (h.id().raw(), h.generation()))
            .collect();
        assert_eq!(handles, vec![(1, 0), (2, reused.generation())]);
        assert_eq!(reused.generation(), 1);
    }

    #[test]
    fn serialize_round_trips_identity_state() {
        let mut reg = EntityRegistry::new();
        reg.spawn_handle();
        let two = reg.spawn_handle();
        reg.spawn_handle();
        reg.despawn(two.id());
        let mut writer = BinaryWriter::new();
        reg.serialize(&mut writer);
        let bytes = writer.into_bytes();
        let restored = EntityRegistry::deserialize(&mut BinaryReader::new(&bytes)).unwrap();

        let before: Vec<(u64, u32)> = reg
            .iter_handles()
            .map(|h| (h.id().raw(), h.generation()))
            .collect();
        let after: Vec<(u64, u32)> = restored
            .iter_handles()
            .map(|h| (h.id().raw(), h.generation()))
            .collect();
        assert_eq!(before, after, "live entities survive the round trip");
    }

    #[test]
    fn restored_registry_reproduces_future_spawns() {
        let mut original = EntityRegistry::new();
        original.spawn_handle();
        let two = original.spawn_handle();
        original.spawn_handle();
        original.despawn(two.id());

        let mut writer = BinaryWriter::new();
        original.serialize(&mut writer);
        let bytes = writer.into_bytes();
        let mut restored = EntityRegistry::deserialize(&mut BinaryReader::new(&bytes)).unwrap();

        let from_original = original.spawn_handle();
        let from_restored = restored.spawn_handle();
        assert_eq!(from_original.id(), from_restored.id());
        assert_eq!(from_original.generation(), from_restored.generation());
        assert_eq!(
            (from_restored.id().raw(), from_restored.generation()),
            (2, 1)
        );
    }

    #[test]
    fn deserialize_rejects_truncation_at_every_prefix() {
        let mut reg = EntityRegistry::new();
        reg.spawn_handle();
        let two = reg.spawn_handle();
        reg.despawn(two.id());
        let mut writer = BinaryWriter::new();
        reg.serialize(&mut writer);
        let bytes = writer.into_bytes();
        for len in 0..bytes.len() {
            assert!(EntityRegistry::deserialize(&mut BinaryReader::new(&bytes[..len])).is_err());
        }
    }
}
