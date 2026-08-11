//! Describing a snapshot to tooling, without decoding a single game value.

use axiom_kernel::{SchemaVersion, StableHash};

use crate::state_id::StateId;
use crate::state_kind::StateKind;
use crate::state_payload::{parse_items, parse_rows};
use crate::state_schema::StateSchema;
use crate::state_schema_id::StateSchemaId;
use crate::state_shape_id::StateShapeId;
use crate::state_snapshot::StateSnapshot;
use crate::StateResult;

/// What one stored state looks like from outside.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateEntryReport {
    id: StateId,
    path: &'static str,
    kind: StateKind,
    shape: StateShapeId,
    hash: StableHash,
    elements: u32,
    byte_len: u32,
}

impl StateEntryReport {
    /// The state's identity.
    pub const fn id(&self) -> StateId {
        self.id
    }

    /// The path it was declared under.
    pub const fn path(&self) -> &'static str {
        self.path
    }

    /// Its storage shape.
    pub const fn kind(&self) -> StateKind {
        self.kind
    }

    /// The shape identity of its values.
    pub const fn shape(&self) -> StateShapeId {
        self.shape
    }

    /// Its digest.
    pub const fn hash(&self) -> StableHash {
        self.hash
    }

    /// How many things it holds: one for a cell, the row count for a table, the
    /// item count for a sequence.
    pub const fn elements(&self) -> u32 {
        self.elements
    }

    /// How many bytes its payload occupies.
    pub const fn byte_len(&self) -> u32 {
        self.byte_len
    }
}

/// A machine-readable description of a snapshot.
///
/// This is the point of explicit state: a snapshot can be described completely —
/// what states exist, what shape each is, how big it is, what it digests to —
/// **without knowing a single game type**. Tooling and agents can answer "what
/// is in here?" and, with a [`crate::StateDiff`], "what changed and who was
/// allowed to change it?".
///
/// Reporting reads and returns; it mutates nothing, and it names no browser,
/// editor, or platform API.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateReport {
    schema_name: &'static str,
    schema_id: StateSchemaId,
    structure_hash: StableHash,
    version: SchemaVersion,
    snapshot_hash: StableHash,
    entries: Vec<StateEntryReport>,
}

impl StateReport {
    /// The schema's name.
    pub const fn schema_name(&self) -> &'static str {
        self.schema_name
    }

    /// The schema's identity.
    pub const fn schema_id(&self) -> StateSchemaId {
        self.schema_id
    }

    /// The schema's shape-only digest.
    pub const fn structure_hash(&self) -> StableHash {
        self.structure_hash
    }

    /// The schema version.
    pub const fn version(&self) -> SchemaVersion {
        self.version
    }

    /// The whole snapshot's digest.
    pub const fn snapshot_hash(&self) -> StableHash {
        self.snapshot_hash
    }

    /// One report per stored state, ascending by identity.
    pub fn entries(&self) -> &[StateEntryReport] {
        &self.entries
    }
}

/// How many elements a payload holds, per storage shape. Indexed by
/// `StateKind as usize`.
type Count = fn(&[u8]) -> StateResult<u32>;

const COUNT: [Count; 3] = [count_cell, count_table, count_sequence];

fn count_cell(_payload: &[u8]) -> StateResult<u32> {
    Ok(1)
}

fn count_table(payload: &[u8]) -> StateResult<u32> {
    parse_rows(payload).map(|rows| rows.len() as u32)
}

fn count_sequence(payload: &[u8]) -> StateResult<u32> {
    parse_items(payload).map(|items| items.len() as u32)
}

