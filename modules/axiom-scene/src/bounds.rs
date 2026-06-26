//! Bounds scene component: a node's axis-aligned bounding box for spatial queries.

use axiom_kernel::{BinaryReader, BinaryWriter, FieldSchema, KernelResult, Reflect, TypeSchema};
use axiom_math::Vec3;

/// An axis-aligned bounding volume attached to a node, the queryable spatial
/// extent the scene's raycast / overlap queries fold over.
///
/// This is a **spatial-query** primitive (picking, line-of-sight, overlap) — the
/// "scene bounding volumes" the math layer is built to serve — *not* a physics
/// collider: the scene owns no rigid bodies, forces, or collision response. The
/// box is a set of half-extents in the node's *local* unit frame; the queries
/// size it by the node's world scale and center it at the node's world
/// translation. World *rotation* is not modeled — v1 bounds are axis-aligned
/// (see `docs/game-vocabulary.md`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    half_extents: Vec3,
}

impl Bounds {
    /// The reflected shape of a bounds component.
    pub const SCHEMA: TypeSchema =
        TypeSchema::new("Bounds", &[FieldSchema::new("half_extents", "Vec3")]);

    /// A bounding box with the given local half-extents. Plain data: a degenerate
    /// (negative or non-finite) extent simply yields no queryable box and is
    /// skipped by the spatial queries, so there is nothing to reject here.
    pub const fn new(half_extents: Vec3) -> Self {
        Bounds { half_extents }
    }

    /// The bounding box's local half-extents.
    pub const fn half_extents(&self) -> Vec3 {
        self.half_extents
    }
}

impl Reflect for Bounds {
    const SCHEMA: TypeSchema = Bounds::SCHEMA;

    fn reflect_write(&self, writer: &mut BinaryWriter) {
        self.half_extents.x.reflect_write(writer);
        self.half_extents.y.reflect_write(writer);
        self.half_extents.z.reflect_write(writer);
    }

    fn reflect_read(reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
        f32::reflect_read(reader).and_then(|x| {
            f32::reflect_read(reader)
                .and_then(|y| f32::reflect_read(reader).map(|z| Bounds::new(Vec3::new(x, y, z))))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_stores_half_extents() {
        let b = Bounds::new(Vec3::new(0.5, 1.0, 2.0));
        assert_eq!(b.half_extents(), Vec3::new(0.5, 1.0, 2.0));
    }

    #[test]
    fn schema_names_the_bounds_field() {
        assert_eq!(Bounds::SCHEMA.name(), "Bounds");
        assert_eq!(Bounds::SCHEMA.fields().len(), 1);
        assert_eq!(Bounds::SCHEMA.fields()[0].name(), "half_extents");
        // The Reflect schema is the same constant.
        assert_eq!(<Bounds as Reflect>::SCHEMA.name(), "Bounds");
    }

    #[test]
    fn reflect_round_trips_and_rejects_truncation() {
        let b = Bounds::new(Vec3::new(1.0, 2.0, 3.0));
        let mut w = BinaryWriter::new();
        b.reflect_write(&mut w);
        let got = Bounds::reflect_read(&mut BinaryReader::new(&w.into_bytes())).unwrap();
        assert_eq!(got, b);
        // A truncated buffer is a clean error, not a panic.
        assert!(Bounds::reflect_read(&mut BinaryReader::new(&[])).is_err());
    }
}
