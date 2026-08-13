//! The canonical versioned binary encoding of a [`crate::Mesh`].
//!
//! This module owns **the** byte shape of a mesh. It is the single definition of
//! "what a mesh is, as bytes", and everything that needs canonical mesh bytes —
//! storage, transport, and the [`crate::digest`] identity — goes through
//! [`write_mesh`]. There is deliberately no second encoding: a digest that
//! disagreed with what was serialized would be worse than no digest at all.
//!
//! # Layout
//!
//! Every value is written with a [`BinaryWriter`] primitive, so the encoding is
//! little-endian on every platform. No memory representation, padding byte, or
//! pointer ever reaches the buffer.
//!
//! ```text
//! SchemaVersion   major: u16, minor: u16      (MESH_SCHEMA_VERSION)
//! vertex_count    u32
//! index_count     u32
//! presence        u32  bitmask, see below
//! positions       vertex_count x Vec3   (3 x f32)      always present
//! indices         index_count  x u32                   always present
//! normals         vertex_count x Vec3   (3 x f32)      iff bit 0
//! uvs             vertex_count x Vec2   (2 x f32)      iff bit 1
//! tangents        vertex_count x Vec4   (4 x f32)      iff bit 2
//! colors          vertex_count x Vec4   (4 x f32)      iff bit 3
//! joints          vertex_count x [u16; 4]              iff bit 4
//! weights         vertex_count x [f32; 4]              iff bit 4
//! ```
//!
//! The presence bitmask is what makes stream **absence** an encoded fact rather
//! than an inference from a length. A mesh carrying an all-zero colour stream
//! and a mesh carrying no colour stream are different meshes, and they produce
//! different bytes. Joints and weights share bit 4 because the mesh contract
//! guarantees they are present together or absent together.
//!
//! # Floating-point exactness
//!
//! `f32` values are written as their IEEE-754 bit patterns. `-0.0` and `+0.0`
//! are therefore **different bytes**, and a mesh differing only in the sign of a
//! zero serializes differently and digests differently. That is correct and
//! deliberate: this encoding reports what the mesh actually holds, and silently
//! normalizing a sign would make the bytes disagree with the value they claim to
//! describe.

use axiom_kernel::{BinaryReader, BinaryWriter, KernelResult, SchemaVersion};
use axiom_math::{Vec2, Vec3, Vec4};

use crate::mesh::Mesh;
use crate::mesh_error::MeshError;
use crate::mesh_error_code::MeshErrorCode;
use crate::mesh_result::MeshResult;
use crate::mesh_streams::MeshStreams;

/// The version of the mesh binary encoding produced by [`write_mesh`].
///
/// [`read_mesh`] accepts any buffer sharing this **major** version (the kernel's
/// [`SchemaVersion::is_compatible_with`] rule): a minor bump may append data a
/// reader can ignore, a major bump may not.
pub const MESH_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);

/// Presence bit for the normal stream.
const NORMALS_BIT: u32 = 0;
/// Presence bit for the texture-coordinate stream.
const UVS_BIT: u32 = 1;
/// Presence bit for the tangent stream.
const TANGENTS_BIT: u32 = 2;
/// Presence bit for the vertex-colour stream.
const COLORS_BIT: u32 = 3;
/// Presence bit for the paired skin streams (joints **and** weights).
const SKIN_BIT: u32 = 4;

/// The presence bitmask for `mesh`, built arithmetically from its stream flags.
fn presence_mask(mesh: &Mesh) -> u32 {
    (u32::from(mesh.has_normals()) << NORMALS_BIT)
        | (u32::from(mesh.has_uvs()) << UVS_BIT)
        | (u32::from(mesh.has_tangents()) << TANGENTS_BIT)
        | (u32::from(mesh.has_colors()) << COLORS_BIT)
        | (u32::from(mesh.is_skinned()) << SKIN_BIT)
}

/// How many entries an optional stream contributes: `vertex_count` when its
/// presence bit is set, zero otherwise.
const fn stream_len(mask: u32, bit: u32, vertex_count: usize) -> usize {
    vertex_count * ((mask >> bit) & 1) as usize
}

