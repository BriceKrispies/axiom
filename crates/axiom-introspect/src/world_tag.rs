//! A semantic tag naming a point in the world — the agent-interrogable "noun".

use axiom_kernel::{
    BinaryReader, BinaryWriter, KernelError, KernelErrorCode, KernelErrorScope, KernelResult,
    SchemaVersion,
};

/// The wire schema version of a serialized [`WorldTag`] set. Bumped on
/// incompatible layout changes; the major component gates compatibility.
const TAGS_SCHEMA: SchemaVersion = SchemaVersion::new(1, 0);

/// A stable, named point of interest in the world.
/// A `WorldTag` is the engine's semantic **noun**: a name (`"mountaintop"`), a
/// coarse `kind_code` so an agent can ask for "every summit", and a world
/// position in micro-units (the same fixed-point convention the agent and the
/// agent harness use for coordinates — millionths of a world unit). It carries
/// no behaviour; it is the inert thing an agent resolves a high-level command
/// against ("walk to the mountaintop").
/// Like [`crate::WorldReport`] it is serializable through the kernel binary
/// primitives, so an external agent can read the world's nouns over the same
/// byte channel as the frame and world snapshots — the [`crate::IntrospectApi`]
/// facade observes a set of these and answers name/kind/nearest queries over it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WorldTag {
    tag_id: u32,
    name: String,
    kind_code: u16,
    x: i64,
    y: i64,
    z: i64,
}

impl WorldTag {
    /// Build a tag: a stable `tag_id`, a semantic `name`, a coarse `kind_code`,
    /// and a world position `(x, y, z)` in micro-units.
    pub fn new(tag_id: u32, name: String, kind_code: u16, x: i64, y: i64, z: i64) -> Self {
        WorldTag {
            tag_id,
            name,
            kind_code,
            x,
            y,
            z,
        }
    }

    /// The stable id of this tag.
    pub const fn tag_id(&self) -> u32 {
        self.tag_id
    }

    /// The tag's semantic name (the noun an agent resolves against).
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The coarse kind discriminant (e.g. "summit", "ground"); the app owns the
    /// vocabulary, this layer only carries the code.
    pub const fn kind_code(&self) -> u16 {
        self.kind_code
    }

    /// The world `x` of the tagged point, in micro-units.
    pub const fn x(&self) -> i64 {
        self.x
    }

    /// The world `y` (height) of the tagged point, in micro-units.
    pub const fn y(&self) -> i64 {
        self.y
    }

    /// The world `z` of the tagged point, in micro-units.
    pub const fn z(&self) -> i64 {
        self.z
    }

    /// Append one tag's fields to a writer (no schema prefix — the set codec
    /// below writes the schema once). The name is length-prefixed.
    pub(crate) fn write_to(&self, writer: &mut BinaryWriter) {
        writer.write_u32(self.tag_id);
        writer.write_byte_slice(self.name.as_bytes());
        writer.write_u16(self.kind_code);
        writer.write_i64(self.x);
        writer.write_i64(self.y);
        writer.write_i64(self.z);
    }

    /// Read one tag's fields, in the order [`Self::write_to`] laid them down.
    /// The name is decoded lossily (tag names are ASCII in practice, so round
    /// trips are exact).
    pub(crate) fn read_from(reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
        reader.read_u32().and_then(|tag_id| {
            reader.read_byte_slice().and_then(|name_bytes| {
                let name = String::from_utf8_lossy(name_bytes).into_owned();
                reader.read_u16().and_then(|kind_code| {
                    reader.read_i64().and_then(|x| {
                        reader.read_i64().and_then(|y| {
                            reader.read_i64().map(|z| WorldTag {
                                tag_id,
                                name,
                                kind_code,
                                x,
                                y,
                                z,
                            })
                        })
                    })
                })
            })
        })
    }

    /// Serialize a whole set of tags — the snapshot an external agent reads to
    /// learn the world's nouns at once. Records a schema, the count, then each
    /// tag in order.
    pub fn encode_set(tags: &[WorldTag]) -> Vec<u8> {
        let mut writer = BinaryWriter::new();
        TAGS_SCHEMA.write_to(&mut writer);
        writer.write_u32(tags.len() as u32);
        tags.iter().for_each(|tag| tag.write_to(&mut writer));
        writer.into_bytes()
    }

    /// Decode a set previously produced by [`Self::encode_set`]. Fails with
    /// [`KernelErrorCode::SchemaVersionMismatch`] for an incompatible major
    /// version, or a binary error for truncated/invalid data.
    pub fn decode_set(bytes: &[u8]) -> KernelResult<Vec<WorldTag>> {
        let mut reader = BinaryReader::new(bytes);
        SchemaVersion::read_from(&mut reader)
            .and_then(|version| {
                TAGS_SCHEMA
                    .is_compatible_with(version)
                    .then_some(())
                    .ok_or_else(|| {
                        KernelError::new(
                            KernelErrorScope::Binary,
                            KernelErrorCode::SchemaVersionMismatch,
                            "WorldTag set schema major version is incompatible",
                        )
                    })
            })
            .and_then(|()| reader.read_u32())
            .and_then(|count| {
                (0..count)
                    .map(|_| WorldTag::read_from(&mut reader))
                    .collect::<KernelResult<Vec<_>>>()
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<WorldTag> {
        vec![
            WorldTag::new(
                1,
                "mountaintop".to_string(),
                7,
                1_000_000,
                8_840_000_000,
                -2_000_000,
            ),
            WorldTag::new(2, "ground".to_string(), 3, 0, 0, 0),
        ]
    }

    #[test]
    fn new_exposes_every_field() {
        let tag = WorldTag::new(5, "spawn".to_string(), 9, -3, 4, 7);
        assert_eq!(tag.tag_id(), 5);
        assert_eq!(tag.name(), "spawn");
        assert_eq!(tag.kind_code(), 9);
        assert_eq!((tag.x(), tag.y(), tag.z()), (-3, 4, 7));
    }

    #[test]
    fn set_round_trips_through_bytes() {
        let tags = sample();
        let decoded = WorldTag::decode_set(&WorldTag::encode_set(&tags)).unwrap();
        assert_eq!(decoded, tags);
    }

    #[test]
    fn empty_set_round_trips() {
        let decoded = WorldTag::decode_set(&WorldTag::encode_set(&[])).unwrap();
        assert!(decoded.is_empty());
    }

    #[test]
    fn encoding_is_deterministic() {
        assert_eq!(
            WorldTag::encode_set(&sample()),
            WorldTag::encode_set(&sample())
        );
    }

    #[test]
    fn truncation_at_every_prefix_is_err() {
        let bytes = WorldTag::encode_set(&sample());
        for len in 0..bytes.len() {
            assert!(
                WorldTag::decode_set(&bytes[..len]).is_err(),
                "truncated decode at len {len} must fail"
            );
        }
    }

    #[test]
    fn incompatible_schema_major_is_rejected() {
        let mut writer = BinaryWriter::new();
        SchemaVersion::new(TAGS_SCHEMA.major() + 1, 0).write_to(&mut writer);
        let bytes = writer.into_bytes();
        assert_eq!(
            WorldTag::decode_set(&bytes).unwrap_err().code(),
            KernelErrorCode::SchemaVersionMismatch
        );
    }

    #[test]
    fn derives_are_exercised() {
        let a = WorldTag::new(1, "a".to_string(), 0, 0, 0, 0);
        let b = a.clone();
        assert_eq!(a, b);
        assert_ne!(a, WorldTag::new(2, "a".to_string(), 0, 0, 0, 0));
        assert!(format!("{a:?}").contains("WorldTag"));
    }
}
