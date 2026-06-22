//! A generational entity handle: a stable slot plus the generation it held.

use axiom_kernel::{
    BinaryReader, BinaryWriter, EntityId, FieldSchema, KernelResult, Reflect, TypeSchema,
};

/// A handle to an entity: the entity's stable **slot** (an [`EntityId`]) paired
/// with the **generation** that slot carried when the handle was minted.
///
/// The slot is reused after despawn (see [`crate::EntityRegistry`]); the
/// generation distinguishes successive occupants of the same slot. A handle is
/// *current* only while the registry's live generation for its slot still equals
/// the handle's generation, so a handle taken before a despawn/reuse is detectably
/// stale and cannot be confused with the new occupant.
///
/// Ordering is by slot, then generation — fully deterministic — so handles sort
/// and serialize stably. The handle composes the kernel's [`EntityId`]; it does
/// not modify or replace it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EntityHandle {
    slot: EntityId,
    generation: u32,
}

impl EntityHandle {
    /// Construct a handle for `slot` at `generation`.
    pub const fn new(slot: EntityId, generation: u32) -> Self {
        EntityHandle { slot, generation }
    }

    /// The entity's stable slot id (the component-storage key).
    pub const fn id(self) -> EntityId {
        self.slot
    }

    /// The generation this handle was minted at.
    pub const fn generation(self) -> u32 {
        self.generation
    }
}

impl Reflect for EntityHandle {
    const SCHEMA: TypeSchema = TypeSchema::new(
        "EntityHandle",
        &[
            FieldSchema::new("slot", "u64"),
            FieldSchema::new("generation", "u32"),
        ],
    );

    fn reflect_write(&self, writer: &mut BinaryWriter) {
        self.slot.reflect_write(writer);
        self.generation.reflect_write(writer);
    }

    fn reflect_read(reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
        EntityId::reflect_read(reader).and_then(|slot| {
            u32::reflect_read(reader).map(|generation| EntityHandle::new(slot, generation))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn h(slot: u64, generation: u32) -> EntityHandle {
        EntityHandle::new(EntityId::from_raw(slot), generation)
    }

    #[test]
    fn carries_slot_and_generation() {
        let handle = h(7, 3);
        assert_eq!(handle.id(), EntityId::from_raw(7));
        assert_eq!(handle.generation(), 3);
    }

    #[test]
    fn ordering_is_by_slot_then_generation() {
        assert!(
            h(1, 9) < h(2, 0),
            "lower slot sorts first regardless of generation"
        );
        assert!(h(2, 0) < h(2, 1), "same slot orders by generation");
        assert_eq!(h(4, 5), h(4, 5));
        assert_ne!(h(4, 5), h(4, 6));
    }

    #[test]
    fn hashes_by_identity() {
        let mut set = HashSet::new();
        set.insert(h(1, 0));
        set.insert(h(1, 0));
        set.insert(h(1, 1));
        assert_eq!(
            set.len(),
            2,
            "same slot+generation collapses; different generation is distinct"
        );
    }

    #[test]
    fn reflect_round_trips() {
        let handle = h(0x0102_0304_0506_0708, 0x0900_000A);
        let mut writer = BinaryWriter::new();
        handle.reflect_write(&mut writer);
        let bytes = writer.into_bytes();
        let decoded = EntityHandle::reflect_read(&mut BinaryReader::new(&bytes)).unwrap();
        assert_eq!(decoded, handle);
        assert_eq!(EntityHandle::SCHEMA.name(), "EntityHandle");
    }

    #[test]
    fn reflect_rejects_truncation_at_every_prefix() {
        let handle = h(5, 6);
        let mut writer = BinaryWriter::new();
        handle.reflect_write(&mut writer);
        let bytes = writer.into_bytes();
        for len in 0..bytes.len() {
            assert!(EntityHandle::reflect_read(&mut BinaryReader::new(&bytes[..len])).is_err());
        }
    }
}
