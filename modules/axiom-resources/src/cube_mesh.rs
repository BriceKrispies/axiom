//! The built-in deterministic unit cube mesh, as neutral geometry.
//!
//! This is a *generator*: it produces plain `(position, normal, uv, color)`
//! vertex data plus a triangle index list — the same neutral form any app would
//! hand to [`crate::resources_api::ResourcesApi::register_mesh`]. The cube is
//! therefore a primitive *layered on* the general mesh path, not a special case
//! baked into the resource table.

use crate::mesh_data::MeshInputVertex;

/// Build the deterministic unit cube centred at the origin with per-face
/// normals.
///
/// The cube has 24 vertices (4 per face × 6 faces) so each face has its own
/// outward normal, and 36 indices (2 triangles × 6 faces). Vertex colour is
/// white; UVs span `[0, 1]` per face.
pub(crate) fn build_cube_mesh() -> (Vec<MeshInputVertex>, Vec<u32>) {
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

    // Each face: (outward normal, 4 corner indices into `p`, in CCW order when
    // viewed from outside).
    let faces: [([f32; 3], [usize; 4]); 6] = [
        ([0.0, 0.0, -1.0], [1, 0, 3, 2]), // back  (-Z)
        ([0.0, 0.0, 1.0], [4, 5, 6, 7]),  // front (+Z)
        ([-1.0, 0.0, 0.0], [0, 4, 7, 3]), // left  (-X)
        ([1.0, 0.0, 0.0], [5, 1, 2, 6]),  // right (+X)
        ([0.0, -1.0, 0.0], [0, 1, 5, 4]), // bottom (-Y)
        ([0.0, 1.0, 0.0], [3, 7, 6, 2]),  // top   (+Y)
    ];
    let uvs = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

    let mut vertices: Vec<MeshInputVertex> = Vec::with_capacity(24);
    let mut indices = Vec::with_capacity(36);

    faces.into_iter().for_each(|(normal, corner)| {
        let base = vertices.len() as u32;
        (0..4).for_each(|i| {
            vertices.push((p[corner[i]], normal, uvs[i], WHITE));
        });
        // Two triangles: (0, 1, 2) and (0, 2, 3).
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    });

    (vertices, indices)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cube_has_24_vertices_and_36_indices() {
        let (vertices, indices) = build_cube_mesh();
        assert_eq!(vertices.len(), 24);
        assert_eq!(indices.len(), 36);
    }

    #[test]
    fn cube_is_deterministic_across_runs() {
        assert_eq!(build_cube_mesh(), build_cube_mesh());
    }

    #[test]
    fn each_face_has_unique_outward_normal() {
        let (v, _) = build_cube_mesh();
        // Each face contributes 4 consecutive vertices; the normal is field `.1`.
        assert_eq!(v[0].1, [0.0, 0.0, -1.0]); // face 0 (-Z)
        assert_eq!(v[4].1, [0.0, 0.0, 1.0]); // face 1 (+Z)
        assert_eq!(v[8].1, [-1.0, 0.0, 0.0]); // face 2 (-X)
        assert_eq!(v[12].1, [1.0, 0.0, 0.0]); // face 3 (+X)
        assert_eq!(v[16].1, [0.0, -1.0, 0.0]); // face 4 (-Y)
        assert_eq!(v[20].1, [0.0, 1.0, 0.0]); // face 5 (+Y)
    }

    #[test]
    fn cube_corner_positions_are_exact() {
        // Kills the digit/`-` deletions in the `p` corner table: every corner
        // coordinate is exactly +/-0.5. The position is field `.0`. Face 0 (-Z)
        // has corners [1, 0, 3, 2] into `p`; face 1 (+Z) has [4, 5, 6, 7].
        let (v, _) = build_cube_mesh();
        // Face 0 (-Z): p[1], p[0], p[3], p[2].
        assert_eq!(v[0].0, [0.5, -0.5, -0.5]); // p[1]
        assert_eq!(v[1].0, [-0.5, -0.5, -0.5]); // p[0]
        assert_eq!(v[2].0, [-0.5, 0.5, -0.5]); // p[3]
        assert_eq!(v[3].0, [0.5, 0.5, -0.5]); // p[2]
                                              // Face 1 (+Z): p[4], p[5], p[6], p[7].
        assert_eq!(v[4].0, [-0.5, -0.5, 0.5]); // p[4]
        assert_eq!(v[5].0, [0.5, -0.5, 0.5]); // p[5]
        assert_eq!(v[6].0, [0.5, 0.5, 0.5]); // p[6]
        assert_eq!(v[7].0, [-0.5, 0.5, 0.5]); // p[7]
    }

    #[test]
    fn cube_uvs_and_color_are_per_face_neutral() {
        // The 4 corners of every face carry the same UV set and white colour.
        let (v, _) = build_cube_mesh();
        assert_eq!(v[0].2, [0.0, 0.0]);
        assert_eq!(v[1].2, [1.0, 0.0]);
        assert_eq!(v[2].2, [1.0, 1.0]);
        assert_eq!(v[3].2, [0.0, 1.0]);
        assert!(v.iter().all(|vert| vert.3 == [1.0, 1.0, 1.0, 1.0]));
    }

    #[test]
    fn cube_indices_match_per_face_winding() {
        // Kills `base + 1 -> base * 1`: on faces after the first, `base` is
        // non-zero, so the two triangles must read base, base+1, base+2, base,
        // base+2, base+3.
        let (_, idx) = build_cube_mesh();
        // Face 1 starts at vertex base = 4 (6 indices per face).
        assert_eq!(&idx[6..12], &[4, 5, 6, 4, 6, 7]);
        // Face 5 starts at base = 20.
        assert_eq!(&idx[30..36], &[20, 21, 22, 20, 22, 23]);
    }

    #[test]
    fn cube_indices_are_valid() {
        let (vertices, indices) = build_cube_mesh();
        for &i in &indices {
            assert!((i as usize) < vertices.len());
        }
    }
}
