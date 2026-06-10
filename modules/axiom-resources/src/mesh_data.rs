//! CPU-side mesh description.

use crate::resource_id::ResourceId;
use crate::vertex::Vertex;

/// One vertex of neutral mesh input: `(position, normal, uv, color)`, each a
/// plain float array. This is the engine's shape-agnostic geometry boundary —
/// an app (or a built-in primitive generator) describes a mesh in this form and
/// hands it to [`crate::resources_api::ResourcesApi::register_mesh`] without
/// naming any resource type.
pub(crate) type MeshInputVertex = ([f32; 3], [f32; 3], [f32; 2], [f32; 4]);

/// One CPU-side mesh: a stable [`ResourceId`], a debug name, a vertex
/// list, and an index list (all triangles).
#[derive(Debug, Clone, PartialEq)]
pub struct MeshData {
    id: ResourceId,
    name: &'static str,
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
}

impl MeshData {
    pub fn new(
        id: ResourceId,
        name: &'static str,
        vertices: Vec<Vertex>,
        indices: Vec<u32>,
    ) -> Self {
        MeshData {
            id,
            name,
            vertices,
            indices,
        }
    }

    pub const fn id(&self) -> ResourceId {
        self.id
    }

    pub const fn name(&self) -> &'static str {
        self.name
    }

    pub fn vertices(&self) -> &[Vertex] {
        &self.vertices
    }

    pub fn indices(&self) -> &[u32] {
        &self.indices
    }
}

/// A minimal one-vertex mesh with a chosen [`ResourceId`], for tests that need
/// *a* mesh in a table without depending on any particular geometry (e.g. the
/// resource-table and resolved-snapshot tests, which only assert ids/counts).
#[cfg(test)]
pub(crate) fn test_mesh(id: ResourceId) -> MeshData {
    use axiom_math::{Vec2, Vec3, Vec4};
    let v = Vertex::new(Vec3::ZERO, Vec3::UNIT_Z, Vec2::ZERO, Vec4::ONE);
    MeshData::new(id, "test.mesh", vec![v], vec![0])
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_math::{Vec2, Vec3, Vec4};

    #[test]
    fn accessors_round_trip_constructed_values() {
        let v = Vertex::new(Vec3::ZERO, Vec3::UNIT_Z, Vec2::ZERO, Vec4::ONE);
        let m = MeshData::new(ResourceId::from_raw(7), "x", vec![v], vec![0]);
        assert_eq!(m.id().raw(), 7);
        assert_eq!(m.name(), "x");
        assert_eq!(m.vertices().len(), 1);
        assert_eq!(m.indices(), &[0]);
    }

    #[test]
    fn equality_requires_same_content() {
        let v = Vertex::new(Vec3::ZERO, Vec3::UNIT_Z, Vec2::ZERO, Vec4::ONE);
        let a = MeshData::new(ResourceId::from_raw(1), "a", vec![v], vec![0]);
        let b = MeshData::new(ResourceId::from_raw(1), "a", vec![v], vec![0]);
        let c = MeshData::new(ResourceId::from_raw(1), "a", vec![v], vec![1]);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
