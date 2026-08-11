//! Complete state at one logical instant, as an immutable value.

use std::collections::BTreeMap;

use axiom_kernel::{BinaryReader, BinaryWriter, SchemaVersion, StableHash};

use crate::state_entry::StateEntry;
use crate::state_error::StateError;
use crate::state_error_code::StateErrorCode;
use crate::state_id::StateId;
use crate::state_key::{CellKey, SequenceKey, TableKey};
use crate::state_kind::StateKind;
use crate::state_payload::{decode_cell, decode_sequence, decode_table};
use crate::state_schema_id::{version_word, StateSchemaId};
use crate::state_sequence::StateSequence;
use crate::state_table::StateTable;
use crate::state_shape_id::StateShapeId;
use crate::StateResult;

/// `"AXST"` little-endian — the leading bytes of a serialized snapshot.
const MAGIC: u32 = 0x5453_5841;

/// Every declared state's contents at one instant.
///
/// Immutable through its entire public API: every method takes `&self` and
/// returns an owned value or a shared slice. There is no `current()`, no
/// `latest()`, and no way to obtain a mutable reference into stored state —
/// because the point of the substrate is that whoever owns "the current
/// snapshot" lives outside it.
///
/// A new snapshot comes from [`crate::StateSnapshotBuilder`] or from applying a
/// patch to an existing one, never from mutating this one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateSnapshot {
    schema_id: StateSchemaId,
    version: SchemaVersion,
    entries: BTreeMap<StateId, StateEntry>,
}

impl StateSnapshot {
    /// Assemble a snapshot from already-validated entries.
    pub(crate) const fn from_parts(
        schema_id: StateSchemaId,
        version: SchemaVersion,
        entries: BTreeMap<StateId, StateEntry>,
    ) -> Self {
        StateSnapshot {
            schema_id,
            version,
            entries,
        }
    }

    /// The stored entries, for the patch applier and the differ.
    pub(crate) const fn entries(&self) -> &BTreeMap<StateId, StateEntry> {
        &self.entries
    }

    /// The identity of the schema this snapshot was built against.
    pub const fn schema_id(&self) -> StateSchemaId {
        self.schema_id
    }

    /// The schema version this snapshot was built at.
    pub const fn version(&self) -> SchemaVersion {
        self.version
    }

    /// How many states this snapshot holds.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether this snapshot holds no state at all.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Every stored identity, ascending.
    pub fn ids(&self) -> Vec<StateId> {
        self.entries.keys().copied().collect()
    }

    /// The stored entry, or a failure naming the identity.
    fn entry(&self, id: StateId) -> StateResult<&StateEntry> {
        self.entries.get(&id).ok_or(StateError::at(
            StateErrorCode::UnknownStateIdentity,
            id,
            "this snapshot holds no state with that identity",
        ))
    }

    /// The stored entry, once its shape is confirmed to be the requested one.
    fn typed_entry(&self, id: StateId, want: StateShapeId) -> StateResult<&StateEntry> {
        self.entry(id).and_then(|entry| {
            (entry.shape() == want).then_some(entry).ok_or(StateError::at(
                StateErrorCode::StateTypeMismatch,
                id,
                "the stored state has a different shape than the one requested",
            ))
        })
    }

    /// The storage shape of a stored state.
    pub fn kind(&self, id: StateId) -> StateResult<StateKind> {
        self.entry(id).map(StateEntry::kind)
    }

    /// The shape identity of a stored state's values.
    pub fn shape(&self, id: StateId) -> StateResult<StateShapeId> {
        self.entry(id).map(StateEntry::shape)
    }

    /// A stored state's canonical bytes — the introspection and diff surface.
    pub fn payload(&self, id: StateId) -> StateResult<&[u8]> {
        self.entry(id).map(StateEntry::payload)
    }

