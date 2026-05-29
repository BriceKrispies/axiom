//! The built-in deterministic unit cube mesh.

use axiom_math::{Vec2, Vec3, Vec4};

use crate::mesh_data::MeshData;
use crate::resource_id::ResourceId;
use crate::vertex::Vertex;

/// Build a deterministic unit cube mesh centred at the origin with
/// per-face normals.
///
/// The cube has 24 vertices (4 per face × 6 faces) so each face has
/// its own outward normal, and 36 indices (2 triangles × 6 faces).
/// Vertex colour is white; UVs span `[0, 1]` per face.
pub fn build_cube_mesh(id: ResourceId) -> MeshData {
    let mut vertices = Vec::with_capacity(24);
    let mut indices = Vec::with_capacity(36);

    let p = [
        [-0.5, -0.5, -0.5],
        [0.5, -0.5, -0.5],
        [0.5, 0.5, -0.5],
        [-0.5, 0.5, -0.5],
        [-0.5, -0.5, 0.5],
        [0.5, -0.5, 0.5],
        [0.5, 0.5, 0.5],
        [-0.5, 0.5, 0.5],
    ];

    // Each face: (normal, 4 corner indices into `p`, in CCW order when
    // viewed from outside).
    let faces: [(Vec3, [usize; 4]); 6] = [
        (Vec3::new(0.0, 0.0, -1.0), [1, 0, 3, 2]), // back  (-Z)
        (Vec3::new(0.0, 0.0, 1.0), [4, 5, 6, 7]),  // front (+Z)
        (Vec3::new(-1.0, 0.0, 0.0), [0, 4, 7, 3]), // left  (-X)
        (Vec3::new(1.0, 0.0, 0.0), [5, 1, 2, 6]),  // right (+X)
        (Vec3::new(0.0, -1.0, 0.0), [0, 1, 5, 4]), // bottom (-Y)
        (Vec3::new(0.0, 1.0, 0.0), [3, 7, 6, 2]),  // top   (+Y)
    ];
    let uvs = [
        Vec2::new(0.0, 0.0),
        Vec2::new(1.0, 0.0),
        Vec2::new(1.0, 1.0),
        Vec2::new(0.0, 1.0),
    ];

    for (normal, corner) in faces {
        let base = vertices.len() as u32;
        for i in 0..4 {
            let pos = p[corner[i]];
            vertices.push(Vertex::new(
                Vec3::new(pos[0], pos[1], pos[2]),
                normal,
                uvs[i],
                Vec4::ONE,
            ));
        }
        // Two triangles: (0, 1, 2) and (0, 2, 3).
        indices.push(base);
        indices.push(base + 1);
        indices.push(base + 2);
        indices.push(base);
        indices.push(base + 2);
        indices.push(base + 3);
    }

    MeshData::new(id, "axiom.builtin.cube", vertices, indices)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cube_has_24_vertices_and_36_indices() {
        let mesh = build_cube_mesh(ResourceId::from_raw(1));
        assert_eq!(mesh.vertices().len(), 24);
        assert_eq!(mesh.indices().len(), 36);
    }

    #[test]
    fn cube_is_deterministic_across_runs() {
        let a = build_cube_mesh(ResourceId::from_raw(1));
        let b = build_cube_mesh(ResourceId::from_raw(1));
        assert_eq!(a, b);
    }

    #[test]
    fn each_face_has_unique_outward_normal() {
        let mesh = build_cube_mesh(ResourceId::from_raw(1));
        // Face 0 normal:
        assert_eq!(mesh.vertices()[0].normal(), Vec3::new(0.0, 0.0, -1.0));
        // Face 1 normal (vertices 4..8):
        assert_eq!(mesh.vertices()[4].normal(), Vec3::new(0.0, 0.0, 1.0));
        // Face 2 normal (vertices 8..12):
        assert_eq!(mesh.vertices()[8].normal(), Vec3::new(-1.0, 0.0, 0.0));
        // Face 3 normal (vertices 12..16):
        assert_eq!(mesh.vertices()[12].normal(), Vec3::new(1.0, 0.0, 0.0));
        // Face 4 normal (vertices 16..20):
        assert_eq!(mesh.vertices()[16].normal(), Vec3::new(0.0, -1.0, 0.0));
        // Face 5 normal (vertices 20..24):
        assert_eq!(mesh.vertices()[20].normal(), Vec3::new(0.0, 1.0, 0.0));
    }

    #[test]
    fn cube_indices_are_valid() {
        let mesh = build_cube_mesh(ResourceId::from_raw(1));
        for &i in mesh.indices() {
            assert!((i as usize) < mesh.vertices().len());
        }
    }
}
