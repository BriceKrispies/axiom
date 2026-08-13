//! The deterministic identity of a [`crate::Mesh`].
//!
//! A mesh digest is how the engine names geometry it did not author by hand: an
//! asset cache key, a golden-artifact fingerprint, a provenance record for a
//! generated mesh, a "did this operation change anything" check. All of those
//! need the *same* answer for the *same* geometry on every platform and every
//! run, which is exactly what the kernel's [`StableHash`] over canonical bytes
//! provides.
//!
//! # Why this reuses the serializer
//!
//! [`digest`] hashes the bytes [`write_mesh`](crate::write_mesh) produces. It
//! does not define its own encoding, and it must never be allowed to: two
//! encodings of the same value are two definitions of that value's identity, and
//! they drift the moment a stream is added. Because the digest is literally a
//! hash of the serialized form, "these meshes serialize the same" and "these
//! meshes digest the same" cannot disagree.
//!
//! # What changes a digest
//!
//! Every byte of the canonical encoding is hashed, so any change to a position
//! component, an index, an attribute value, the vertex or index count, or the
//! **presence** of an optional stream changes the digest. Presence is explicit
//! in the encoding's bitmask, so a mesh carrying an all-zero colour stream and a
//! mesh carrying no colours are distinguishable — as they must be, since they
//! are different meshes.
//!
//! `f32` components are hashed as their IEEE-754 bit patterns. `-0.0` and `+0.0`
//! have different bit patterns and therefore **digest differently**, even though
//! they compare equal as numbers. That is honest rather than convenient: the
//! digest reports the bytes the mesh actually holds, and normalizing the sign
//! would make the digest claim two different values are the same one.
//!
//! # It is an index, not a proof
//!
//! Following the kernel's stance on [`StableHash`]: byte equality is the verdict,
//! a digest match is a hint. Use [`digest`] to label, key, and locate geometry —
//! not to certify that two meshes are identical.

use axiom_kernel::{BinaryWriter, StableHash};

use crate::mesh::Mesh;
use crate::mesh_binary::write_mesh;

/// The stable 64-bit digest of `mesh`'s canonical byte encoding.
///
/// Deterministic across runs, processes, and platforms: the encoding is
/// little-endian everywhere and contains no memory layout, padding, or pointer.
/// Equal meshes always digest equally; different meshes essentially always
/// digest differently (FNV-1a is a 64-bit non-cryptographic hash — collisions
/// are astronomically unlikely, not impossible, which is why byte equality
/// remains the source of truth).
pub fn digest(mesh: &Mesh) -> StableHash {
    let mut writer = BinaryWriter::new();
    write_mesh(mesh, &mut writer);
    StableHash::of_bytes(writer.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_math::{Vec2, Vec3, Vec4};

    use crate::mesh_streams::MeshStreams;

    fn positions() -> Vec<Vec3> {
        vec![Vec3::new(1.0, 2.0, 3.0), Vec3::UNIT_X, Vec3::UNIT_Y]
    }

    fn build(streams: MeshStreams) -> Mesh {
        Mesh::from_streams(streams).unwrap()
    }

    fn triangle() -> Mesh {
        build(MeshStreams::new(positions(), vec![0, 1, 2]))
    }

    #[test]
    fn the_same_mesh_digests_the_same_every_time() {
        let mesh = triangle();
        assert_eq!(digest(&mesh), digest(&mesh));
    }

    #[test]
    fn independently_built_identical_meshes_digest_the_same() {
        assert_eq!(digest(&triangle()), digest(&triangle()));
    }

    #[test]
    fn changing_one_position_component_changes_the_digest() {
        let moved = build(MeshStreams::new(
            vec![Vec3::new(1.0, 2.000_001, 3.0), Vec3::UNIT_X, Vec3::UNIT_Y],
            vec![0, 1, 2],
        ));
        assert_ne!(digest(&triangle()), digest(&moved));
    }

    #[test]
    fn changing_one_index_changes_the_digest() {
        let rewound = build(MeshStreams::new(positions(), vec![0, 2, 1]));
        assert_ne!(digest(&triangle()), digest(&rewound));
    }

    #[test]
    fn adding_a_normal_stream_changes_the_digest() {
        let with_normals = build(MeshStreams {
            normals: vec![Vec3::UNIT_Y; 3],
            ..MeshStreams::new(positions(), vec![0, 1, 2])
        });
        assert_ne!(digest(&triangle()), digest(&with_normals));
    }

    #[test]
    fn changing_one_attribute_value_changes_the_digest() {
        let base = build(MeshStreams {
            uvs: vec![Vec2::ZERO, Vec2::new(1.0, 0.0), Vec2::new(0.0, 1.0)],
            colors: vec![Vec4::ONE; 3],
            ..MeshStreams::new(positions(), vec![0, 1, 2])
        });
        let nudged_uv = build(MeshStreams {
            uvs: vec![Vec2::ZERO, Vec2::new(1.0, 0.5), Vec2::new(0.0, 1.0)],
            colors: vec![Vec4::ONE; 3],
            ..MeshStreams::new(positions(), vec![0, 1, 2])
        });
        let nudged_color = build(MeshStreams {
            uvs: vec![Vec2::ZERO, Vec2::new(1.0, 0.0), Vec2::new(0.0, 1.0)],
            colors: vec![Vec4::ONE, Vec4::new(1.0, 1.0, 1.0, 0.5), Vec4::ONE],
            ..MeshStreams::new(positions(), vec![0, 1, 2])
        });
        assert_ne!(digest(&base), digest(&nudged_uv));
        assert_ne!(digest(&base), digest(&nudged_color));
        assert_ne!(digest(&nudged_uv), digest(&nudged_color));
    }

    #[test]
    fn a_present_all_zero_stream_differs_from_an_absent_one() {
        // The values would be zero either way; only the presence bitmask tells
        // the two apart, which is exactly why the encoding records it.
        let absent = triangle();
        let present = build(MeshStreams {
            colors: vec![Vec4::ZERO; 3],
            ..MeshStreams::new(positions(), vec![0, 1, 2])
        });
        assert_ne!(digest(&absent), digest(&present));
    }

    #[test]
    fn negative_zero_digests_differently_from_positive_zero() {
        // Documented, deliberate: the digest reports the bit patterns the mesh
        // holds. It does not normalize a signed zero away.
        let plus = build(MeshStreams::new(
            vec![Vec3::new(0.0, 0.0, 0.0), Vec3::UNIT_X, Vec3::UNIT_Y],
            vec![0, 1, 2],
        ));
        let minus = build(MeshStreams::new(
            vec![Vec3::new(-0.0, 0.0, 0.0), Vec3::UNIT_X, Vec3::UNIT_Y],
            vec![0, 1, 2],
        ));
        assert_eq!(plus.positions()[0], minus.positions()[0]);
        assert_ne!(digest(&plus), digest(&minus));
    }

    #[test]
    fn the_digest_is_the_hash_of_the_canonical_serialized_bytes() {
        let mesh = triangle();
        let mut writer = BinaryWriter::new();
        write_mesh(&mesh, &mut writer);
        assert_eq!(digest(&mesh), StableHash::of_bytes(&writer.into_bytes()));
    }
}
