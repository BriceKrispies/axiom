//! Applying proposed changes: `(snapshot, patch) -> snapshot`.

use std::collections::BTreeMap;

use crate::state_entry::StateEntry;
use crate::state_error::StateError;
use crate::state_error_code::StateErrorCode;
use crate::state_id::StateId;
use crate::state_op::StateOp;
use crate::state_patch::{detect_conflict, StatePatch};
use crate::state_payload::{parse_items, parse_rows, write_items, write_rows};
use crate::state_schema::StateSchema;
use crate::state_snapshot::StateSnapshot;
use crate::StateResult;

/// How one operation rewrites the payload it targets.
type Rewrite = fn(&StateEntry, &StateOp) -> StateResult<Vec<u8>>;

/// The applier, one entry per [`StateOpKind`], indexed by its discriminant.
///
/// This table is why the applier needs no `match`: dispatch is
/// `REWRITE[op.kind() as usize]`, which is a load rather than a branch. It is
/// also why there is no unreachable arm to leave uncovered — a table has
/// elements, not arms.
const REWRITE: [Rewrite; 8] = [
    set_cell,
    table_insert,
    table_update,
    table_remove,
    sequence_insert,
    sequence_replace,
    sequence_remove,
    sequence_append,
];

/// Apply a patch to a snapshot, producing a new snapshot.
///
/// Pure: the base snapshot is untouched and nothing is remembered between calls.
/// Validation runs first and rejects the whole patch — a patch is applied
/// entirely or not at all, so a caller can never be left holding a
/// half-transformed snapshot.
pub fn apply(
    schema: &StateSchema,
    base: &StateSnapshot,
    patch: &StatePatch,
) -> StateResult<StateSnapshot> {
    validate(schema, patch)
        .and_then(|()| {
            detect_conflict(core::slice::from_ref(patch)).map_or(Ok(()), |conflict| {
                Err(StateError::at(
                    StateErrorCode::ConflictingWrites,
                    conflict.state(),
                    "two origins proposed changes to the same state",
                ))
            })
        })
        .and_then(|()| {
            patch
                .ops()
                .iter()
                .try_fold(base.entries().clone(), apply_op)
        })
        .map(|entries| StateSnapshot::from_parts(base.schema_id(), base.version(), entries))
}

/// Combine several patches into one, rejecting conflicting writes.
///
/// Operations keep their relative order: every operation of the first patch,
/// then the second, and so on, which makes the result a deterministic function
/// of the input order.
pub fn merge(patches: &[StatePatch]) -> StateResult<StatePatch> {
    detect_conflict(patches)
        .map_or(Ok(()), |conflict| {
            Err(StateError::at(
                StateErrorCode::ConflictingWrites,
                conflict.state(),
                "two origins proposed changes to the same state",
            ))
        })
        .map(|()| {
            StatePatch::from_ops(
                patches
                    .iter()
                    .flat_map(|patch| patch.ops().iter().cloned())
                    .collect(),
            )
        })
}

/// Reject a patch that targets an undeclared state, or uses an operation that
/// does not belong to the target's storage shape.
fn validate(schema: &StateSchema, patch: &StatePatch) -> StateResult<()> {
    patch.ops().iter().try_fold((), |(), op| {
        schema.decl(op.target()).and_then(|decl| {
            (decl.kind() == op.kind().target_kind())
                .then_some(())
                .ok_or(StateError::at(
                    StateErrorCode::InvalidPatch,
                    op.target(),
                    "this operation does not apply to the target's storage shape",
                ))
        })
    })
}

/// Apply one operation to the working set of entries.
fn apply_op(
    mut entries: BTreeMap<StateId, StateEntry>,
    op: &StateOp,
) -> StateResult<BTreeMap<StateId, StateEntry>> {
    let target = op.target();
    entries
        .get(&target)
        .cloned()
        .ok_or(StateError::at(
            StateErrorCode::UnknownStateIdentity,
            target,
            "this snapshot holds no state with that identity",
        ))
        .and_then(|entry| {
            REWRITE[op.kind() as usize](&entry, op).map(|payload| entry.with_payload(payload))
        })
        .map(|updated| {
            entries.insert(target, updated);
            entries
        })
}

