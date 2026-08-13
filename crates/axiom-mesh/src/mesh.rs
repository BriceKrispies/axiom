//! The canonical neutral CPU-side indexed triangle mesh.

use axiom_math::{Vec2, Vec3, Vec4};

use crate::mesh_result::MeshResult;
use crate::mesh_streams::MeshStreams;
use crate::mesh_validation::validate_streams;

/// Axiom's one representation of triangle geometry.
///
/// A `Mesh` is a validated set of parallel attribute streams plus a triangle-list
/// index buffer. It is **structure of arrays**, never interleaved: interleaving
/// is a GPU vertex-layout decision that belongs to a backend, and baking one
/// into the representation is exactly what produced the engine's several
/// mutually-incompatible mesh types.
///
/// # What it deliberately does not know
///
/// No material, no texture, no shader, no GPU buffer, no vertex layout, no
/// scene node, no entity, no resource id, no asset origin, no LOD policy, no
/// browser type. A mesh that came out of a glTF importer and a mesh that came
/// out of [`axiom_mesh_ops`](../axiom_mesh_ops/index.html) are the same kind of
/// value here, and nothing downstream can tell them apart. That
/// indistinguishability is the point of the type.
///
/// # Invariants
///
/// Guaranteed by construction — see [`validate_streams`] for the full contract:
/// positions are non-empty and finite; the index count is divisible by three
/// and every index is in range; each optional stream is either absent (empty)
/// or exactly one entry per vertex, with finite components; skin joints and
/// weights are present together and every weight row sums to one.
///
/// # Winding
///
/// Counter-clockwise triangles are front-facing, in a right-handed Y-up space.
/// For triangle `(a, b, c)` the geometric normal is
/// `(p[b] - p[a]).cross(p[c] - p[a])`.
#[derive(Debug, Clone, PartialEq)]
pub struct Mesh {
    streams: MeshStreams,
}

impl Mesh {
    /// Validate `streams` and take ownership of them.
    ///
    /// This is the only constructor. Every generator, importer, and mesh
    /// operation in the engine funnels through it, so no unvalidated mesh can
    /// exist.
    pub fn from_streams(streams: MeshStreams) -> MeshResult<Mesh> {
        validate_streams(&streams).map(|()| Mesh { streams })
    }

    /// Give the streams back, so a caller can rebuild a modified mesh without
    /// copying every attribute out one accessor at a time.
    pub fn into_streams(self) -> MeshStreams {
        self.streams
    }

    /// The vertex positions.
    pub fn positions(&self) -> &[Vec3] {
        &self.streams.positions
    }

    /// The triangle-list indices.
    pub fn indices(&self) -> &[u32] {
        &self.streams.indices
    }

    /// The per-vertex normals; empty when absent.
    pub fn normals(&self) -> &[Vec3] {
        &self.streams.normals
    }

    /// The per-vertex texture coordinates; empty when absent.
    pub fn uvs(&self) -> &[Vec2] {
        &self.streams.uvs
    }

    /// The per-vertex tangents (`xyz` direction, `w` handedness); empty when
    /// absent.
    pub fn tangents(&self) -> &[Vec4] {
        &self.streams.tangents
    }

    /// The per-vertex linear RGBA colours; empty when absent.
    pub fn colors(&self) -> &[Vec4] {
        &self.streams.colors
    }

    /// The per-vertex skin bone indices; empty when absent.
    pub fn joints(&self) -> &[[u16; 4]] {
        &self.streams.joints
    }

    /// The per-vertex skin blend weights; empty when absent.
    pub fn weights(&self) -> &[[f32; 4]] {
        &self.streams.weights
    }

    /// The number of vertices.
    pub fn vertex_count(&self) -> usize {
        self.streams.positions.len()
    }

    /// The number of triangles.
    pub fn triangle_count(&self) -> usize {
        self.streams.indices.len() / 3
    }

    /// Whether this mesh carries per-vertex normals.
    pub fn has_normals(&self) -> bool {
        !self.streams.normals.is_empty()
    }

    /// Whether this mesh carries texture coordinates.
    pub fn has_uvs(&self) -> bool {
        !self.streams.uvs.is_empty()
    }

