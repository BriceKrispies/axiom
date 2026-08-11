//! The shape identity of a stored state's value type.

use axiom_kernel::{Reflect, StableHash, TypeSchema};

use crate::state_kind::StateKind;

/// A digest of the *shape* stored under a state identity: the storage kind plus
/// the `TypeSchema` of every type involved.
///
/// This is what catches a mismatch at a trust boundary — bytes arriving from
/// disk, from a golden artifact, or from another process — where the compiler
/// can prove nothing. Within one process a mismatch is impossible, because the
/// identity and the value type come from the same key type.
///
/// The digest folds the type's name **and each of its fields' name and type
/// name**, so reordering or renaming a field changes the identity even when the
/// type name does not. `TypeSchema` is flat, so this guard is one level deep: a
/// nested composite whose *inner* shape changed while its own name and field
/// type-names stayed the same is not distinguished. Deepening that would be a
/// kernel change to `TypeSchema`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StateShapeId(u64);

/// Fold one type's schema into a digest word.
fn schema_word(schema: TypeSchema) -> u64 {
    let head = StableHash::of_bytes(schema.name().as_bytes()).raw();
    schema.fields().iter().fold(head, |acc, field| {
        StableHash::of_words(&[
            acc,
            StableHash::of_bytes(field.name().as_bytes()).raw(),
            StableHash::of_bytes(field.type_name().as_bytes()).raw(),
        ])
        .raw()
    })
}

impl StateShapeId {
    /// The shape identity of a cell holding `T`.
    pub fn cell_of<T: Reflect>() -> Self {
        StateShapeId(
            StableHash::of_words(&[StateKind::Cell.code().into(), schema_word(T::SCHEMA)]).raw(),
        )
    }

    /// The shape identity of a table mapping `K` to `V`.
    pub fn table_of<K: Reflect, V: Reflect>() -> Self {
        StateShapeId(
            StableHash::of_words(&[
                StateKind::Table.code().into(),
                schema_word(K::SCHEMA),
                schema_word(V::SCHEMA),
            ])
            .raw(),
        )
    }

    /// The shape identity of a sequence of `T`.
    pub fn sequence_of<T: Reflect>() -> Self {
        StateShapeId(
            StableHash::of_words(&[StateKind::Sequence.code().into(), schema_word(T::SCHEMA)])
                .raw(),
        )
    }

    /// Rebuild from a raw digest (decoding a stored snapshot).
    pub const fn from_raw(raw: u64) -> Self {
        StateShapeId(raw)
    }

    /// The raw 64-bit digest.
    pub const fn raw(self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::{BinaryReader, BinaryWriter, FieldSchema, KernelResult};

    /// A two-field composite, so the field-folding path is exercised.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct Pair {
        left: u32,
        right: u32,
    }

    impl Reflect for Pair {
        const SCHEMA: TypeSchema = TypeSchema::new(
            "Pair",
            &[
                FieldSchema::new("left", "u32"),
                FieldSchema::new("right", "u32"),
            ],
        );

        fn reflect_write(&self, writer: &mut BinaryWriter) {
            writer.write_u32(self.left);
            writer.write_u32(self.right);
        }

        fn reflect_read(reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
            reader
                .read_u32()
                .and_then(|left| reader.read_u32().map(|right| Pair { left, right }))
        }
    }

    /// The same fields in the other order — must digest differently.
    #[derive(Debug)]
    struct Swapped;

    impl Reflect for Swapped {
        const SCHEMA: TypeSchema = TypeSchema::new(
            "Pair",
            &[
                FieldSchema::new("right", "u32"),
                FieldSchema::new("left", "u32"),
            ],
        );

        fn reflect_write(&self, _writer: &mut BinaryWriter) {}

        fn reflect_read(_reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
            Ok(Swapped)
        }
    }

    #[test]
    fn the_same_shape_always_digests_the_same_way() {
        assert_eq!(StateShapeId::cell_of::<Pair>(), StateShapeId::cell_of::<Pair>());
    }

    #[test]
    fn the_storage_kind_is_part_of_the_shape() {
        assert_ne!(
            StateShapeId::cell_of::<u32>(),
            StateShapeId::sequence_of::<u32>()
        );
    }

    #[test]
    fn different_value_types_digest_differently() {
        assert_ne!(StateShapeId::cell_of::<u32>(), StateShapeId::cell_of::<u64>());
    }

    #[test]
    fn a_table_distinguishes_its_key_from_its_value() {
        assert_ne!(
            StateShapeId::table_of::<u32, u64>(),
            StateShapeId::table_of::<u64, u32>()
        );
    }

    #[test]
    fn reordering_fields_changes_the_shape_even_under_the_same_type_name() {
        assert_ne!(
            StateShapeId::cell_of::<Pair>(),
            StateShapeId::cell_of::<Swapped>()
        );
    }

    #[test]
    fn shape_identity_round_trips_through_its_raw_digest() {
        let id = StateShapeId::table_of::<u32, Pair>();
        assert_eq!(StateShapeId::from_raw(id.raw()), id);
    }
}