/// Describe a snapshot against the schema it was built from.
pub fn report(schema: &StateSchema, snapshot: &StateSnapshot) -> StateResult<StateReport> {
    snapshot
        .ids()
        .into_iter()
        .try_fold(Vec::new(), |mut entries, id| {
            schema.decl(id).and_then(|decl| {
                snapshot.payload(id).and_then(|payload| {
                    COUNT[decl.kind() as usize](payload).and_then(|elements| {
                        snapshot.entry_hash(id).map(|hash| {
                            entries.push(StateEntryReport {
                                id,
                                path: decl.path(),
                                kind: decl.kind(),
                                shape: decl.shape(),
                                hash,
                                elements,
                                byte_len: payload.len() as u32,
                            });
                            entries
                        })
                    })
                })
            })
        })
        .map(|entries| StateReport {
            schema_name: schema.name(),
            schema_id: schema.identity(),
            structure_hash: schema.structure_hash(),
            version: schema.version(),
            snapshot_hash: snapshot.hash(),
            entries,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state_decl::StateDecl;
    use crate::state_key::{CellKey, SequenceKey, StateKey, TableKey};
    use crate::state_sequence::StateSequence;
    use crate::state_snapshot_builder::StateSnapshotBuilder;
    use crate::state_table::StateTable;

    struct Tick;
    impl StateKey for Tick {
        const PATH: &'static str = "report/tick";
        const KIND: StateKind = StateKind::Cell;
    }
    impl CellKey for Tick {
        type Value = u64;
    }

    struct Rows;
    impl StateKey for Rows {
        const PATH: &'static str = "report/rows";
        const KIND: StateKind = StateKind::Table;
    }
    impl TableKey for Rows {
        type Key = u32;
        type Value = u64;
    }

    struct Log;
    impl StateKey for Log {
        const PATH: &'static str = "report/log";
        const KIND: StateKind = StateKind::Sequence;
    }
    impl SequenceKey for Log {
        type Item = u32;
    }

    fn schema() -> StateSchema {
        StateSchema::build(
            "report",
            SchemaVersion::new(1, 0),
            &[
                StateDecl::cell::<Tick>(),
                StateDecl::table::<Rows>(),
                StateDecl::sequence::<Log>(),
            ],
        )
        .expect("valid")
    }

    fn snapshot() -> StateSnapshot {
        StateSnapshotBuilder::new(&schema())
            .with_cell::<Tick>(&7)
            .with_table::<Rows>(&StateTable::new().with(1, 10).with(2, 20))
            .with_sequence::<Log>(&StateSequence::new().appended(5).appended(6).appended(7))
            .build()
            .expect("complete")
    }

    fn entry_for(report: &StateReport, id: StateId) -> StateEntryReport {
        *report
            .entries()
            .iter()
            .find(|entry| entry.id() == id)
            .expect("the state is reported")
    }

    #[test]
    fn a_report_describes_the_schema_it_was_taken_against() {
        let report = report(&schema(), &snapshot()).expect("reports");
        assert_eq!(report.schema_name(), "report");
        assert_eq!(report.schema_id(), schema().identity());
        assert_eq!(report.structure_hash(), schema().structure_hash());
        assert_eq!(report.version(), SchemaVersion::new(1, 0));
        assert_eq!(report.snapshot_hash(), snapshot().hash());
    }

    #[test]
    fn every_stored_state_is_described_with_its_declared_path() {
        let report = report(&schema(), &snapshot()).expect("reports");
        assert_eq!(report.entries().len(), 3);
        assert_eq!(entry_for(&report, Tick::id()).path(), "report/tick");
        assert_eq!(entry_for(&report, Rows::id()).kind(), StateKind::Table);
        assert_eq!(
            entry_for(&report, Log::id()).shape(),
            <Log as SequenceKey>::shape()
        );
    }

    #[test]
    fn element_counts_match_each_storage_shape() {
        let report = report(&schema(), &snapshot()).expect("reports");
        assert_eq!(entry_for(&report, Tick::id()).elements(), 1, "a cell is one");
        assert_eq!(entry_for(&report, Rows::id()).elements(), 2, "two rows");
        assert_eq!(entry_for(&report, Log::id()).elements(), 3, "three items");
    }

    #[test]
    fn each_entry_reports_its_size_and_digest() {
        let stored = snapshot();
        let report = report(&schema(), &stored).expect("reports");
        let tick = entry_for(&report, Tick::id());
        assert_eq!(tick.byte_len(), 8, "a u64 payload is eight bytes");
        assert_eq!(tick.hash(), stored.entry_hash(Tick::id()).expect("hash"));
    }

    #[test]
    fn entries_are_ordered_by_identity() {
        let report = report(&schema(), &snapshot()).expect("reports");
        let ids: Vec<StateId> = report.entries().iter().map(StateEntryReport::id).collect();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        assert_eq!(ids, sorted);
    }

    #[test]
    fn reporting_is_repeatable_and_changes_nothing() {
        let stored = snapshot();
        let before = stored.hash();
        let once = report(&schema(), &stored).expect("reports");
        let twice = report(&schema(), &stored).expect("reports");
        assert_eq!(once, twice);
        assert_eq!(stored.hash(), before);
    }

    #[test]
    fn a_state_the_schema_does_not_declare_is_rejected() {
        let narrow =
            StateSchema::build("report", SchemaVersion::new(1, 0), &[StateDecl::cell::<Tick>()])
                .expect("valid");
        assert!(report(&narrow, &snapshot()).is_err());
    }

    #[test]
    fn a_corrupt_payload_is_reported_rather_than_panicking() {
        let mut entries = snapshot().entries().clone();
        let broken = entries.get(&Rows::id()).expect("rows").with_payload(vec![9]);
        entries.insert(Rows::id(), broken);
        let damaged =
            StateSnapshot::from_parts(snapshot().schema_id(), snapshot().version(), entries);
        assert!(report(&schema(), &damaged).is_err());
    }

    #[test]
    fn an_empty_snapshot_reports_no_entries() {
        let empty_schema =
            StateSchema::build("report", SchemaVersion::new(1, 0), &[]).expect("valid");
        let empty = StateSnapshotBuilder::new(&empty_schema)
            .build()
            .expect("nothing to set");
        let report = report(&empty_schema, &empty).expect("reports");
        assert!(report.entries().is_empty());
    }
}
