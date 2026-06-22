//! A deterministic, ECS-owned identity for a component type.

use axiom_kernel::Reflect;

/// A stable numeric identity for a component type, derived deterministically from
/// the type's [`Reflect`] schema name.
///
/// This is the durable identity seam future query/change/replay work keys on,
/// instead of carrying schema-name strings around. It is a pure function of the
/// declared type schema — the same type yields the same id on every run and every
/// platform, with no dependence on allocation order, wall-clock time, or
/// randomness. It is an ECS concept and stays in this layer; the kernel is never
/// taught about it.
///
/// Identity is by the type's declared schema *name*, so two distinct types that
/// declare the same schema name share an id — the same deliberate trade
/// [`crate::DynamicComponents`] makes; a type's schema name is its durable
/// identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ComponentTypeId(u64);

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01B3;

/// FNV-1a over bytes. Deterministic, allocation-free, no external state.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(FNV_OFFSET, |hash, byte| {
        (hash ^ *byte as u64).wrapping_mul(FNV_PRIME)
    })
}

impl ComponentTypeId {
    /// The id of component type `T`, hashed from its [`Reflect`] schema name.
    pub fn of<T: Reflect>() -> Self {
        ComponentTypeId(fnv1a(T::SCHEMA.name().as_bytes()))
    }

    /// The raw 64-bit value, for serialization.
    pub const fn raw(self) -> u64 {
        self.0
    }

    /// Reconstruct a type id from a raw value produced by [`Self::raw`].
    pub const fn from_raw(raw: u64) -> Self {
        ComponentTypeId(raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::{BinaryReader, BinaryWriter, FieldSchema, KernelResult, TypeSchema};
    use std::collections::HashSet;

    struct Position;
    impl Reflect for Position {
        const SCHEMA: TypeSchema = TypeSchema::new("Position", &[FieldSchema::new("x", "u32")]);
        fn reflect_write(&self, _writer: &mut BinaryWriter) {}
        fn reflect_read(_reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
            Ok(Position)
        }
    }
    struct Velocity;
    impl Reflect for Velocity {
        const SCHEMA: TypeSchema = TypeSchema::new("Velocity", &[]);
        fn reflect_write(&self, _writer: &mut BinaryWriter) {}
        fn reflect_read(_reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
            Ok(Velocity)
        }
    }

    #[test]
    fn id_is_stable_for_a_type() {
        assert_eq!(
            ComponentTypeId::of::<Position>(),
            ComponentTypeId::of::<Position>()
        );
    }

    #[test]
    fn distinct_types_have_distinct_ids() {
        assert_ne!(
            ComponentTypeId::of::<Position>(),
            ComponentTypeId::of::<Velocity>()
        );
    }

    #[test]
    fn matches_a_direct_schema_name_hash() {
        assert_eq!(
            ComponentTypeId::of::<Position>().raw(),
            fnv1a("Position".as_bytes())
        );
    }

    #[test]
    fn raw_round_trips() {
        let id = ComponentTypeId::of::<Velocity>();
        assert_eq!(ComponentTypeId::from_raw(id.raw()), id);
    }

    #[test]
    fn helper_reflect_impls_write_nothing_and_read_back() {
        // The id is derived purely from the schema name, so these helper
        // components carry empty bodies; assert that documented contract.
        let mut writer = BinaryWriter::new();
        Position.reflect_write(&mut writer);
        Velocity.reflect_write(&mut writer);
        assert!(writer.as_bytes().is_empty());
        let mut reader = BinaryReader::new(writer.as_bytes());
        assert!(Position::reflect_read(&mut reader).is_ok());
        assert!(Velocity::reflect_read(&mut reader).is_ok());
    }

    #[test]
    fn ordering_and_hashing_are_stable() {
        let a = ComponentTypeId::from_raw(1);
        let b = ComponentTypeId::from_raw(2);
        assert!(a < b);
        let mut set = HashSet::new();
        set.insert(a);
        set.insert(a);
        set.insert(b);
        assert_eq!(set.len(), 2);
    }
}
