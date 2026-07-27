//! The built-in deterministic unit plane (quad) mesh, as neutral geometry.
//!
//! Like [`crate::cube_mesh`], this is a *generator* producing plain
//! `(position, normal, uv, color)` vertices + a triangle index list. The plane is
//! a 1x1 quad in the XZ plane centred at the origin, facing +Y — an app scales it
//! (via the renderable's transform) into a ground plane.

use crate::mesh_data::MeshInputVertex;

/// Build the deterministic unit plane: 4 vertices, 2 triangles, normal +Y, white
/// vertex colour, UVs spanning `[0, 1]`.
///
/// The triangle winding is chosen so the **geometric** face normal
/// (`cross(v1 - v0, v2 - v0)`) points the same way as the authored vertex
/// normal, `+Y`. That agreement is load-bearing, not cosmetic: a flat-shading
/// renderer is entitled to take a triangle's normal from its winding rather
/// than from the authored per-vertex normals, and the software rasterizer in
/// `axiom-canvas2d-backend` does exactly that (`face_normal_world`). With the
/// two disagreeing, every ground plane in every app was lit from *below* on
/// that backend — no key light at all, only the ground half of the hemisphere
/// ambient — which is why flat surfaces rendered nearly black there while the
/// GPU path (which interpolates the authored normals) looked correct.
/// [`plane_winding_agrees_with_its_normals`] pins this down.
pub(crate) fn build_plane_mesh() -> (Vec<MeshInputVertex>, Vec<u32>) {
    const UP: [f32; 3] = [0.0, 1.0, 0.0];
    const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
    let vertices = vec![
        ([-0.5, 0.0, -0.5], UP, [0.0, 0.0], WHITE),
        ([0.5, 0.0, -0.5], UP, [1.0, 0.0], WHITE),
        ([0.5, 0.0, 0.5], UP, [1.0, 1.0], WHITE),
        ([-0.5, 0.0, 0.5], UP, [0.0, 1.0], WHITE),
    ];
    let indices = vec![0, 2, 1, 0, 3, 2];
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

    /// Every triangle's winding must produce the same normal the vertices
    /// declare. A renderer that flat-shades from the winding and one that
    /// interpolates the authored normals must agree about which way the
    /// surface faces, or the same scene is lit from opposite sides on two
    /// backends.
    #[test]
    fn plane_winding_agrees_with_its_normals() {
        let (vertices, indices) = build_plane_mesh();
        let position = |i: u32| vertices[i as usize].0;
        indices.chunks_exact(3).for_each(|tri| {
            let (a, b, c) = (position(tri[0]), position(tri[1]), position(tri[2]));
            let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let e2 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
            // cross(e1, e2), the geometric face normal.
            let face = [
                e1[1] * e2[2] - e1[2] * e2[1],
                e1[2] * e2[0] - e1[0] * e2[2],
                e1[0] * e2[1] - e1[1] * e2[0],
            ];
            let authored = vertices[tri[0] as usize].1;
            let agreement =
                face[0] * authored[0] + face[1] * authored[1] + face[2] * authored[2];
            assert!(
                agreement > 0.0,
                "winding {tri:?} faces {face:?}, opposing the authored normal {authored:?}"
            );
        });
    }

    #[test]
    fn plane_indices_are_valid() {
        let (vertices, indices) = build_plane_mesh();
        assert!(indices.iter().all(|&i| (i as usize) < vertices.len()));
    }
}