    /// Whether this mesh carries tangents.
    pub fn has_tangents(&self) -> bool {
        !self.streams.tangents.is_empty()
    }

    /// Whether this mesh carries per-vertex colours.
    pub fn has_colors(&self) -> bool {
        !self.streams.colors.is_empty()
    }

    /// Whether this mesh carries skin streams (is deformed by a skeleton).
    pub fn is_skinned(&self) -> bool {
        !self.streams.joints.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh_error_code::MeshErrorCode;

    fn tri() -> Mesh {
        Mesh::from_streams(MeshStreams::new(
            vec![Vec3::ZERO, Vec3::UNIT_X, Vec3::UNIT_Y],
            vec![0, 1, 2],
        ))
        .unwrap()
    }

    #[test]
    fn a_triangle_reports_its_counts_and_streams() {
        let m = tri();
        assert_eq!(m.vertex_count(), 3);
        assert_eq!(m.triangle_count(), 1);
        assert_eq!(m.positions()[1], Vec3::UNIT_X);
        assert_eq!(m.indices(), &[0, 1, 2]);
    }

    #[test]
    fn optional_streams_are_absent_by_default() {
        let m = tri();
        assert!(!m.has_normals());
        assert!(!m.has_uvs());
        assert!(!m.has_tangents());
        assert!(!m.has_colors());
        assert!(!m.is_skinned());
        assert!(m.normals().is_empty());
        assert!(m.uvs().is_empty());
        assert!(m.tangents().is_empty());
        assert!(m.colors().is_empty());
        assert!(m.joints().is_empty());
        assert!(m.weights().is_empty());
    }

    #[test]
    fn present_streams_are_reported_and_readable() {
        let m = Mesh::from_streams(MeshStreams {
            normals: vec![Vec3::UNIT_Z; 3],
            uvs: vec![Vec2::new(0.25, 0.75); 3],
            tangents: vec![Vec4::new(1.0, 0.0, 0.0, -1.0); 3],
            colors: vec![Vec4::new(0.1, 0.2, 0.3, 1.0); 3],
            joints: vec![[7, 0, 0, 0]; 3],
            weights: vec![[1.0, 0.0, 0.0, 0.0]; 3],
            ..MeshStreams::new(vec![Vec3::ZERO; 3], vec![0, 1, 2])
        })
        .unwrap();

        assert!(m.has_normals() & m.has_uvs() & m.has_tangents() & m.has_colors());
        assert!(m.is_skinned());
        assert_eq!(m.normals()[0], Vec3::UNIT_Z);
        assert_eq!(m.uvs()[2], Vec2::new(0.25, 0.75));
        assert_eq!(m.tangents()[0].w, -1.0);
        assert_eq!(m.colors()[1], Vec4::new(0.1, 0.2, 0.3, 1.0));
        assert_eq!(m.joints()[0], [7, 0, 0, 0]);
        assert_eq!(m.weights()[0], [1.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn construction_rejects_streams_that_break_the_contract() {
        let bad = Mesh::from_streams(MeshStreams::new(vec![Vec3::ZERO; 3], vec![0, 1, 9]));
        assert_eq!(bad.unwrap_err().code(), MeshErrorCode::IndexOutOfRange);
    }

    #[test]
    fn into_streams_round_trips_every_attribute() {
        let original = MeshStreams {
            normals: vec![Vec3::UNIT_Z; 3],
            uvs: vec![Vec2::new(0.5, 0.5); 3],
            ..MeshStreams::new(vec![Vec3::ZERO, Vec3::UNIT_X, Vec3::UNIT_Y], vec![0, 1, 2])
        };
        let recovered = Mesh::from_streams(original.clone()).unwrap().into_streams();
        assert_eq!(recovered, original);
    }

    #[test]
    fn meshes_compare_by_value() {
        assert_eq!(tri(), tri());
        let other = Mesh::from_streams(MeshStreams::new(
            vec![Vec3::ZERO, Vec3::UNIT_X, Vec3::UNIT_Z],
            vec![0, 1, 2],
        ))
        .unwrap();
        assert_ne!(tri(), other);
    }
}
