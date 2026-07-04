//! The neutral mesh buffer a mesh recipe evaluates to.

use axiom_math::{Vec2, Vec3};

/// The largest vertex count a mesh operator may produce. Generators clamp their
/// subdivision so no recipe can ask for an unbounded mesh; operators that would
/// exceed this fail rather than allocate without bound.
pub const MAX_VERTS: usize = 100_000;

/// A generated mesh: parallel position / normal / uv streams (one entry per
/// vertex) and a triangle-list index buffer. This is the neutral output an app
/// translates into `axiom::MeshData`; it names no engine type.
#[derive(Debug, Clone, PartialEq)]
pub struct MeshBuffer {
    positions: Vec<Vec3>,
    normals: Vec<Vec3>,
    uvs: Vec<Vec2>,
    indices: Vec<u32>,
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
            .map(|()| Self { positions, normals, uvs, indices })
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
    fn from_parts_rejects_malformed_meshes() {
        // Stream length mismatch.
        assert!(MeshBuffer::from_parts(vec![Vec3::ZERO], vec![], vec![], vec![]).is_none());
        // Non-triangular index buffer.
        assert!(MeshBuffer::from_parts(vec![Vec3::ZERO; 3], vec![Vec3::UNIT_Z; 3], vec![Vec2::new(0.0, 0.0); 3], vec![0, 1]).is_none());
        // Out-of-range index.
        assert!(MeshBuffer::from_parts(vec![Vec3::ZERO; 3], vec![Vec3::UNIT_Z; 3], vec![Vec2::new(0.0, 0.0); 3], vec![0, 1, 9]).is_none());
    }
}
