//! Constructing the first snapshot, once, from declared values.

use std::collections::BTreeMap;

use axiom_kernel::SchemaVersion;

use crate::state_entry::StateEntry;
use crate::state_error::StateError;
use crate::state_error_code::StateErrorCode;
use crate::state_id::StateId;
use crate::state_key::{CellKey, SequenceKey, TableKey};
use crate::state_kind::StateKind;
use crate::state_payload::{encode_cell, encode_sequence, encode_table};
use crate::state_schema::StateSchema;
use crate::state_schema_id::StateSchemaId;
use crate::state_sequence::StateSequence;
use crate::state_snapshot::StateSnapshot;
use crate::state_table::StateTable;
use crate::state_shape_id::StateShapeId;
use crate::StateResult;

/// Builds the initial [`StateSnapshot`] for a schema.
///
/// The builder consumes and returns itself, so no caller ever holds a mutable
/// reference into a snapshot under construction. Faults are collected rather
/// than returned per step, which is what lets the whole chain read as one
/// expression and still fail with the *first* problem at [`Self::build`].
///
/// There is deliberately no `StateSnapshot::empty()`. The substrate cannot
/// invent a starting value for a state it knows nothing about; requiring
/// `Default` would be the engine assuming something about game meaning. Every
/// declared state must be supplied exactly once, and a missing one is an
/// `IncompleteSnapshot` naming the slot.
#[derive(Debug, Clone)]
pub struct StateSnapshotBuilder {
    schema_id: StateSchemaId,
    version: SchemaVersion,
    declared: Vec<(StateId, StateKind, StateShapeId)>,
    entries: BTreeMap<StateId, StateEntry>,
    fault: Option<StateError>,
}

impl StateSnapshotBuilder {
    /// Start building a snapshot for `schema`.
    pub fn new(schema: &StateSchema) -> Self {
        StateSnapshotBuilder {
            schema_id: schema.identity(),
            version: schema.version(),
            declared: schema
                .decls()
                .iter()
                .map(|decl| (decl.id(), decl.kind(), decl.shape()))
                .collect(),
            entries: BTreeMap::new(),
            fault: None,
        }
    }

    /// Record a value for a declared slot, or the first fault that prevents it.
    fn set(mut self, id: StateId, kind: StateKind, shape: StateShapeId, payload: Vec<u8>) -> Self {
        let expected = self
            .declared
            .iter()
            .find(|(declared, _, _)| *declared == id)
            .copied();
        let fault = expected.map_or(
            Some(StateError::at(
                StateErrorCode::UnknownStateIdentity,
                id,
                "this schema declares no state with that identity",
            )),
            |(_, declared_kind, declared_type)| {
                ((declared_kind == kind) & (declared_type == shape))
                    .then_some(())
                    .map_or(
                        Some(StateError::at(
                            StateErrorCode::StateTypeMismatch,
                            id,
                            "the value's shape does not match the declaration",
                        )),
                        |()| None,
                    )
            },
        );
        // Record the entry regardless; `build` reports the first fault, and a
        // faulted builder never produces a snapshot.
        fault
            .is_none()
            .then(|| self.entries.insert(id, StateEntry::new(kind, shape, payload)));
        self.fault = self.fault.or(fault);
        self
    }

    /// Supply a cell's value.
    pub fn with_cell<K: CellKey>(self, value: &K::Value) -> Self {
        self.set(
            K::id(),
            StateKind::Cell,
            <K as CellKey>::shape(),
            encode_cell(value),
        )
    }

    /// Supply a table's rows.
    pub fn with_table<K: TableKey>(self, table: &StateTable<K::Key, K::Value>) -> Self {
        self.set(
            K::id(),
            StateKind::Table,
            <K as TableKey>::shape(),
            encode_table(table),
        )
    }

    /// Supply a sequence's items.
    pub fn with_sequence<K: SequenceKey>(self, sequence: &StateSequence<K::Item>) -> Self {
        self.set(
            K::id(),
            StateKind::Sequence,
            <K as SequenceKey>::shape(),
            encode_sequence(sequence),
        )
    }

