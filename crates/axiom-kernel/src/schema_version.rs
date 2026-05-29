//! A `major.minor` schema version with a compatibility rule.

use crate::binary_reader::BinaryReader;
use crate::binary_writer::BinaryWriter;
use crate::result::KernelResult;

/// A binary schema version, `major.minor`.
///
/// Compatibility is defined deterministically: two versions are compatible iff
/// they share the same `major`. This lets a reader reject data it cannot decode
/// via [`crate::error_code::KernelErrorCode::SchemaVersionMismatch`] without any
/// ambiguity. The version itself serializes as two little-endian `u16`s.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SchemaVersion {
    major: u16,
    minor: u16,
}

impl SchemaVersion {
    /// Construct a version.
    pub const fn new(major: u16, minor: u16) -> Self {
        SchemaVersion { major, minor }
    }

    /// The major component.
    pub const fn major(self) -> u16 {
        self.major
    }

    /// The minor component.
    pub const fn minor(self) -> u16 {
        self.minor
    }

    /// Whether `self` and `other` share a major version.
    pub const fn is_compatible_with(self, other: SchemaVersion) -> bool {
        self.major == other.major
    }

    /// Serialize as `major` then `minor`, each little-endian `u16`.
    pub fn write_to(self, writer: &mut BinaryWriter) {
        writer.write_u16(self.major);
        writer.write_u16(self.minor);
    }

    /// Read a version previously written with [`Self::write_to`].
    pub fn read_from(reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
        let major = reader.read_u16()?;
        let minor = reader.read_u16()?;
        Ok(SchemaVersion { major, minor })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accessors_return_parts() {
        let v = SchemaVersion::new(2, 7);
        assert_eq!(v.major(), 2);
        assert_eq!(v.minor(), 7);
    }

    #[test]
    fn compatibility_is_by_major_only() {
        assert!(SchemaVersion::new(1, 0).is_compatible_with(SchemaVersion::new(1, 9)));
        assert!(!SchemaVersion::new(1, 0).is_compatible_with(SchemaVersion::new(2, 0)));
    }

    #[test]
    fn serialization_round_trips() {
        let v = SchemaVersion::new(3, 14);
        let mut w = BinaryWriter::new();
        v.write_to(&mut w);
        let bytes = w.into_bytes();
        assert_eq!(bytes.len(), 4);

        let mut r = BinaryReader::new(&bytes);
        assert_eq!(SchemaVersion::read_from(&mut r).unwrap(), v);
    }
}
