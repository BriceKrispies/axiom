//! Render-facing mesh data (CPU-side positions/normals/uvs + indices).

use axiom_math::{Vec2, Vec3};

/// Render-facing mesh: one CPU-side mesh description the render
/// command list refers to by index.
///
/// The renderer does not know what an `axiom-resources::MeshData` is
/// — the app translates resource data into this neutral shape.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderMesh {
    /// An opaque ID the producer assigns; round-tripped to commands
    /// for backend identification.
    id: u64,
    positions: Vec<Vec3>,
    normals: Vec<Vec3>,
    uvs: Vec<Vec2>,
    indices: Vec<u32>,
}

impl RenderMesh {
    pub fn new(
        id: u64,
        positions: Vec<Vec3>,
        normals: Vec<Vec3>,
        uvs: Vec<Vec2>,
        indices: Vec<u32>,
    ) -> Self {
        RenderMesh {
            id,
            positions,
            normals,
            uvs,
            indices,
        }
    }

    pub const fn id(&self) -> u64 {
        self.id
    }

    pub fn positions(&self) -> &[Vec3] {
        &self.positions
    }

    pub fn normals(&self) -> &[Vec3] {
        &self.normals
    }

    pub fn uvs(&self) -> &[Vec2] {
        &self.uvs
    }

    pub fn indices(&self) -> &[u32] {
        &self.indices
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accessors_round_trip_constructed_values() {
        let m = RenderMesh::new(
            7,
            vec![Vec3::ZERO],
            vec![Vec3::UNIT_Y],
            vec![Vec2::ZERO],
            vec![0],
        );
        assert_eq!(m.id(), 7);
        assert_eq!(m.positions().len(), 1);
        assert_eq!(m.indices(), &[0]);
    }

    #[test]
    fn equality_requires_all_fields() {
        let a = RenderMesh::new(1, vec![], vec![], vec![], vec![]);
        let b = RenderMesh::new(1, vec![], vec![], vec![], vec![]);
        let c = RenderMesh::new(2, vec![], vec![], vec![], vec![]);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn normals_and_uvs_accessors_round_trip() {
        let m = RenderMesh::new(
            3,
            vec![Vec3::ZERO],
            vec![Vec3::UNIT_Y, Vec3::UNIT_X],
            vec![Vec2::ZERO, Vec2::ZERO, Vec2::ZERO],
            vec![0, 1, 2],
        );
        assert_eq!(m.normals(), &[Vec3::UNIT_Y, Vec3::UNIT_X]);
        assert_eq!(m.uvs().len(), 3);
    }
}