/// Write one skin joint row: four little-endian `u16`s.
fn write_joint_row(row: &[u16; 4], writer: &mut BinaryWriter) {
    row.iter().for_each(|&joint| writer.write_u16(joint));
}

/// Write one skin weight row: four little-endian `f32`s.
fn write_weight_row(row: &[f32; 4], writer: &mut BinaryWriter) {
    row.iter().for_each(|&weight| writer.write_f32(weight));
}

/// Append the canonical byte encoding of `mesh` to `writer`.
///
/// The encoding is self-describing (version, counts, presence bitmask) and total:
/// every mesh has exactly one byte encoding, and byte-equal encodings come only
/// from equal meshes. See the module documentation for the layout.
pub(crate) fn write_mesh(mesh: &Mesh, writer: &mut BinaryWriter) {
    MESH_SCHEMA_VERSION.write_to(writer);
    writer.write_u32(mesh.vertex_count() as u32);
    writer.write_u32(mesh.indices().len() as u32);
    writer.write_u32(presence_mask(mesh));
    mesh.positions().iter().for_each(|p| p.write_to(writer));
    mesh.indices().iter().for_each(|&i| writer.write_u32(i));
    mesh.normals().iter().for_each(|n| n.write_to(writer));
    mesh.uvs().iter().for_each(|uv| uv.write_to(writer));
    mesh.tangents().iter().for_each(|t| t.write_to(writer));
    mesh.colors().iter().for_each(|c| c.write_to(writer));
    mesh.joints()
        .iter()
        .for_each(|row| write_joint_row(row, writer));
    mesh.weights()
        .iter()
        .for_each(|row| write_weight_row(row, writer));
}

/// Read one triangle index.
///
/// The kernel's `read_u32` is an inherent method, whose lifetime shape does not
/// coerce to the higher-ranked function pointer [`read_stream`] takes; this free
/// function is that adapter, matching the shape `Vec3::read_from` already has.
fn read_index(reader: &mut BinaryReader<'_>) -> KernelResult<u32> {
    reader.read_u32()
}

/// Read one skin joint row: four little-endian `u16`s.
fn read_joint_row(reader: &mut BinaryReader<'_>) -> KernelResult<[u16; 4]> {
    reader.read_u16().and_then(|a| {
        reader.read_u16().and_then(|b| {
            reader
                .read_u16()
                .and_then(|c| reader.read_u16().map(|d| [a, b, c, d]))
        })
    })
}

/// Read one skin weight row: four little-endian `f32`s.
fn read_weight_row(reader: &mut BinaryReader<'_>) -> KernelResult<[f32; 4]> {
    reader.read_f32().and_then(|a| {
        reader.read_f32().and_then(|b| {
            reader
                .read_f32()
                .and_then(|c| reader.read_f32().map(|d| [a, b, c, d]))
        })
    })
}

