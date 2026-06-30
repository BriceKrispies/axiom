//! Author-supplied mesh geometry: the value an app hands to
//! [`crate::prelude::RunningApp::add_mesh_data`] to register a mesh built from
//! explicit vertex data — the non-catalog counterpart to the built-in
//! [`crate::prelude::Mesh`] primitives (SPEC-11 §4.2 + the `MeshData` deferral,
//! §9).
//!
//! Where [`crate::prelude::Mesh`] names one of the engine's built-in primitives
//! (cube / plane / sphere / cylinder) and resolves to fixed geometry, a
//! `MeshData` carries an author's own positions, normals, optional UVs, and
//! triangle indices. The umbrella validates it (finite coordinates, one normal
//! per vertex, optional UVs matching the vertex count, a non-empty in-range
//! triangle-list) and threads the neutral geometry through the SAME
//! `axiom-resources` registration + resolution the primitives use, so an author
//! mesh is "just another set of triangles" to every backend — no special render
//! path. A malformed value is rejected with a [`MeshDataError`] before anything
//! is registered.

use axiom_math::{Vec2, Vec3};

/// Why a [`MeshData`] is not valid renderable geometry. Returned by
/// [`crate::prelude::RunningApp::add_mesh_data`]; the first failing check, in the
/// declaration order below, is the one reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeshDataError {
    /// No positions were supplied — a mesh needs at least one vertex.
    EmptyPositions,
    /// A position, normal, or UV coordinate was NaN or infinite.
    NonFinite,
    /// The normal count does not match the position count (one normal per vertex).
    NormalCountMismatch,
    /// UVs were supplied but their count does not match the position count.
    UvCountMismatch,
    /// No indices were supplied — a mesh needs at least one triangle.
    NoIndices,
    /// The index count is not a multiple of three (a triangle list).
    IndicesNotTriangles,
    /// An index addresses a vertex at or beyond the position count.
    IndexOutOfRange,
}

/// Author-supplied mesh geometry: per-vertex `positions` and `normals`, optional
/// per-vertex `uvs`, and a triangle-list `indices` into them.
///
/// Pass an empty `uvs` vector to default every vertex's texture coordinate to the
/// origin `(0, 0)`. The value is validated when it is registered through
/// [`crate::prelude::RunningApp::add_mesh_data`] — constructing a `MeshData` never
/// fails, so an author can build one up and let the engine report any geometry
/// problem at registration with a precise [`MeshDataError`].
#[derive(Debug, Clone, PartialEq)]
pub struct MeshData {
    positions: Vec<Vec3>,
    normals: Vec<Vec3>,
    uvs: Vec<Vec2>,
    indices: Vec<u32>,
}

impl MeshData {
    /// Build author mesh geometry from explicit vertex data: one position and one
    /// normal per vertex, an optional UV per vertex (pass an empty `uvs` to
    /// default them to the origin), and a triangle-list `indices` into the
    /// vertices.
    pub fn new(positions: Vec<Vec3>, normals: Vec<Vec3>, uvs: Vec<Vec2>, indices: Vec<u32>) -> Self {
        MeshData {
            positions,
            normals,
            uvs,
            indices,
        }
    }

    /// The per-vertex positions.
    pub fn positions(&self) -> &[Vec3] {
        &self.positions
    }

    /// The per-vertex normals.
    pub fn normals(&self) -> &[Vec3] {
        &self.normals
    }

    /// The per-vertex UVs (empty ⇒ every vertex defaults to the origin `(0, 0)`).
    pub fn uvs(&self) -> &[Vec2] {
        &self.uvs
    }

    /// The triangle-list indices into the vertices.
    pub fn indices(&self) -> &[u32] {
        &self.indices
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accessors_round_trip_constructed_geometry() {
        let data = MeshData::new(
            vec![Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0)],
            vec![Vec3::UNIT_Z, Vec3::UNIT_Z],
            vec![Vec2::ZERO, Vec2::new(1.0, 0.0)],
            vec![0, 1, 0],
        );
        assert_eq!(data.positions().len(), 2);
        assert_eq!(data.normals(), &[Vec3::UNIT_Z, Vec3::UNIT_Z]);
        assert_eq!(data.uvs(), &[Vec2::ZERO, Vec2::new(1.0, 0.0)]);
        assert_eq!(data.indices(), &[0, 1, 0]);
        // Clone + Debug + PartialEq (the derives) are exercised.
        assert_eq!(data.clone(), data);
        assert!(format!("{data:?}").contains("MeshData"));
    }

    #[test]
    fn empty_uvs_are_a_distinct_value() {
        let with = MeshData::new(vec![Vec3::ZERO], vec![Vec3::UNIT_Z], vec![Vec2::ONE], vec![0]);
        let without = MeshData::new(vec![Vec3::ZERO], vec![Vec3::UNIT_Z], vec![], vec![0]);
        assert!(without.uvs().is_empty());
        assert_ne!(with, without);
    }

    #[test]
    fn errors_are_distinct_and_debuggable() {
        // Kills any "all errors equal" collapse: the variants are distinct.
        assert_ne!(MeshDataError::EmptyPositions, MeshDataError::NonFinite);
        assert_eq!(MeshDataError::IndexOutOfRange, MeshDataError::IndexOutOfRange);
        assert!(format!("{:?}", MeshDataError::NoIndices).contains("NoIndices"));
    }
}
