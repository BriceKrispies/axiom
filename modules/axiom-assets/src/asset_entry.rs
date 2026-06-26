//! One asset's manifest record and its deterministic binary codec.

use axiom_kernel::{AssetId, BinaryReader, BinaryWriter, KernelResult};

/// A single asset's manifest entry: stable identity, an app-defined `kind` tag,
/// scheduling `priority`, a `size_hint`, a `content_hash`, an opaque `locator`
/// the app fetches, and the ids this asset depends on (its outgoing edges in the
/// dependency DAG). The module never interprets `kind` or `locator`.
#[derive(Debug, Clone)]
pub(crate) struct AssetEntry {
    pub(crate) id: AssetId,
    pub(crate) kind: u32,
    pub(crate) priority: u32,
    pub(crate) size_hint: u64,
    pub(crate) content_hash: u64,
    pub(crate) locator: Vec<u8>,
    pub(crate) dependencies: Vec<AssetId>,
}

impl AssetEntry {
    /// Append this entry to `writer` in canonical field order.
    pub(crate) fn write_to(&self, writer: &mut BinaryWriter) {
        self.id.write_to(writer);
        writer.write_u32(self.kind);
        writer.write_u32(self.priority);
        writer.write_u64(self.size_hint);
        writer.write_u64(self.content_hash);
        writer.write_byte_slice(&self.locator);
        writer.write_u32(self.dependencies.len() as u32);
        self.dependencies
            .iter()
            .for_each(|dep| dep.write_to(writer));
    }

    /// Read one entry previously written by [`Self::write_to`]. Any short read
    /// propagates the kernel's bounds error.
    pub(crate) fn read_from(reader: &mut BinaryReader<'_>) -> KernelResult<AssetEntry> {
        AssetId::read_from(reader).and_then(|id| {
            reader.read_u32().and_then(|kind| {
                reader.read_u32().and_then(|priority| {
                    reader.read_u64().and_then(|size_hint| {
                        reader.read_u64().and_then(|content_hash| {
                            reader.read_byte_slice().and_then(|locator| {
                                read_dependencies(reader).map(|dependencies| AssetEntry {
                                    id,
                                    kind,
                                    priority,
                                    size_hint,
                                    content_hash,
                                    locator: locator.to_vec(),
                                    dependencies,
                                })
                            })
                        })
                    })
                })
            })
        })
    }
}

/// Read a `u32` dependency count then that many ids, via `try_fold` (no loop).
/// Starts from an empty `Vec` (never pre-allocated from the untrusted count) so
/// a malformed length cannot trigger a huge allocation before the read fails.
fn read_dependencies(reader: &mut BinaryReader<'_>) -> KernelResult<Vec<AssetId>> {
    reader.read_u32().and_then(|count| {
        (0..count).try_fold(Vec::new(), |mut accumulated, _| {
            AssetId::read_from(reader).map(|dep| {
                accumulated.push(dep);
                accumulated
            })
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> AssetEntry {
        AssetEntry {
            id: AssetId::from_raw(7),
            kind: 3,
            priority: 50,
            size_hint: 4096,
            content_hash: 0xDEAD_BEEF,
            locator: b"assets/hero.mesh".to_vec(),
            dependencies: vec![AssetId::from_raw(1), AssetId::from_raw(2)],
        }
    }

    #[test]
    fn round_trips_through_the_binary_codec() {
        let entry = sample();
        let mut writer = BinaryWriter::new();
        entry.write_to(&mut writer);
        let bytes = writer.into_bytes();

        let mut reader = BinaryReader::new(&bytes);
        let restored = AssetEntry::read_from(&mut reader).unwrap();
        assert_eq!(restored.id, entry.id);
        assert_eq!(restored.kind, entry.kind);
        assert_eq!(restored.priority, entry.priority);
        assert_eq!(restored.size_hint, entry.size_hint);
        assert_eq!(restored.content_hash, entry.content_hash);
        assert_eq!(restored.locator, entry.locator);
        assert_eq!(restored.dependencies, entry.dependencies);
        assert_eq!(reader.remaining(), 0);
    }

    #[test]
    fn read_from_rejects_a_truncated_entry() {
        let entry = sample();
        let mut writer = BinaryWriter::new();
        entry.write_to(&mut writer);
        let bytes = writer.into_bytes();
        // Drop the final dependency id's bytes so the dep read runs past the end.
        let mut reader = BinaryReader::new(&bytes[..bytes.len() - 4]);
        assert!(AssetEntry::read_from(&mut reader).is_err());
    }

    #[test]
    fn round_trips_an_entry_with_no_dependencies() {
        let entry = AssetEntry {
            id: AssetId::from_raw(9),
            kind: 0,
            priority: 0,
            size_hint: 0,
            content_hash: 0,
            locator: Vec::new(),
            dependencies: Vec::new(),
        };
        let mut writer = BinaryWriter::new();
        entry.write_to(&mut writer);
        let bytes = writer.into_bytes();
        let mut reader = BinaryReader::new(&bytes);
        let restored = AssetEntry::read_from(&mut reader).unwrap();
        assert!(restored.dependencies.is_empty());
        assert!(restored.locator.is_empty());
    }
}