    /// Read a cell.
    pub fn cell<K: CellKey>(&self) -> StateResult<K::Value> {
        let id = K::id();
        self.typed_entry(id, <K as CellKey>::shape())
            .and_then(|entry| {
                decode_cell::<K::Value>(entry.payload()).map_err(|error| error.about(id))
            })
    }

    /// Read a table.
    pub fn table<K: TableKey>(&self) -> StateResult<StateTable<K::Key, K::Value>> {
        let id = K::id();
        self.typed_entry(id, <K as TableKey>::shape())
            .and_then(|entry| {
                decode_table::<K::Key, K::Value>(entry.payload())
                    .map_err(|error| error.about(id))
            })
    }

    /// Read a sequence.
    pub fn sequence<K: SequenceKey>(&self) -> StateResult<StateSequence<K::Item>> {
        let id = K::id();
        self.typed_entry(id, <K as SequenceKey>::shape())
            .and_then(|entry| {
                decode_sequence::<K::Item>(entry.payload()).map_err(|error| error.about(id))
            })
    }

    /// One stored state's digest.
    pub fn entry_hash(&self, id: StateId) -> StateResult<StableHash> {
        self.entry(id).map(StateEntry::digest)
    }

    /// The whole snapshot's digest.
    ///
    /// Deterministic across runs, processes and targets: it folds only the
    /// schema identity, the version, and each state's identity and digest, in
    /// identity order. See `ARCHITECTURE.md` for exactly what it does and does
    /// not guarantee — in particular it is a 64-bit index, so byte equality
    /// remains the proof and a hash match is a strong hint.
    pub fn hash(&self) -> StableHash {
        let words: Vec<u64> = core::iter::once(self.schema_id.raw())
            .chain(core::iter::once(version_word(self.version)))
            .chain(
                self.entries
                    .iter()
                    .flat_map(|(id, entry)| [id.raw(), entry.digest().raw()]),
            )
            .collect();
        StableHash::of_words(&words)
    }

    /// Serialize to canonical little-endian bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut writer = BinaryWriter::new();
        writer.write_u32(MAGIC);
        self.version.write_to(&mut writer);
        writer.write_u64(self.schema_id.raw());
        writer.write_u32(self.entries.len() as u32);
        self.entries.iter().for_each(|(id, entry)| {
            writer.write_u64(id.raw());
            writer.write_u8(entry.kind().code());
            writer.write_u64(entry.shape().raw());
            writer.write_byte_slice(entry.payload());
        });
        writer.into_bytes()
    }

    /// Deserialize from canonical bytes.
    ///
    /// Rejects, deterministically: wrong magic, an incompatible schema major
    /// version, an out-of-range storage-kind code, and truncation at any point.
    pub fn from_bytes(bytes: &[u8], expected: SchemaVersion) -> StateResult<Self> {
        let mut reader = BinaryReader::new(bytes);
        read_magic(&mut reader)
            .and_then(|()| read_version(&mut reader, expected))
            .and_then(|version| {
                reader
                    .read_u64()
                    .map_err(corrupted)
                    .map(|schema_id| (version, StateSchemaId::from_raw(schema_id)))
            })
            .and_then(|(version, schema_id)| {
                read_entries(&mut reader)
                    .map(|entries| StateSnapshot::from_parts(schema_id, version, entries))
            })
    }
}

/// Wrap a kernel decode failure as a corrupt-snapshot failure.
fn corrupted(cause: axiom_kernel::KernelError) -> StateError {
    StateError::new(
        StateErrorCode::CorruptedSnapshot,
        "the snapshot bytes are malformed or truncated",
    )
    .caused_by(cause)
}

fn read_magic(reader: &mut BinaryReader<'_>) -> StateResult<()> {
    reader.read_u32().map_err(corrupted).and_then(|magic| {
        (magic == MAGIC).then_some(()).ok_or(StateError::new(
            StateErrorCode::CorruptedSnapshot,
            "these bytes are not an Axiom state snapshot",
        ))
    })
}

