//! The built-in deterministic unit plane (quad) mesh, as neutral geometry.
//!
//! Like [`crate::cube_mesh`], this is a *generator* producing plain
//! `(position, normal, uv, color)` vertices + a triangle index list. The plane is
//! a 1x1 quad in the XZ plane centred at the origin, facing +Y — an app scales it
//! (via the renderable's transform) into a ground plane.

use crate::mesh_data::MeshInputVertex;

/// Build the deterministic unit plane: 4 vertices, 2 triangles, normal +Y, white
/// vertex colour, UVs spanning `[0, 1]`.
pub(crate) fn build_plane_mesh() -> (Vec<MeshInputVertex>, Vec<u32>) {
    const UP: [f32; 3] = [0.0, 1.0, 0.0];
    const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    let vertices = vec![
        ([-0.5, 0.0, -0.5], UP, [0.0, 0.0], WHITE),
        ([0.5, 0.0, -0.5], UP, [1.0, 0.0], WHITE),
        ([0.5, 0.0, 0.5], UP, [1.0, 1.0], WHITE),
        ([-0.5, 0.0, 0.5], UP, [0.0, 1.0], WHITE),
    ];
    let indices = vec![0, 1, 2, 0, 2, 3];
    (vertices, indices)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plane_has_4_vertices_and_6_indices() {
        let (vertices, indices) = build_plane_mesh();
        assert_eq!(vertices.len(), 4);
        assert_eq!(indices.len(), 6);
    }

    #[test]
    fn plane_is_deterministic_and_faces_up() {
        assert_eq!(build_plane_mesh(), build_plane_mesh());
        let (vertices, _) = build_plane_mesh();
        assert!(vertices.iter().all(|v| v.1 == [0.0, 1.0, 0.0]));
        assert!(vertices.iter().all(|v| v.0[1] == 0.0));
    }

    #[test]
    fn plane_indices_are_valid() {
        let (vertices, indices) = build_plane_mesh();
        assert!(indices.iter().all(|&i| (i as usize) < vertices.len()));
    }
}
