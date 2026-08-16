//! Stable identity for a field graph and for one slot of its parameter table.
//!
//! Both ids are minted the way the `state` layer mints a [`crate::FieldId`]'s
//! cousin (`StateId::of_path`): from a **name**, through the kernel's
//! [`StableHash`]. The name is an authoring convenience and never enters the wire
//! format — a serialized field carries the dense slot index only.

use axiom_kernel::StableHash;

/// The stable identity of one field graph.
///
/// A 64-bit digest, so it is safe to put in a serialized artifact, a golden, a
/// diff or a replay: it is derived from the author's name for the field, never
/// from an address, an insertion order, a randomized hash, or `TypeId`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FieldId(u64);

impl FieldId {
    /// The identity of a named field, e.g. `"material/asphalt"`.
    pub fn of_name(name: &str) -> Self {
        FieldId(StableHash::of_bytes(name.as_bytes()).raw())
    }

    /// Rebuild an identity from its raw digest (decoding a stored artifact).
    pub const fn from_raw(raw: u64) -> Self {
        FieldId(raw)
    }

    /// The raw 64-bit digest.
    pub const fn raw(self) -> u64 {
        self.0
    }
}

/// One slot of a field's parameter table: a dense index, `0..params.len()`.
///
/// A slot index — not a name and not a digest — is what a `Param` node carries,
/// which is exactly why **changing a parameter's value cannot change the graph's
/// structure**. The name→slot map lives authoring-side in
/// [`crate::FieldBuilder`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FieldParamSlot(u16);

impl FieldParamSlot {
    /// Construct from a raw dense index.
    pub const fn from_raw(raw: u16) -> Self {
        FieldParamSlot(raw)
    }

    /// The raw dense index.
    pub const fn raw(self) -> u16 {
        self.0
    }

    /// The index as a `usize`, for indexing the parameter table.
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_field_id_is_the_digest_of_its_name() {
        assert_eq!(FieldId::of_name("material/asphalt"), FieldId::of_name("material/asphalt"));
        assert_ne!(FieldId::of_name("material/asphalt"), FieldId::of_name("material/marble"));
        assert_eq!(
            FieldId::of_name("material/asphalt").raw(),
            StableHash::of_bytes(b"material/asphalt").raw()
        );
    }

    #[test]
    fn a_field_id_round_trips_through_its_raw_digest() {
        let id = FieldId::of_name("material/rust");
        assert_eq!(FieldId::from_raw(id.raw()), id);
        assert!(FieldId::from_raw(1) < FieldId::from_raw(2));
    }

    #[test]
    fn a_slot_is_a_dense_index() {
        let slot = FieldParamSlot::from_raw(7);
        assert_eq!(slot.raw(), 7);
        assert_eq!(slot.index(), 7);
        assert!(FieldParamSlot::from_raw(1) < FieldParamSlot::from_raw(2));
        assert_ne!(FieldParamSlot::from_raw(1), FieldParamSlot::from_raw(2));
    }
}
