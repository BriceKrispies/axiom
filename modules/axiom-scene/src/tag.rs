//! Tag scene component: a node's coarse semantic kind for agent perception.

use axiom_kernel::{BinaryReader, BinaryWriter, FieldSchema, KernelResult, Reflect, TypeSchema};

/// A coarse semantic kind attached to a node — what the thing *is* (a wall, an
/// enemy, a door, an item), as a numeric code whose vocabulary the app owns.
///
/// This is the engine-native answer to "what did I just see": a perceiving agent
/// resolves a raycast / overlap hit to a node, reads its `Tag`, and knows the
/// kind without the app maintaining a side table of entity classifications. The
/// scene only carries the code; it ascribes no meaning to any particular value
/// (that is the game's vocabulary), exactly as `player_index` carries an opaque
/// actor index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tag {
    kind_code: u32,
}

impl Tag {
    /// The reflected shape of a tag component.
    pub const SCHEMA: TypeSchema =
        TypeSchema::new("Tag", &[FieldSchema::new("kind_code", "u32")]);

    /// A tag carrying the given coarse kind code. Plain data: any `u32` is a
    /// valid kind, so there is nothing to reject.
    pub const fn new(kind_code: u32) -> Self {
        Tag { kind_code }
    }

    /// The coarse kind code this tag carries.
    pub const fn kind_code(&self) -> u32 {
        self.kind_code
    }
}

impl Reflect for Tag {
    const SCHEMA: TypeSchema = Tag::SCHEMA;

    fn reflect_write(&self, writer: &mut BinaryWriter) {
        self.kind_code.reflect_write(writer);
    }

    fn reflect_read(reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
        u32::reflect_read(reader).map(Tag::new)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_stores_kind_code() {
        assert_eq!(Tag::new(7).kind_code(), 7);
    }

    #[test]
    fn schema_names_the_kind_field() {
        assert_eq!(Tag::SCHEMA.name(), "Tag");
        assert_eq!(Tag::SCHEMA.fields().len(), 1);
        assert_eq!(Tag::SCHEMA.fields()[0].name(), "kind_code");
        assert_eq!(<Tag as Reflect>::SCHEMA.name(), "Tag");
    }

    #[test]
    fn reflect_round_trips_and_rejects_truncation() {
        let t = Tag::new(42);
        let mut w = BinaryWriter::new();
        t.reflect_write(&mut w);
        let got = Tag::reflect_read(&mut BinaryReader::new(&w.into_bytes())).unwrap();
        assert_eq!(got, t);
        assert!(Tag::reflect_read(&mut BinaryReader::new(&[])).is_err());
    }

    #[test]
    fn derives_are_exercised() {
        let a = Tag::new(1);
        assert_eq!(a, a);
        assert_ne!(a, Tag::new(2));
        assert!(format!("{a:?}").contains("Tag"));
    }
}
