//! The asset manifest: the versioned, deterministic list of asset entries the
//! engine streams, plus its binary codec and well-formedness validation.
//!
//! The dependency DAG lives in each entry's `dependencies`. Cycle-freeness is an
//! authoring/tooling concern, not a runtime one: a cyclic entry simply never
//! becomes eligible to load (its dependency never reaches `ready`), the same way
//! the engine's *layer* DAG is validated by `xtask`, not the kernel at runtime.

use std::collections::BTreeMap;

use axiom_kernel::{
    AssetId, BinaryReader, BinaryWriter, KernelError, KernelErrorCode, KernelErrorScope,
    KernelResult, SchemaVersion,
};

use crate::asset_entry::AssetEntry;

/// One authoring tuple: `(id, kind, priority, size_hint, content_hash, locator,
/// dependencies)`. The shape [`crate::AssetsApi::encode_manifest`] accepts.
pub(crate) type EntryTuple<'a> = (AssetId, u32, u32, u64, u64, &'a str, &'a [AssetId]);

/// The manifest wire-format version. A reader accepts any manifest sharing this
/// major (see [`SchemaVersion::is_compatible_with`]).
const MANIFEST_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

/// A parsed, validated manifest: the assets in authored order.
#[derive(Debug, Clone)]
pub(crate) struct Manifest {
    entries: Vec<AssetEntry>,
}

impl Manifest {
    pub(crate) fn entries(&self) -> &[AssetEntry] {
        &self.entries
    }

    /// Encode authoring tuples to canonical manifest bytes (no validation — the
    /// inverse [`Self::read`] validates on the way back in).
    pub(crate) fn encode(entries: &[EntryTuple<'_>]) -> Vec<u8> {
        let mut writer = BinaryWriter::new();
        MANIFEST_VERSION.write_to(&mut writer);
        writer.write_u32(entries.len() as u32);
        entries
            .iter()
            .for_each(|tuple| entry_from_tuple(tuple).write_to(&mut writer));
        writer.into_bytes()
    }

    /// Parse and validate a manifest from canonical bytes.
    pub(crate) fn read(bytes: &[u8]) -> KernelResult<Manifest> {
        let mut reader = BinaryReader::new(bytes);
        SchemaVersion::read_from(&mut reader)
            .and_then(|version| {
                version
                    .is_compatible_with(MANIFEST_VERSION)
                    .then_some(())
                    .ok_or_else(|| {
                        manifest_error(
                            KernelErrorCode::SchemaVersionMismatch,
                            "asset manifest schema major is incompatible",
                        )
                    })
                    .and_then(|()| reader.read_u32())
                    .and_then(|count| read_entries(&mut reader, count))
            })
            .map(|entries| Manifest { entries })
            .and_then(validate)
    }
}

fn entry_from_tuple(tuple: &EntryTuple<'_>) -> AssetEntry {
    AssetEntry {
        id: tuple.0,
        kind: tuple.1,
        priority: tuple.2,
        size_hint: tuple.3,
        content_hash: tuple.4,
        locator: tuple.5.as_bytes().to_vec(),
        dependencies: tuple.6.to_vec(),
    }
}

/// Read a `u32` entry count then that many entries, via `try_fold` (no loop).
fn read_entries(reader: &mut BinaryReader<'_>, count: u32) -> KernelResult<Vec<AssetEntry>> {
    (0..count).try_fold(Vec::new(), |mut accumulated, _| {
        AssetEntry::read_from(reader).map(|entry| {
            accumulated.push(entry);
            accumulated
        })
    })
}

/// The id→position index. A smaller map than the entry count means a duplicate
/// id collapsed, so this doubles as the uniqueness signal during validation.
pub(crate) fn build_index(entries: &[AssetEntry]) -> BTreeMap<AssetId, usize> {
    entries
        .iter()
        .enumerate()
        .map(|(position, entry)| (entry.id, position))
        .collect()
}

/// Reject a manifest whose ids are null or duplicated, or whose dependencies
/// reference an absent asset. The reasons fold into one check: the scheduler
/// relies only on the manifest being well-formed, not on which rule failed.
fn validate(manifest: Manifest) -> KernelResult<Manifest> {
    let index = build_index(&manifest.entries);
    let ids_unique = index.len() == manifest.entries.len();
    let ids_valid = manifest.entries.iter().all(|entry| entry.id.is_valid());
    let deps_exist = manifest
        .entries
        .iter()
        .all(|entry| entry.dependencies.iter().all(|dep| index.contains_key(dep)));
    (ids_unique & ids_valid & deps_exist)
        .then_some(manifest)
        .ok_or_else(|| {
            manifest_error(
                KernelErrorCode::InvalidId,
                "asset manifest has a null or duplicate id, or a dependency on an unknown asset",
            )
        })
}

fn manifest_error(code: KernelErrorCode, message: &'static str) -> KernelError {
    KernelError::new(KernelErrorScope::Binary, code, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deps() -> Vec<AssetId> {
        Vec::new()
    }

    #[test]
    fn encode_then_read_round_trips_a_valid_manifest() {
        let one = AssetId::from_raw(1);
        let two = AssetId::from_raw(2);
        let bytes = Manifest::encode(&[
            (one, 0, 10, 100, 0xAA, "a", deps().as_slice()),
            (two, 1, 20, 200, 0xBB, "b", &[one]),
        ]);
        let manifest = Manifest::read(&bytes).unwrap();
        assert_eq!(manifest.entries().len(), 2);
        assert_eq!(manifest.entries()[1].dependencies, vec![one]);
    }

    #[test]
    fn read_rejects_incompatible_schema_major() {
        // A well-formed buffer whose major is 2, not the reader's 1.
        let mut writer = BinaryWriter::new();
        SchemaVersion::new(2, 0).write_to(&mut writer);
        writer.write_u32(0);
        let err = Manifest::read(&writer.into_bytes()).unwrap_err();
        assert_eq!(err.code(), KernelErrorCode::SchemaVersionMismatch);
    }

    #[test]
    fn read_rejects_a_duplicate_id() {
        let dup = AssetId::from_raw(5);
        let bytes = Manifest::encode(&[
            (dup, 0, 0, 0, 0, "a", deps().as_slice()),
            (dup, 0, 0, 0, 0, "b", deps().as_slice()),
        ]);
        assert_eq!(
            Manifest::read(&bytes).unwrap_err().code(),
            KernelErrorCode::InvalidId
        );
    }

    #[test]
    fn read_rejects_a_null_id() {
        let bytes = Manifest::encode(&[(AssetId::NULL, 0, 0, 0, 0, "a", deps().as_slice())]);
        assert_eq!(
            Manifest::read(&bytes).unwrap_err().code(),
            KernelErrorCode::InvalidId
        );
    }

    #[test]
    fn read_rejects_a_dependency_on_an_unknown_asset() {
        let known = AssetId::from_raw(1);
        let missing = AssetId::from_raw(99);
        let bytes = Manifest::encode(&[(known, 0, 0, 0, 0, "a", &[missing])]);
        assert_eq!(
            Manifest::read(&bytes).unwrap_err().code(),
            KernelErrorCode::InvalidId
        );
    }

    #[test]
    fn read_rejects_truncated_bytes() {
        // Empty buffer: even the schema version cannot be read.
        assert!(Manifest::read(&[]).is_err());
        // Declares two entries but provides none.
        let mut writer = BinaryWriter::new();
        SchemaVersion::new(1, 0).write_to(&mut writer);
        writer.write_u32(2);
        assert!(Manifest::read(&writer.into_bytes()).is_err());
    }
}