fn read_version(reader: &mut BinaryReader<'_>, expected: SchemaVersion) -> StateResult<SchemaVersion> {
    SchemaVersion::read_from(reader)
        .map_err(corrupted)
        .and_then(|version| {
            version
                .is_compatible_with(expected)
                .then_some(version)
                .ok_or(StateError::new(
                    StateErrorCode::SchemaVersionMismatch,
                    "the snapshot's schema major version is not compatible",
                ))
        })
}

fn read_entries(reader: &mut BinaryReader<'_>) -> StateResult<BTreeMap<StateId, StateEntry>> {
    reader.read_u32().map_err(corrupted).and_then(|count| {
        (0..count).try_fold(BTreeMap::new(), |mut entries, _| {
            read_entry(reader).map(|(id, entry)| {
                entries.insert(id, entry);
                entries
            })
        })
    })
}

fn read_entry(reader: &mut BinaryReader<'_>) -> StateResult<(StateId, StateEntry)> {
    reader
        .read_u64()
        .map_err(corrupted)
        .map(StateId::from_raw)
        .and_then(|id| {
            reader
                .read_u8()
                .map_err(corrupted)
                .and_then(|code| {
                    StateKind::from_code(code).ok_or(StateError::at(
                        StateErrorCode::CorruptedSnapshot,
                        id,
                        "the stored storage-kind code names no state kind",
                    ))
                })
                .and_then(|kind| {
                    reader
                        .read_u64()
                        .map_err(corrupted)
                        .map(|shape| (kind, StateShapeId::from_raw(shape)))
                })
                .and_then(|(kind, shape)| {
                    reader
                        .read_byte_slice()
                        .map_err(corrupted)
                        .map(|payload| (id, StateEntry::new(kind, shape, payload.to_vec())))
                })
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state_decl::StateDecl;
    use crate::state_key::StateKey;
    use crate::state_schema::StateSchema;
    use crate::state_snapshot_builder::StateSnapshotBuilder;

    pub(crate) struct Tick;
    impl StateKey for Tick {
        const PATH: &'static str = "test/tick";
        const KIND: StateKind = StateKind::Cell;
    }
    impl CellKey for Tick {
        type Value = u64;
    }

    pub(crate) struct Rows;
    impl StateKey for Rows {
        const PATH: &'static str = "test/rows";
        const KIND: StateKind = StateKind::Table;
    }
    impl TableKey for Rows {
        type Key = u32;
        type Value = u64;
    }

    pub(crate) struct Log;
    impl StateKey for Log {
        const PATH: &'static str = "test/log";
        const KIND: StateKind = StateKind::Sequence;
    }
    impl SequenceKey for Log {
        type Item = u32;
    }

    /// A cell key at the same path as `Tick` but a different value type — the
    /// only way to reach a shape mismatch within one process.
    pub(crate) struct TickAsU32;
    impl StateKey for TickAsU32 {
        const PATH: &'static str = "test/tick";
        const KIND: StateKind = StateKind::Cell;
    }
    impl CellKey for TickAsU32 {
        type Value = u32;
    }

    pub(crate) fn version() -> SchemaVersion {
        SchemaVersion::new(1, 0)
    }

    pub(crate) fn schema() -> StateSchema {
        StateSchema::build(
            "test",
            version(),
            &[
                StateDecl::cell::<Tick>(),
                StateDecl::table::<Rows>(),
                StateDecl::sequence::<Log>(),
            ],
        )
        .expect("valid schema")
    }

    pub(crate) fn snapshot() -> StateSnapshot {
        StateSnapshotBuilder::new(&schema())
            .with_cell::<Tick>(&7)
            .with_table::<Rows>(&StateTable::new().with(1, 10).with(2, 20))
            .with_sequence::<Log>(&StateSequence::new().appended(5))
            .build()
            .expect("every declared state was set")
    }

    #[test]
    fn a_snapshot_reports_its_schema_version_and_size() {
        let snapshot = snapshot();
        assert_eq!(snapshot.schema_id(), schema().identity());
        assert_eq!(snapshot.version(), version());
        assert_eq!(snapshot.len(), 3);
        assert!(!snapshot.is_empty());
        let mut expected = vec![Tick::id(), Rows::id(), Log::id()];
        expected.sort_unstable();
        assert_eq!(snapshot.ids(), expected);
    }

    #[test]
    fn stored_identities_are_ascending() {
        let ids = snapshot().ids();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        assert_eq!(ids, sorted);
    }

    #[test]
    fn every_shape_reads_back_as_it_was_written() {
        let snapshot = snapshot();
        assert_eq!(snapshot.cell::<Tick>(), Ok(7));
        assert_eq!(
            snapshot.table::<Rows>().expect("table reads"),
            StateTable::new().with(1, 10).with(2, 20)
        );
        assert_eq!(
            snapshot.sequence::<Log>().expect("sequence reads"),
            StateSequence::new().appended(5)
        );
    }

    #[test]
    fn a_snapshot_describes_what_it_stores() {
        let snapshot = snapshot();
        assert_eq!(snapshot.kind(Tick::id()), Ok(StateKind::Cell));
        assert_eq!(snapshot.kind(Rows::id()), Ok(StateKind::Table));
        assert_eq!(snapshot.kind(Log::id()), Ok(StateKind::Sequence));
        assert_eq!(
            snapshot.shape(Tick::id()),
            Ok(StateShapeId::cell_of::<u64>())
        );
        assert!(!snapshot.payload(Tick::id()).expect("payload").is_empty());
    }

    #[test]
    fn an_unknown_identity_is_rejected_everywhere_it_can_be_asked_for() {
        let snapshot = snapshot();
        let missing = StateId::of_path("test/absent");
        assert_eq!(
            snapshot.kind(missing).unwrap_err().code(),
            StateErrorCode::UnknownStateIdentity
        );
        assert_eq!(
            snapshot.shape(missing).unwrap_err().code(),
            StateErrorCode::UnknownStateIdentity
        );
        assert_eq!(
            snapshot.payload(missing).unwrap_err().code(),
            StateErrorCode::UnknownStateIdentity
        );
        assert_eq!(
            snapshot.entry_hash(missing).unwrap_err().code(),
            StateErrorCode::UnknownStateIdentity
        );
    }

    #[test]
    fn reading_a_stored_state_as_the_wrong_type_is_a_shape_mismatch() {
        let error = snapshot().cell::<TickAsU32>().unwrap_err();
        assert_eq!(error.code(), StateErrorCode::StateTypeMismatch);
        assert_eq!(error.state(), Tick::id());
    }

    #[test]
    fn reading_a_cell_as_a_table_or_sequence_is_a_shape_mismatch() {
        struct TickAsTable;
        impl StateKey for TickAsTable {
            const PATH: &'static str = "test/tick";
            const KIND: StateKind = StateKind::Table;
        }
        impl TableKey for TickAsTable {
            type Key = u32;
            type Value = u64;
        }

        struct TickAsSequence;
        impl StateKey for TickAsSequence {
            const PATH: &'static str = "test/tick";
            const KIND: StateKind = StateKind::Sequence;
        }
        impl SequenceKey for TickAsSequence {
            type Item = u32;
        }

        let snapshot = snapshot();
        assert_eq!(
            snapshot.table::<TickAsTable>().unwrap_err().code(),
            StateErrorCode::StateTypeMismatch
        );
        assert_eq!(
            snapshot.sequence::<TickAsSequence>().unwrap_err().code(),
            StateErrorCode::StateTypeMismatch
        );
    }

    #[test]
    fn identical_snapshots_hash_identically_and_repeatedly() {
        assert_eq!(snapshot().hash(), snapshot().hash());
        let once = snapshot();
        assert_eq!(once.hash(), once.hash());
    }

    #[test]
    fn a_changed_value_changes_the_hash_and_the_entry_hash() {
        let base = snapshot();
        let changed = StateSnapshotBuilder::new(&schema())
            .with_cell::<Tick>(&8)
            .with_table::<Rows>(&StateTable::new().with(1, 10).with(2, 20))
            .with_sequence::<Log>(&StateSequence::new().appended(5))
            .build()
            .expect("complete");
        assert_ne!(base.hash(), changed.hash());
        assert_ne!(
            base.entry_hash(Tick::id()).expect("hash"),
            changed.entry_hash(Tick::id()).expect("hash")
        );
        assert_eq!(
            base.entry_hash(Rows::id()).expect("hash"),
            changed.entry_hash(Rows::id()).expect("hash"),
            "an untouched state's digest must not move"
        );
    }

    #[test]
    fn a_snapshot_round_trips_through_its_bytes() {
        let base = snapshot();
        let bytes = base.to_bytes();
        let restored = StateSnapshot::from_bytes(&bytes, version()).expect("round trip");
        assert_eq!(restored, base);
        assert_eq!(restored.hash(), base.hash());
        assert_eq!(restored.to_bytes(), bytes, "serialization is canonical");
    }

    #[test]
    fn identical_snapshots_serialize_to_identical_bytes() {
        assert_eq!(snapshot().to_bytes(), snapshot().to_bytes());
    }

    #[test]
    fn bytes_that_are_not_a_snapshot_are_rejected() {
        let error = StateSnapshot::from_bytes(&[0, 0, 0, 0], version()).unwrap_err();
        assert_eq!(error.code(), StateErrorCode::CorruptedSnapshot);
    }

    #[test]
    fn an_incompatible_major_version_is_rejected() {
        let bytes = snapshot().to_bytes();
        let error = StateSnapshot::from_bytes(&bytes, SchemaVersion::new(2, 0)).unwrap_err();
        assert_eq!(error.code(), StateErrorCode::SchemaVersionMismatch);
    }

    #[test]
    fn a_compatible_minor_version_is_accepted() {
        let bytes = snapshot().to_bytes();
        assert!(StateSnapshot::from_bytes(&bytes, SchemaVersion::new(1, 9)).is_ok());
    }

    #[test]
    fn truncation_at_every_prefix_is_rejected_and_never_panics() {
        let bytes = snapshot().to_bytes();
        (0..bytes.len()).for_each(|len| {
            assert!(
                StateSnapshot::from_bytes(&bytes[..len], version()).is_err(),
                "a snapshot truncated to {len} bytes must not decode"
            );
        });
    }

    #[test]
    fn an_out_of_range_storage_kind_code_is_rejected() {
        let mut bytes = snapshot().to_bytes();
        // magic(4) + version(4) + schema_id(8) + count(4) + state_id(8) = 28,
        // which is the first entry's kind byte.
        bytes[28] = 200;
        let error = StateSnapshot::from_bytes(&bytes, version()).unwrap_err();
        assert_eq!(error.code(), StateErrorCode::CorruptedSnapshot);
    }

    #[test]
    fn an_empty_snapshot_is_valid_and_round_trips() {
        let empty = StateSnapshotBuilder::new(
            &StateSchema::build("empty", version(), &[]).expect("valid"),
        )
        .build()
        .expect("nothing to set");
        assert!(empty.is_empty());
        assert_eq!(empty.len(), 0);
        assert!(empty.ids().is_empty());
        let restored = StateSnapshot::from_bytes(&empty.to_bytes(), version()).expect("round trip");
        assert_eq!(restored, empty);
    }

    #[test]
    fn two_snapshots_are_independent_values() {
        let first = snapshot();
        let second = StateSnapshotBuilder::new(&schema())
            .with_cell::<Tick>(&99)
            .with_table::<Rows>(&StateTable::new())
            .with_sequence::<Log>(&StateSequence::new())
            .build()
            .expect("complete");
        assert_eq!(first.cell::<Tick>(), Ok(7), "the first snapshot is untouched");
        assert_eq!(second.cell::<Tick>(), Ok(99));
    }
}