fn invalid_table(op: &StateOp, message: &'static str) -> StateError {
    StateError::at(StateErrorCode::InvalidTableOperation, op.target(), message)
}

fn invalid_sequence(op: &StateOp, message: &'static str) -> StateError {
    StateError::at(
        StateErrorCode::InvalidSequenceOperation,
        op.target(),
        message,
    )
}

/// Locate a row by its encoded key. Rows are stored in canonical key-byte order,
/// so this is a binary search.
fn find_row(rows: &[(Vec<u8>, Vec<u8>)], op: &StateOp) -> Result<usize, usize> {
    rows.binary_search_by(|(key, _)| key.as_slice().cmp(op.key()))
}

fn set_cell(_entry: &StateEntry, op: &StateOp) -> StateResult<Vec<u8>> {
    Ok(op.value().to_vec())
}

fn table_insert(entry: &StateEntry, op: &StateOp) -> StateResult<Vec<u8>> {
    parse_rows(entry.payload()).and_then(|mut rows| {
        find_row(&rows, op)
            .err()
            .ok_or(invalid_table(op, "insert onto a row that already exists"))
            .map(|at| {
                rows.insert(at, (op.key().to_vec(), op.value().to_vec()));
                write_rows(&rows)
            })
    })
}

fn table_update(entry: &StateEntry, op: &StateOp) -> StateResult<Vec<u8>> {
    parse_rows(entry.payload()).and_then(|mut rows| {
        find_row(&rows, op)
            .ok()
            .ok_or(invalid_table(op, "update of a row that does not exist"))
            .map(|at| {
                rows[at].1 = op.value().to_vec();
                write_rows(&rows)
            })
    })
}

fn table_remove(entry: &StateEntry, op: &StateOp) -> StateResult<Vec<u8>> {
    parse_rows(entry.payload()).and_then(|mut rows| {
        find_row(&rows, op)
            .ok()
            .ok_or(invalid_table(op, "removal of a row that does not exist"))
            .map(|at| {
                rows.remove(at);
                write_rows(&rows)
            })
    })
}

fn sequence_insert(entry: &StateEntry, op: &StateOp) -> StateResult<Vec<u8>> {
    parse_items(entry.payload()).and_then(|mut items| {
        let at = op.index() as usize;
        // Inserting AT the length appends, which is in range.
        (at <= items.len())
            .then_some(at)
            .ok_or(invalid_sequence(op, "insert past the end of the sequence"))
            .map(|at| {
                items.insert(at, op.value().to_vec());
                write_items(&items)
            })
    })
}

fn sequence_replace(entry: &StateEntry, op: &StateOp) -> StateResult<Vec<u8>> {
    parse_items(entry.payload()).and_then(|mut items| {
        let at = op.index() as usize;
        (at < items.len())
            .then_some(at)
            .ok_or(invalid_sequence(op, "replace of a position that does not exist"))
            .map(|at| {
                items[at] = op.value().to_vec();
                write_items(&items)
            })
    })
}

fn sequence_remove(entry: &StateEntry, op: &StateOp) -> StateResult<Vec<u8>> {
    parse_items(entry.payload()).and_then(|mut items| {
        let at = op.index() as usize;
        (at < items.len())
            .then_some(at)
            .ok_or(invalid_sequence(op, "removal of a position that does not exist"))
            .map(|at| {
                items.remove(at);
                write_items(&items)
            })
    })
}