    /// Finish, or report the first thing that went wrong.
    pub fn build(self) -> StateResult<StateSnapshot> {
        self.fault.map_or_else(
            || {
                self.declared
                    .iter()
                    .find(|(id, _, _)| !self.entries.contains_key(id))
                    .map_or(Ok(()), |(id, _, _)| {
                        Err(StateError::at(
                            StateErrorCode::IncompleteSnapshot,
                            *id,
                            "a declared state was never given a value",
                        ))
                    })
                    .map(|()| {
                        StateSnapshot::from_parts(
                            self.schema_id,
                            self.version,
                            self.entries.clone(),
                        )
                    })
            },
            Err,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state_decl::StateDecl;
    use crate::state_key::StateKey;

    struct Tick;
    impl StateKey for Tick {
        const PATH: &'static str = "build/tick";
        const KIND: StateKind = StateKind::Cell;
    }
    impl CellKey for Tick {
        type Value = u64;
    }

    struct Rows;
    impl StateKey for Rows {
        const PATH: &'static str = "build/rows";
        const KIND: StateKind = StateKind::Table;
    }
    impl TableKey for Rows {
        type Key = u32;
        type Value = u64;
    }

    struct Log;
    impl StateKey for Log {
        const PATH: &'static str = "build/log";
        const KIND: StateKind = StateKind::Sequence;
    }
    impl SequenceKey for Log {
        type Item = u32;
    }

    /// Declared nowhere — used to reach the undeclared-slot fault.
    struct Undeclared;
    impl StateKey for Undeclared {
        const PATH: &'static str = "build/undeclared";
        const KIND: StateKind = StateKind::Cell;
    }
    impl CellKey for Undeclared {
        type Value = u64;
    }

    /// Same path as `Tick`, different value type — reaches the shape fault.
    struct TickAsU32;
    impl StateKey for TickAsU32 {
        const PATH: &'static str = "build/tick";
        const KIND: StateKind = StateKind::Cell;
    }
    impl CellKey for TickAsU32 {
        type Value = u32;
    }

    fn schema() -> StateSchema {
        StateSchema::build(
            "build",
            SchemaVersion::new(1, 0),
            &[
                StateDecl::cell::<Tick>(),
                StateDecl::table::<Rows>(),
                StateDecl::sequence::<Log>(),
            ],
        )
        .expect("valid schema")
    }

    fn complete() -> StateSnapshotBuilder {
        StateSnapshotBuilder::new(&schema())
            .with_cell::<Tick>(&1)
            .with_table::<Rows>(&StateTable::new().with(1, 10))
            .with_sequence::<Log>(&StateSequence::new().appended(2))
    }

    #[test]
    fn a_complete_snapshot_builds_and_carries_the_schema_identity() {
        let snapshot = complete().build().expect("complete");
        assert_eq!(snapshot.schema_id(), schema().identity());
        assert_eq!(snapshot.version(), SchemaVersion::new(1, 0));
        assert_eq!(snapshot.len(), 3);
        assert_eq!(snapshot.cell::<Tick>(), Ok(1));
    }

    #[test]
    fn a_schema_with_no_declarations_builds_an_empty_snapshot() {
        let empty = StateSchema::build("none", SchemaVersion::new(1, 0), &[]).expect("valid");
        assert!(StateSnapshotBuilder::new(&empty)
            .build()
            .expect("nothing to set")
            .is_empty());
    }

    #[test]
    fn a_missing_declared_state_is_incomplete_and_names_the_slot() {
        let error = StateSnapshotBuilder::new(&schema())
            .with_cell::<Tick>(&1)
            .build()
            .unwrap_err();
        assert_eq!(error.code(), StateErrorCode::IncompleteSnapshot);
        assert!([Rows::id(), Log::id()].contains(&error.state()));
    }

    #[test]
    fn setting_an_undeclared_state_is_rejected() {
        let error = complete()
            .with_cell::<Undeclared>(&1)
            .build()
            .unwrap_err();
        assert_eq!(error.code(), StateErrorCode::UnknownStateIdentity);
        assert_eq!(error.state(), Undeclared::id());
    }

    #[test]
    fn setting_a_declared_state_with_the_wrong_shape_is_rejected() {
        let error = complete()
            .with_cell::<TickAsU32>(&1)
            .build()
            .unwrap_err();
        assert_eq!(error.code(), StateErrorCode::StateTypeMismatch);
        assert_eq!(error.state(), Tick::id());
    }

    #[test]
    fn the_first_fault_is_the_one_reported() {
        let error = StateSnapshotBuilder::new(&schema())
            .with_cell::<Undeclared>(&1)
            .with_cell::<TickAsU32>(&1)
            .build()
            .unwrap_err();
        assert_eq!(error.code(), StateErrorCode::UnknownStateIdentity);
    }

    #[test]
    fn supplying_a_state_twice_keeps_the_last_value() {
        let snapshot = complete().with_cell::<Tick>(&42).build().expect("complete");
        assert_eq!(snapshot.cell::<Tick>(), Ok(42));
    }

    #[test]
    fn building_the_same_values_twice_gives_identical_snapshots() {
        assert_eq!(
            complete().build().expect("complete"),
            complete().build().expect("complete")
        );
        assert_eq!(
            complete().build().expect("complete").to_bytes(),
            complete().build().expect("complete").to_bytes()
        );
    }
}
