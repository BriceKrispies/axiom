//! One declared state slot, as data.

use crate::state_id::StateId;
use crate::state_key::{CellKey, SequenceKey, TableKey};
use crate::state_kind::StateKind;
use crate::state_shape_id::StateShapeId;

/// A declaration: a path, its identity, its storage shape, and the shape of the
/// values it holds.
///
/// This is the runtime residue of a compile-time key. A key type is erased into
/// one of these so a schema can be a plain `Vec` of declarations that tooling can
/// walk, serialize, and diff without naming any game type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateDecl {
    path: &'static str,
    id: StateId,
    kind: StateKind,
    shape: StateShapeId,
}

impl StateDecl {
    /// Declare a cell slot from its key type.
    pub fn cell<K: CellKey>() -> Self {
        StateDecl {
            path: K::PATH,
            id: K::id(),
            kind: StateKind::Cell,
            shape: <K as CellKey>::shape(),
        }
    }

    /// Declare a table slot from its key type.
    pub fn table<K: TableKey>() -> Self {
        StateDecl {
            path: K::PATH,
            id: K::id(),
            kind: StateKind::Table,
            shape: <K as TableKey>::shape(),
        }
    }

    /// Declare a sequence slot from its key type.
    pub fn sequence<K: SequenceKey>() -> Self {
        StateDecl {
            path: K::PATH,
            id: K::id(),
            kind: StateKind::Sequence,
            shape: <K as SequenceKey>::shape(),
        }
    }

    /// The declared path — the string the identity is derived from.
    pub const fn path(&self) -> &'static str {
        self.path
    }

    /// The slot's stable identity.
    pub const fn id(&self) -> StateId {
        self.id
    }

    /// The slot's storage shape.
    pub const fn kind(&self) -> StateKind {
        self.kind
    }

    /// The shape identity of the values this slot holds.
    pub const fn shape(&self) -> StateShapeId {
        self.shape
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state_key::StateKey;

    struct Tick;
    impl StateKey for Tick {
        const PATH: &'static str = "test/tick";
        const KIND: StateKind = StateKind::Cell;
    }
    impl CellKey for Tick {
        type Value = u64;
    }

    struct Rows;
    impl StateKey for Rows {
        const PATH: &'static str = "test/rows";
        const KIND: StateKind = StateKind::Table;
    }
    impl TableKey for Rows {
        type Key = u32;
        type Value = u64;
    }

    struct Log;
    impl StateKey for Log {
        const PATH: &'static str = "test/log";
        const KIND: StateKind = StateKind::Sequence;
    }
    impl SequenceKey for Log {
        type Item = u32;
    }

    #[test]
    fn a_cell_declaration_carries_its_path_identity_kind_and_shape() {
        let decl = StateDecl::cell::<Tick>();
        assert_eq!(decl.path(), "test/tick");
        assert_eq!(decl.id(), StateId::of_path("test/tick"));
        assert_eq!(decl.kind(), StateKind::Cell);
        assert_eq!(decl.shape(), StateShapeId::cell_of::<u64>());
    }

    #[test]
    fn a_table_declaration_carries_both_of_its_types() {
        let decl = StateDecl::table::<Rows>();
        assert_eq!(decl.kind(), StateKind::Table);
        assert_eq!(decl.shape(), StateShapeId::table_of::<u32, u64>());
    }

    #[test]
    fn a_sequence_declaration_carries_its_item_type() {
        let decl = StateDecl::sequence::<Log>();
        assert_eq!(decl.kind(), StateKind::Sequence);
        assert_eq!(decl.shape(), StateShapeId::sequence_of::<u32>());
    }

    #[test]
    fn declarations_of_different_slots_differ() {
        assert_ne!(StateDecl::cell::<Tick>(), StateDecl::table::<Rows>());
    }
}
