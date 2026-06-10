//! The demo's cube mesh, as app-owned geometry.
//!
//! Apps are leaves in the dependency graph and the engine's resource module
//! is shape-agnostic, so the demo owns the geometry it wants to draw. This
//! builds the unit cube the headless slice renders — 24 vertices (one set of
//! per-face normals), 36 indices, white vertex colour — in the
//! [`axiom_resources::ResourcesApi::register_mesh`] input format.

/// One cube vertex in `register_mesh` input form: `(position, normal, uv,
/// color)`, each a plain float array.
type CubeVertex = ([f32; 3], [f32; 3], [f32; 2], [f32; 4]);

/// Build the demo's unit cube centred at the origin, with per-face outward
/// normals so each face is flat-shaded.
pub(crate) fn demo_cube() -> (Vec<CubeVertex>, Vec<u32>) {
    // The 8 corners of the unit cube.
    const P: [[f32; 3]; 8] = [
        [-0.5, -0.5, -0.5],
        [0.5, -0.5, -0.5],
        [0.5, 0.5, -0.5],
        [-0.5, 0.5, -0.5],
        [-0.5, -0.5, 0.5],
        [0.5, -0.5, 0.5],
        [0.5, 0.5, 0.5],
        [-0.5, 0.5, 0.5],
    ];
    // Each face: its outward normal and the 4 corner indices into `P`, in
    // CCW order viewed from outside.
    const FACES: [([f32; 3], [usize; 4]); 6] = [
        ([0.0, 0.0, -1.0], [1, 0, 3, 2]), // back  (-Z)
        ([0.0, 0.0, 1.0], [4, 5, 6, 7]),  // front (+Z)
        ([-1.0, 0.0, 0.0], [0, 4, 7, 3]), // left  (-X)
        ([1.0, 0.0, 0.0], [5, 1, 2, 6]),  // right (+X)
        ([0.0, -1.0, 0.0], [0, 1, 5, 4]), // bottom (-Y)
        ([0.0, 1.0, 0.0], [3, 7, 6, 2]),  // top   (+Y)
    ];
    const UVS: [[f32; 2]; 4] = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

    let mut vertices = Vec::with_capacity(24);
    let mut indices = Vec::with_capacity(36);
    for (normal, corner) in FACES {
        let base = vertices.len() as u32;
        for i in 0..4 {
            vertices.push((P[corner[i]], normal, UVS[i], WHITE));
        }
        // Two triangles per face: (0,1,2) and (0,2,3).
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    (vertices, indices)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cube_has_24_vertices_and_36_indices() {
        let (vertices, indices) = demo_cube();
        assert_eq!(vertices.len(), 24);
        assert_eq!(indices.len(), 36);
    }

    #[test]
    fn every_corner_coordinate_is_half_unit() {
        let (vertices, _) = demo_cube();
        for (pos, _, _, _) in vertices {
            for c in pos {
                assert!((c.abs() - 0.5).abs() < 1.0e-6);
            }
        }
    }

    #[test]
    fn every_index_is_in_vertex_range() {
        let (vertices, indices) = demo_cube();
        for i in indices {
            assert!((i as usize) < vertices.len());
        }
    }

    #[test]
    fn is_deterministic_across_calls() {
        assert_eq!(demo_cube(), demo_cube());
    }
}
