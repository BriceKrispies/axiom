//! The neutral mesh buffer a mesh recipe evaluates to.

use axiom_math::{Vec2, Vec3};

/// The largest vertex count a mesh operator may produce. Generators clamp their
/// subdivision so no recipe can ask for an unbounded mesh; operators that would
/// exceed this fail rather than allocate without bound.
pub const MAX_VERTS: usize = 100_000;

/// A generated mesh: parallel position / normal / uv streams (one entry per
/// vertex) and a triangle-list index buffer. This is the neutral output an app
/// translates into `axiom::MeshData`; it names no engine type.
///
/// A mesh may optionally carry **skin streams** — a `joints` (four bone indices)
/// and `weights` (four blend weights) entry per vertex — for skeletal skinning.
/// Both are empty on a static mesh and are ignored by every non-skinned path.
#[derive(Debug, Clone, PartialEq)]
pub struct MeshBuffer {
    positions: Vec<Vec3>,
    normals: Vec<Vec3>,
    uvs: Vec<Vec2>,
    indices: Vec<u32>,
    joints: Vec<[u16; 4]>,
    weights: Vec<[f32; 4]>,
}

impl MeshBuffer {
    /// Build a mesh from its streams. `None` if the streams disagree on vertex
    /// count, the index buffer is not a whole number of triangles, an index is
    /// out of range, or the vertex count exceeds [`MAX_VERTS`].
    pub fn from_parts(positions: Vec<Vec3>, normals: Vec<Vec3>, uvs: Vec<Vec2>, indices: Vec<u32>) -> Option<Self> {
        let n = positions.len();
        let aligned = (normals.len() == n) & (uvs.len() == n);
        let bounded = n <= MAX_VERTS;
        let triangular = indices.len().is_multiple_of(3);
        let in_range = indices.iter().all(|&i| (i as usize) < n);
        (aligned & bounded & triangular & in_range)
            .then_some(())
            .map(|()| Self { positions, normals, uvs, indices, joints: Vec::new(), weights: Vec::new() })
    }

    /// Build a **skinned** mesh: the static streams plus one `joints` (four bone
    /// indices) and `weights` (four blend weights) entry per vertex. `None` on
    /// any static-stream failure (as [`Self::from_parts`]) or if the skin streams
    /// disagree with the vertex count.
    pub fn from_parts_skinned(
        positions: Vec<Vec3>,
        normals: Vec<Vec3>,
        uvs: Vec<Vec2>,
        joints: Vec<[u16; 4]>,
        weights: Vec<[f32; 4]>,
        indices: Vec<u32>,
    ) -> Option<Self> {
        let n = positions.len();
        let skin_aligned = (joints.len() == n) & (weights.len() == n);
        Self::from_parts(positions, normals, uvs, indices)
            .filter(|_| skin_aligned)
            .map(move |m| Self { joints, weights, ..m })
    }

    /// The vertex positions.
    pub fn positions(&self) -> &[Vec3] {
        &self.positions
    }

    /// The per-vertex normals.
    pub fn normals(&self) -> &[Vec3] {
        &self.normals
    }

    /// The per-vertex texture coordinates.
    pub fn uvs(&self) -> &[Vec2] {
        &self.uvs
    }

    /// The triangle-list indices.
    pub fn indices(&self) -> &[u32] {
        &self.indices
    }

    /// The number of vertices.
    pub fn vertex_count(&self) -> usize {
        self.positions.len()
    }

    /// The number of triangles.
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// The per-vertex bone indices (four per vertex); empty on a static mesh.
    pub fn joints(&self) -> &[[u16; 4]] {
        &self.joints
    }

    /// The per-vertex blend weights (four per vertex); empty on a static mesh.
    pub fn weights(&self) -> &[[f32; 4]] {
        &self.weights
    }

    /// Whether this mesh carries skin streams (is deformed by a skeleton).
    pub fn is_skinned(&self) -> bool {
        !self.joints.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tri() -> MeshBuffer {
        MeshBuffer::from_parts(
            vec![Vec3::ZERO, Vec3::UNIT_X, Vec3::UNIT_Y],
            vec![Vec3::UNIT_Z; 3],
            vec![Vec2::new(0.0, 0.0); 3],
            vec![0, 1, 2],
        )
        .unwrap()
    }

    #[test]
    fn from_parts_builds_and_reports_counts() {
        let m = tri();
        assert_eq!(m.vertex_count(), 3);
        assert_eq!(m.triangle_count(), 1);
        assert_eq!(m.positions().len(), 3);
        assert_eq!(m.normals().len(), 3);
        assert_eq!(m.uvs().len(), 3);
        assert_eq!(m.indices(), &[0, 1, 2]);
    }

    #[test]
    fn a_static_mesh_carries_no_skin_streams() {
        let m = tri();
        assert!(!m.is_skinned());
        assert!(m.joints().is_empty());
        assert!(m.weights().is_empty());
    }

    #[test]
    fn from_parts_skinned_attaches_aligned_skin_streams() {
        let m = MeshBuffer::from_parts_skinned(
            vec![Vec3::ZERO, Vec3::UNIT_X, Vec3::UNIT_Y],
            vec![Vec3::UNIT_Z; 3],
            vec![Vec2::new(0.0, 0.0); 3],
            vec![[0, 1, 0, 0]; 3],
            vec![[1.0, 0.0, 0.0, 0.0]; 3],
            vec![0, 1, 2],
        )
        .unwrap();
        assert!(m.is_skinned());
        assert_eq!(m.joints().len(), 3);
        assert_eq!(m.weights()[0], [1.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn from_parts_skinned_rejects_misaligned_skin_streams() {
        // Skin streams shorter than the vertex count.
        assert!(MeshBuffer::from_parts_skinned(
            vec![Vec3::ZERO; 3],
            vec![Vec3::UNIT_Z; 3],
            vec![Vec2::new(0.0, 0.0); 3],
            vec![[0, 0, 0, 0]; 2],
            vec![[1.0, 0.0, 0.0, 0.0]; 2],
            vec![0, 1, 2],
        )
        .is_none());
    }

    #[test]
    fn from_parts_rejects_malformed_meshes() {
        // Stream length mismatch.
        assert!(MeshBuffer::from_parts(vec![Vec3::ZERO], vec![], vec![], vec![]).is_none());
        // Non-triangular index buffer.
        assert!(MeshBuffer::from_parts(vec![Vec3::ZERO; 3], vec![Vec3::UNIT_Z; 3], vec![Vec2::new(0.0, 0.0); 3], vec![0, 1]).is_none());
        // Out-of-range index.
        assert!(MeshBuffer::from_parts(vec![Vec3::ZERO; 3], vec![Vec3::UNIT_Z; 3], vec![Vec2::new(0.0, 0.0); 3], vec![0, 1, 9]).is_none());
    }
}