fn sequence_append(entry: &StateEntry, op: &StateOp) -> StateResult<Vec<u8>> {
    parse_items(entry.payload()).map(|mut items| {
        items.push(op.value().to_vec());
        write_items(&items)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state_decl::StateDecl;
    use crate::state_key::{CellKey, SequenceKey, StateKey, TableKey};
    use crate::state_kind::StateKind;
    use crate::state_op_kind::StateOpKind;
    use crate::state_patch_builder::StatePatchBuilder;
    use crate::state_sequence::StateSequence;
    use crate::state_snapshot_builder::StateSnapshotBuilder;
    use crate::state_table::StateTable;
    use crate::StateOrigin;
    use axiom_kernel::SchemaVersion;

    struct Tick;
    impl StateKey for Tick {
        const PATH: &'static str = "apply/tick";
        const KIND: StateKind = StateKind::Cell;
    }
    impl CellKey for Tick {
        type Value = u64;
    }

    struct Rows;
    impl StateKey for Rows {
        const PATH: &'static str = "apply/rows";
        const KIND: StateKind = StateKind::Table;
    }
    impl TableKey for Rows {
        type Key = u32;
        type Value = u64;
    }

    struct Log;
    impl StateKey for Log {
        const PATH: &'static str = "apply/log";
        const KIND: StateKind = StateKind::Sequence;
    }
    impl SequenceKey for Log {
        type Item = u32;
    }

    /// Declared in the schema but never given a value — used to reach the
    /// "snapshot holds no such state" arm of the applier.
    struct Absent;
    impl StateKey for Absent {
        const PATH: &'static str = "apply/absent";
        const KIND: StateKind = StateKind::Cell;
    }
    impl CellKey for Absent {
        type Value = u64;
    }

    fn schema() -> StateSchema {
        StateSchema::build(
            "apply",
            SchemaVersion::new(1, 0),
            &[
                StateDecl::cell::<Tick>(),
                StateDecl::table::<Rows>(),
                StateDecl::sequence::<Log>(),
            ],
        )
        .expect("valid")
    }

    fn base() -> StateSnapshot {
        StateSnapshotBuilder::new(&schema())
            .with_cell::<Tick>(&1)
            .with_table::<Rows>(&StateTable::new().with(1, 10).with(2, 20))
            .with_sequence::<Log>(&StateSequence::new().appended(7).appended(8))
            .build()
            .expect("complete")
    }

    fn author() -> StatePatchBuilder {
        StatePatchBuilder::new(StateOrigin::of_name("test"))
    }

    #[test]
    fn setting_a_cell_produces_a_new_snapshot_and_leaves_the_base_alone() {
        let start = base();
        let patch = author().set_cell::<Tick>(&42).build().expect("valid");
        let next = apply(&schema(), &start, &patch).expect("applies");
        assert_eq!(next.cell::<Tick>(), Ok(42));
        assert_eq!(start.cell::<Tick>(), Ok(1), "the base snapshot is unchanged");
    }

    #[test]
    fn applying_the_same_patch_twice_to_the_same_base_gives_the_same_result() {
        let start = base();
        let patch = author().set_cell::<Tick>(&42).build().expect("valid");
        let once = apply(&schema(), &start, &patch).expect("applies");
        let twice = apply(&schema(), &start, &patch).expect("applies");
        assert_eq!(once, twice);
        assert_eq!(once.to_bytes(), twice.to_bytes());
    }

    #[test]
    fn an_empty_patch_leaves_the_snapshot_equal() {
        let start = base();
        let unchanged = apply(&schema(), &start, &StatePatch::default()).expect("applies");
        assert_eq!(unchanged, start);
    }

    #[test]
    fn several_non_conflicting_writes_all_land() {
        let patch = author()
            .set_cell::<Tick>(&5)
            .table_insert::<Rows>(&3, &30)
            .sequence_append::<Log>(&9)
            .build()
            .expect("valid");
        let next = apply(&schema(), &base(), &patch).expect("applies");
        assert_eq!(next.cell::<Tick>(), Ok(5));
        assert_eq!(next.table::<Rows>().expect("table").get(&3), Some(&30));
        assert_eq!(next.sequence::<Log>().expect("sequence").items(), &[7, 8, 9]);
    }

    #[test]
    fn every_table_operation_works() {
        let inserted = apply(
            &schema(),
            &base(),
            &author().table_insert::<Rows>(&3, &30).build().expect("valid"),
        )
        .expect("applies");
        assert_eq!(inserted.table::<Rows>().expect("table").len(), 3);

        let updated = apply(
            &schema(),
            &base(),
            &author().table_update::<Rows>(&1, &99).build().expect("valid"),
        )
        .expect("applies");
        assert_eq!(updated.table::<Rows>().expect("table").get(&1), Some(&99));

        let removed = apply(
            &schema(),
            &base(),
            &author().table_remove::<Rows>(&1).build().expect("valid"),
        )
        .expect("applies");
        assert!(!removed.table::<Rows>().expect("table").contains(&1));
    }

    #[test]
    fn inserting_an_existing_row_is_rejected() {
        let error = apply(
            &schema(),
            &base(),
            &author().table_insert::<Rows>(&1, &99).build().expect("valid"),
        )
        .unwrap_err();
        assert_eq!(error.code(), StateErrorCode::InvalidTableOperation);
        assert_eq!(error.state(), Rows::id());
    }

    #[test]
    fn updating_or_removing_a_missing_row_is_rejected() {
        [
            author().table_update::<Rows>(&404, &1).build().expect("valid"),
            author().table_remove::<Rows>(&404).build().expect("valid"),
        ]
        .into_iter()
        .for_each(|patch| {
            let error = apply(&schema(), &base(), &patch).unwrap_err();
            assert_eq!(error.code(), StateErrorCode::InvalidTableOperation);
        });
    }

    #[test]
    fn every_sequence_operation_works() {
        let inserted = apply(
            &schema(),
            &base(),
            &author().sequence_insert::<Log>(0, &1).build().expect("valid"),
        )
        .expect("applies");
        assert_eq!(inserted.sequence::<Log>().expect("seq").items(), &[1, 7, 8]);

        let at_end = apply(
            &schema(),
            &base(),
            &author().sequence_insert::<Log>(2, &1).build().expect("valid"),
        )
        .expect("inserting at the length appends");
        assert_eq!(at_end.sequence::<Log>().expect("seq").items(), &[7, 8, 1]);

        let replaced = apply(
            &schema(),
            &base(),
            &author().sequence_replace::<Log>(1, &99).build().expect("valid"),
        )
        .expect("applies");
        assert_eq!(replaced.sequence::<Log>().expect("seq").items(), &[7, 99]);

        let removed = apply(
            &schema(),
            &base(),
            &author().sequence_remove::<Log>(0).build().expect("valid"),
        )
        .expect("applies");
        assert_eq!(removed.sequence::<Log>().expect("seq").items(), &[8]);
    }

    #[test]
    fn sequence_positions_out_of_range_are_rejected() {
        [
            author().sequence_insert::<Log>(3, &1).build().expect("valid"),
            author().sequence_replace::<Log>(2, &1).build().expect("valid"),
            author().sequence_remove::<Log>(2).build().expect("valid"),
        ]
        .into_iter()
        .for_each(|patch| {
            let error = apply(&schema(), &base(), &patch).unwrap_err();
            assert_eq!(error.code(), StateErrorCode::InvalidSequenceOperation);
            assert_eq!(error.state(), Log::id());
        });
    }

    #[test]
    fn an_operation_against_the_wrong_storage_shape_is_rejected() {
        // A sequence append aimed at the cell `Tick`.
        let op = StateOp::new(
            StateOpKind::SequenceAppend,
            Tick::id(),
            StateOrigin::ANONYMOUS,
            vec![],
            0,
            vec![],
        );
        let error = apply(&schema(), &base(), &StatePatch::from_ops(vec![op])).unwrap_err();
        assert_eq!(error.code(), StateErrorCode::InvalidPatch);
        assert_eq!(error.state(), Tick::id());
    }

    #[test]
    fn an_operation_against_an_undeclared_state_is_rejected() {
        let op = StateOp::new(
            StateOpKind::SetCell,
            StateId::of_path("apply/nowhere"),
            StateOrigin::ANONYMOUS,
            vec![],
            0,
            vec![1],
        );
        let error = apply(&schema(), &base(), &StatePatch::from_ops(vec![op])).unwrap_err();
        assert_eq!(error.code(), StateErrorCode::UnknownStateIdentity);
    }

    #[test]
    fn an_operation_against_a_state_the_snapshot_does_not_hold_is_rejected() {
        // Declared by the schema, so validation passes, but absent from this
        // snapshot — the applier's own missing-entry arm.
        let wider = StateSchema::build(
            "apply",
            SchemaVersion::new(1, 0),
            &[
                StateDecl::cell::<Tick>(),
                StateDecl::table::<Rows>(),
                StateDecl::sequence::<Log>(),
                StateDecl::cell::<Absent>(),
            ],
        )
        .expect("valid");
        let op = StateOp::new(
            StateOpKind::SetCell,
            Absent::id(),
            StateOrigin::ANONYMOUS,
            vec![],
            0,
            vec![1],
        );
        let error = apply(&wider, &base(), &StatePatch::from_ops(vec![op])).unwrap_err();
        assert_eq!(error.code(), StateErrorCode::UnknownStateIdentity);
        assert_eq!(error.state(), Absent::id());
    }

    #[test]
    fn conflicting_writes_inside_one_patch_are_rejected() {
        let mixed = StatePatch::from_ops(vec![
            StateOp::new(
                StateOpKind::SetCell,
                Tick::id(),
                StateOrigin::of_name("a"),
                vec![],
                0,
                vec![4, 0, 0, 0, 0, 0, 0, 0],
            ),
            StateOp::new(
                StateOpKind::SetCell,
                Tick::id(),
                StateOrigin::of_name("b"),
                vec![],
                0,
                vec![5, 0, 0, 0, 0, 0, 0, 0],
            ),
        ]);
        let error = apply(&schema(), &base(), &mixed).unwrap_err();
        assert_eq!(error.code(), StateErrorCode::ConflictingWrites);
        assert_eq!(error.state(), Tick::id());
    }

    #[test]
    fn a_rejected_patch_changes_nothing_at_all() {
        let start = base();
        let doomed = author()
            .set_cell::<Tick>(&42)
            .table_update::<Rows>(&404, &1)
            .build()
            .expect("valid to author");
        assert!(apply(&schema(), &start, &doomed).is_err());
        assert_eq!(start.cell::<Tick>(), Ok(1), "the base is untouched");
    }

    #[test]
    fn merging_non_conflicting_patches_concatenates_them_in_order() {
        let merged = merge(&[
            StatePatchBuilder::new(StateOrigin::of_name("a"))
                .set_cell::<Tick>(&2)
                .build()
                .expect("valid"),
            StatePatchBuilder::new(StateOrigin::of_name("b"))
                .table_insert::<Rows>(&3, &30)
                .build()
                .expect("valid"),
        ])
        .expect("no conflict");
        assert_eq!(merged.len(), 2);
        assert_eq!(merged.ops()[0].kind(), StateOpKind::SetCell);
        assert_eq!(merged.ops()[1].kind(), StateOpKind::TableInsert);
        let next = apply(&schema(), &base(), &merged).expect("applies");
        assert_eq!(next.cell::<Tick>(), Ok(2));
        assert_eq!(next.table::<Rows>().expect("table").len(), 3);
    }

    #[test]
    fn merging_conflicting_patches_is_rejected() {
        let error = merge(&[
            StatePatchBuilder::new(StateOrigin::of_name("a"))
                .set_cell::<Tick>(&2)
                .build()
                .expect("valid"),
            StatePatchBuilder::new(StateOrigin::of_name("b"))
                .set_cell::<Tick>(&3)
                .build()
                .expect("valid"),
        ])
        .unwrap_err();
        assert_eq!(error.code(), StateErrorCode::ConflictingWrites);
        assert_eq!(error.state(), Tick::id());
    }

    #[test]
    fn merging_nothing_produces_an_empty_patch() {
        assert!(merge(&[]).expect("no conflict").is_empty());
    }

    #[test]
    fn a_corrupt_payload_is_reported_rather_than_panicking() {
        // A table entry whose payload is not a valid row list.
        let mut entries = base().entries().clone();
        let broken = entries
            .get(&Rows::id())
            .expect("rows exist")
            .with_payload(vec![9]);
        entries.insert(Rows::id(), broken);
        let damaged = StateSnapshot::from_parts(
            base().schema_id(),
            base().version(),
            entries,
        );
        let error = apply(
            &schema(),
            &damaged,
            &author().table_insert::<Rows>(&5, &50).build().expect("valid"),
        )
        .unwrap_err();
        assert_eq!(error.code(), StateErrorCode::CorruptedSnapshot);
    }
}