/// Read exactly `count` items with `read_one`, failing on the first short read.
///
/// The reserved capacity is clamped by the reader's remaining byte count: every
/// item this module reads occupies at least four bytes, so `remaining` is a hard
/// upper bound on how many can still be there. A corrupt buffer declaring a
/// four-billion-vertex mesh therefore fails on a bounds-checked read instead of
/// asking the allocator for the whole fiction up front.
fn read_stream<T>(
    reader: &mut BinaryReader<'_>,
    count: usize,
    read_one: fn(&mut BinaryReader<'_>) -> KernelResult<T>,
) -> KernelResult<Vec<T>> {
    let capacity = count.min(reader.remaining());
    (0..count).try_fold(Vec::with_capacity(capacity), |mut items, _| {
        read_one(reader).map(|item| {
            items.push(item);
            items
        })
    })
}

/// Read `vertex_count`, `index_count`, and the presence bitmask.
fn read_header(reader: &mut BinaryReader<'_>) -> KernelResult<(usize, usize, u32)> {
    reader.read_u32().and_then(|vertex_count| {
        reader.read_u32().and_then(|index_count| {
            reader
                .read_u32()
                .map(|mask| (vertex_count as usize, index_count as usize, mask))
        })
    })
}

/// Read the optional streams in canonical order and assemble the full streams.
fn read_optional_streams(
    reader: &mut BinaryReader<'_>,
    vertex_count: usize,
    mask: u32,
    positions: Vec<Vec3>,
    indices: Vec<u32>,
) -> KernelResult<MeshStreams> {
    read_stream(
        reader,
        stream_len(mask, NORMALS_BIT, vertex_count),
        Vec3::read_from,
    )
    .and_then(|normals| {
        read_stream(
            reader,
            stream_len(mask, UVS_BIT, vertex_count),
            Vec2::read_from,
        )
        .and_then(|uvs| {
            read_stream(
                reader,
                stream_len(mask, TANGENTS_BIT, vertex_count),
                Vec4::read_from,
            )
            .and_then(|tangents| {
                read_stream(
                    reader,
                    stream_len(mask, COLORS_BIT, vertex_count),
                    Vec4::read_from,
                )
                .and_then(|colors| {
                    read_stream(
                        reader,
                        stream_len(mask, SKIN_BIT, vertex_count),
                        read_joint_row,
                    )
                    .and_then(|joints| {
                        read_stream(
                            reader,
                            stream_len(mask, SKIN_BIT, vertex_count),
                            read_weight_row,
                        )
                        .map(|weights| MeshStreams {
                            positions,
                            indices,
                            normals,
                            uvs,
                            tangents,
                            colors,
                            joints,
                            weights,
                        })
                    })
                })
            })
        })
    })
}

/// Read the counted body: header, positions, indices, then the optional streams.
fn read_body(reader: &mut BinaryReader<'_>) -> KernelResult<MeshStreams> {
    read_header(reader).and_then(|(vertex_count, index_count, mask)| {
        read_stream(reader, vertex_count, Vec3::read_from).and_then(|positions| {
            read_stream(reader, index_count, read_index).and_then(|indices| {
                read_optional_streams(reader, vertex_count, mask, positions, indices)
            })
        })
    })
}

/// Reject a buffer whose declared version this build cannot decode.
fn check_version(version: SchemaVersion) -> MeshResult<()> {
    version
        .is_compatible_with(MESH_SCHEMA_VERSION)
        .then_some(())
        .ok_or_else(|| {
            MeshError::new(
                MeshErrorCode::DeserializationFailed,
                "the buffer's mesh schema major version is not decodable by this build",
            )
        })
}

/// Decode a mesh previously written by [`write_mesh`].
///
/// Three things can go wrong, and each is a deterministic
/// [`MeshErrorCode::DeserializationFailed`]:
///
/// - the declared [`SchemaVersion`] is incompatible — reported without a kernel
///   cause, because nothing in the kernel failed;
/// - the buffer is short at any point — reported with the kernel reader's fault
///   as the wrapped cause, and the reader is left parked at the failing read, so
///   [`BinaryReader::position`] says exactly where the data ran out;
/// - the bytes decode but describe an illegal mesh (an out-of-range index, a
///   misaligned stream, unnormalized skin weights). The final step is
///   [`Mesh::from_streams`], so a corrupt-but-readable buffer is rejected with
///   the specific structural code rather than yielding an invalid `Mesh`.
pub(crate) fn read_mesh(reader: &mut BinaryReader<'_>) -> MeshResult<Mesh> {
    SchemaVersion::read_from(reader)
        .map_err(|cause| {
            MeshError::with_kernel(
                MeshErrorCode::DeserializationFailed,
                "the mesh schema version could not be read",
                cause,
            )
        })
        .and_then(check_version)
        .and_then(|()| {
            read_body(reader).map_err(|cause| {
                MeshError::with_kernel(
                    MeshErrorCode::DeserializationFailed,
                    "the mesh body ran past the end of the buffer",
                    cause,
                )
            })
        })
        .and_then(Mesh::from_streams)
}

/// The canonical byte encoding of `mesh`.
///
/// This is the public serialization boundary. It hands back an owned buffer
/// rather than taking a `&mut BinaryWriter`, so the layer's public surface stays
/// free of caller-supplied mutable state — the writer is an implementation
/// detail of the encoding, not part of the contract. Feeding the same mesh in
/// twice yields byte-identical output on every platform (the kernel writer is
/// little-endian everywhere), which is what makes [`crate::digest`] stable.
pub fn encode_mesh(mesh: &Mesh) -> Vec<u8> {
    let mut writer = BinaryWriter::new();
    write_mesh(mesh, &mut writer);
    writer.into_bytes()
}

/// Rebuild a mesh from its canonical byte encoding.
///
/// Fails with [`MeshErrorCode::DeserializationFailed`] on an incompatible schema
/// version or a truncated buffer, and with the specific structural code when the
/// bytes decode but describe an illegal mesh — the final step is
/// [`Mesh::from_streams`], so no invalid `Mesh` can be produced by decoding.
pub fn decode_mesh(bytes: &[u8]) -> MeshResult<Mesh> {
    read_mesh(&mut BinaryReader::new(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn positions() -> Vec<Vec3> {
        vec![Vec3::ZERO, Vec3::UNIT_X, Vec3::UNIT_Y]
    }

    #[test]
    fn encode_and_decode_round_trip_every_stream() {
        let original = every_stream();
        let bytes = encode_mesh(&original);
        assert_eq!(decode_mesh(&bytes).unwrap(), original);
    }

    #[test]
    fn encode_matches_the_writer_it_wraps() {
        // The public boundary must not become a second encoding: whatever
        // `encode_mesh` hands back is exactly what `write_mesh` produces, which
        // is what lets `crate::digest` be defined in terms of it.
        let mesh = every_stream();
        let mut writer = BinaryWriter::new();
        write_mesh(&mesh, &mut writer);
        assert_eq!(encode_mesh(&mesh), writer.as_bytes());
    }

    #[test]
    fn encoding_is_reproducible() {
        let mesh = every_stream();
        assert_eq!(encode_mesh(&mesh), encode_mesh(&mesh));
    }

    #[test]
    fn decode_preserves_the_absence_of_optional_streams() {
        let decoded = decode_mesh(&encode_mesh(&minimal())).unwrap();
        assert!(!decoded.has_normals());
        assert!(!decoded.has_uvs());
        assert!(!decoded.has_tangents());
        assert!(!decoded.has_colors());
        assert!(!decoded.is_skinned());
        assert_eq!(decoded.positions(), &positions()[..]);
    }

    #[test]
    fn decode_rejects_a_truncated_buffer_with_the_kernel_cause() {
        let bytes = encode_mesh(&every_stream());
        let err = decode_mesh(&bytes[..bytes.len() - 4]).unwrap_err();
        assert_eq!(err.code(), MeshErrorCode::DeserializationFailed);
        assert!(err.kernel().is_some());
    }

    #[test]
    fn decode_rejects_an_empty_buffer() {
        let err = decode_mesh(&[]).unwrap_err();
        assert_eq!(err.code(), MeshErrorCode::DeserializationFailed);
    }

    fn minimal() -> Mesh {
        Mesh::from_streams(MeshStreams::new(positions(), vec![0, 1, 2])).unwrap()
    }

    fn every_stream() -> Mesh {
        Mesh::from_streams(MeshStreams {
            normals: vec![Vec3::UNIT_Z, Vec3::UNIT_Y, Vec3::UNIT_X],
            uvs: vec![
                Vec2::ZERO,
                Vec2::new(1.0, 0.0),
                Vec2::new(0.25, 0.75),
            ],
            tangents: vec![
                Vec4::new(1.0, 0.0, 0.0, 1.0),
                Vec4::new(0.0, 1.0, 0.0, -1.0),
                Vec4::new(0.0, 0.0, 1.0, 1.0),
            ],
            colors: vec![
                Vec4::new(0.1, 0.2, 0.3, 1.0),
                Vec4::ONE,
                Vec4::new(0.9, 0.8, 0.7, 0.5),
            ],
            joints: vec![[0, 1, 2, 3], [4, 5, 6, 7], [8, 9, 10, 11]],
            weights: vec![
                [1.0, 0.0, 0.0, 0.0],
                [0.5, 0.5, 0.0, 0.0],
                [0.25, 0.25, 0.25, 0.25],
            ],
            ..MeshStreams::new(positions(), vec![0, 1, 2])
        })
        .unwrap()
    }

    fn encode(mesh: &Mesh) -> Vec<u8> {
        let mut writer = BinaryWriter::new();
        write_mesh(mesh, &mut writer);
        writer.into_bytes()
    }

    fn decode(bytes: &[u8]) -> MeshResult<Mesh> {
        read_mesh(&mut BinaryReader::new(bytes))
    }

    #[test]
    fn the_schema_version_starts_at_one_zero() {
        assert_eq!(MESH_SCHEMA_VERSION, SchemaVersion::new(1, 0));
        assert_eq!(MESH_SCHEMA_VERSION.major(), 1);
        assert_eq!(MESH_SCHEMA_VERSION.minor(), 0);
    }

    #[test]
    fn the_header_is_version_then_counts_then_the_presence_mask() {
        let bytes = encode(&minimal());
        // major=1, minor=0, vertex_count=3, index_count=3, mask=0.
        assert_eq!(
            &bytes[..16],
            &[1, 0, 0, 0, 3, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0]
        );
        // 16 header + 3 positions * 12 + 3 indices * 4.
        assert_eq!(bytes.len(), 64);
    }

    #[test]
    fn the_presence_mask_records_each_stream_in_its_own_bit() {
        let bytes = encode(&every_stream());
        assert_eq!(&bytes[12..16], &[0b0001_1111, 0, 0, 0]);
        // 64 (minimal) + normals 36 + uvs 24 + tangents 48 + colors 48
        //              + joints 24 + weights 48.
        assert_eq!(bytes.len(), 292);

        let uvs_only = Mesh::from_streams(MeshStreams {
            uvs: vec![Vec2::ZERO; 3],
            ..MeshStreams::new(positions(), vec![0, 1, 2])
        })
        .unwrap();
        assert_eq!(&encode(&uvs_only)[12..16], &[0b0000_0010, 0, 0, 0]);
    }

    #[test]
    fn a_mesh_with_every_stream_round_trips_exactly() {
        let original = every_stream();
        let recovered = decode(&encode(&original)).unwrap();
        assert_eq!(recovered, original);
        assert_eq!(recovered.normals()[0], Vec3::UNIT_Z);
        assert_eq!(recovered.uvs()[2], Vec2::new(0.25, 0.75));
        assert_eq!(recovered.tangents()[1].w, -1.0);
        assert_eq!(recovered.colors()[2], Vec4::new(0.9, 0.8, 0.7, 0.5));
        assert_eq!(recovered.joints()[2], [8, 9, 10, 11]);
        assert_eq!(recovered.weights()[2], [0.25, 0.25, 0.25, 0.25]);
    }

    #[test]
    fn a_minimal_mesh_round_trips_and_keeps_its_streams_absent() {
        let recovered = decode(&encode(&minimal())).unwrap();
        assert_eq!(recovered, minimal());
        assert_eq!(recovered.positions(), positions().as_slice());
        assert_eq!(recovered.indices(), &[0, 1, 2]);
        assert!(!recovered.has_normals());
        assert!(!recovered.has_uvs());
        assert!(!recovered.has_tangents());
        assert!(!recovered.has_colors());
        assert!(!recovered.is_skinned());
    }

    #[test]
    fn re_encoding_a_decoded_mesh_reproduces_the_same_bytes() {
        let bytes = encode(&every_stream());
        assert_eq!(encode(&decode(&bytes).unwrap()), bytes);
    }

    #[test]
    fn signed_zero_survives_the_round_trip_unnormalized() {
        let mesh = Mesh::from_streams(MeshStreams::new(
            vec![Vec3::new(-0.0, 0.0, 0.0), Vec3::UNIT_X, Vec3::UNIT_Y],
            vec![0, 1, 2],
        ))
        .unwrap();
        let recovered = decode(&encode(&mesh)).unwrap();
        assert!(recovered.positions()[0].x.is_sign_negative());
        assert_ne!(encode(&mesh), encode(&minimal()));
    }

    #[test]
    fn truncating_the_buffer_anywhere_fails_with_a_kernel_cause() {
        let bytes = encode(&every_stream());
        for cut in 0..bytes.len() {
            let err = decode(&bytes[..cut]).unwrap_err();
            assert_eq!(
                err.code(),
                MeshErrorCode::DeserializationFailed,
                "prefix of {cut} bytes should not decode"
            );
            assert!(
                err.kernel().is_some(),
                "prefix of {cut} bytes should carry the kernel reader cause"
            );
        }
        assert!(decode(&bytes).is_ok());
    }

    #[test]
    fn an_incompatible_major_version_is_rejected_without_a_kernel_cause() {
        let mut bytes = encode(&minimal());
        bytes[0] = 2; // major 2
        let err = decode(&bytes).unwrap_err();
        assert_eq!(err.code(), MeshErrorCode::DeserializationFailed);
        assert_eq!(err.kernel(), None);
    }

    #[test]
    fn a_differing_minor_version_is_still_decodable() {
        let mut bytes = encode(&minimal());
        bytes[2] = 9; // minor 9, same major
        assert_eq!(decode(&bytes).unwrap(), minimal());
    }

    #[test]
    fn a_readable_buffer_that_breaks_the_mesh_contract_is_rejected_structurally() {
        // Same bytes, but the last index addresses a vertex that does not exist.
        let mut bytes = encode(&minimal());
        let last = bytes.len() - 4;
        bytes[last..].copy_from_slice(&9u32.to_le_bytes());
        let err = decode(&bytes).unwrap_err();
        assert_eq!(err.code(), MeshErrorCode::IndexOutOfRange);
        assert_eq!(err.kernel(), None);
    }

    #[test]
    fn a_buffer_declaring_no_vertices_is_rejected_as_an_empty_mesh() {
        let mut writer = BinaryWriter::new();
        MESH_SCHEMA_VERSION.write_to(&mut writer);
        writer.write_u32(0);
        writer.write_u32(0);
        writer.write_u32(0);
        let err = decode(&writer.into_bytes()).unwrap_err();
        assert_eq!(err.code(), MeshErrorCode::EmptyPositions);
    }

    #[test]
    fn an_absurd_declared_vertex_count_fails_on_a_bounds_check() {
        let mut writer = BinaryWriter::new();
        MESH_SCHEMA_VERSION.write_to(&mut writer);
        writer.write_u32(u32::MAX);
        writer.write_u32(0);
        writer.write_u32(0);
        let err = decode(&writer.into_bytes()).unwrap_err();
        assert_eq!(err.code(), MeshErrorCode::DeserializationFailed);
        assert!(err.kernel().is_some());
    }

    #[test]
    fn a_readable_payload_with_unnormalized_skin_weights_is_rejected() {
        let mut writer = BinaryWriter::new();
        MESH_SCHEMA_VERSION.write_to(&mut writer);
        writer.write_u32(3);
        writer.write_u32(0);
        writer.write_u32(1 << SKIN_BIT);
        positions().iter().for_each(|p| p.write_to(&mut writer));
        (0..3).for_each(|_| write_joint_row(&[0, 0, 0, 0], &mut writer));
        (0..3).for_each(|_| write_weight_row(&[0.5, 0.0, 0.0, 0.0], &mut writer));
        let err = decode(&writer.into_bytes()).unwrap_err();
        assert_eq!(err.code(), MeshErrorCode::SkinWeightsNotNormalized);
        assert_eq!(err.kernel(), None);
    }

    #[test]
    fn skin_streams_share_one_presence_bit() {
        let bytes = encode(&every_stream());
        assert_eq!((bytes[12] >> SKIN_BIT) & 1, 1);
        let recovered = decode(&bytes).unwrap();
        assert_eq!(recovered.joints().len(), 3);
        assert_eq!(recovered.weights().len(), 3);
    }

    #[test]
    fn presence_mask_and_stream_len_agree_on_every_bit() {
        assert_eq!(presence_mask(&minimal()), 0);
        assert_eq!(presence_mask(&every_stream()), 0b0001_1111);
        assert_eq!(stream_len(0b0001_1111, TANGENTS_BIT, 7), 7);
        assert_eq!(stream_len(0b0001_1011, TANGENTS_BIT, 7), 0);
    }
}
