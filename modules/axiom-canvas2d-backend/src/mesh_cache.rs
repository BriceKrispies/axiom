//! CPU mesh geometry cache: the vertex positions, vertex colours, and triangle
//! indices the rasterizer projects each frame.
//!
//! Meshes are uploaded in the same `(mesh_id, 12-float interleaved vertices,
//! indices)` form the GPU backend takes, so windowing hands both backends the
//! identical geometry. Of each 12-float vertex (position, normal, uv, colour)
//! the rasterizer keeps the position (floats 0..3), the **uv** (floats 6..8) and
//! the colour (floats 8..12).
//!
//! **The positions are object-space**, exactly as uploaded: the draw's `mvp`
//! transforms them at projection time and the draw's `world` rotates the face
//! normal. That is what lets a CPU-evaluated surface be sampled in the object's
//! own frame — the space `axiom_surface` declares its channel expressions to be
//! evaluated in — with no matrix inversion anywhere, per frame or otherwise.
//!
//! The uv is kept for the same reason and only that reason: it is the second of
//! the two evaluation-context inputs a surface reads that geometry can supply.
//! It costs one `[f32; 2]` per vertex, once, at upload.

use std::collections::HashMap;

/// Floats per interleaved vertex: position(3) + normal(3) + uv(2) + colour(4).
const VERTEX_STRIDE: usize = 12;

/// One uploaded mesh's CPU geometry: parallel per-vertex position and linear
/// RGBA colour arrays, plus the triangle indices.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct MeshGeometry {
    positions: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

impl MeshGeometry {
    /// Split a 12-float interleaved vertex stream into parallel
    /// position/uv/colour arrays (positions = floats 0..3, uvs = floats 6..8,
    /// colours = floats 8..12).
    fn from_interleaved(verts: &[f32], indices: &[u32]) -> Self {
        let (positions, rest): (Vec<[f32; 3]>, Vec<([f32; 2], [f32; 4])>) = verts
            .chunks_exact(VERTEX_STRIDE)
            .map(|v| {
                (
                    [v[0], v[1], v[2]],
                    ([v[6], v[7]], [v[8], v[9], v[10], v[11]]),
                )
            })
            .unzip();
        let (uvs, colors): (Vec<[f32; 2]>, Vec<[f32; 4]>) = rest.into_iter().unzip();
        MeshGeometry {
            positions,
            uvs,
            colors,
            indices: indices.to_vec(),
        }
    }

    /// Build geometry directly from parallel posed position/colour arrays plus
    /// indices — the output shape of CPU skinning (see [`crate::mesh_skinning`]),
    /// which produces **already-posed** positions per frame rather than an
    /// interleaved upload. Colours ride through from the skinned mesh; normals
    /// and uvs are not carried, so a skinned draw's uv reads as the origin and a
    /// surface evaluated on one samples at its posed position rather than at a
    /// rest-pose object position. Skinned bodies are outside this manifest's
    /// surface path for exactly that reason, and the zero uv makes that visible
    /// rather than silently wrong.
    pub(crate) fn from_posed(
        positions: Vec<[f32; 3]>,
        colors: Vec<[f32; 4]>,
        indices: Vec<u32>,
    ) -> Self {
        MeshGeometry {
            uvs: vec![[0.0; 2]; positions.len()],
            positions,
            colors,
            indices,
        }
    }

    /// The triangle indices (triples index the position/colour arrays).
    pub(crate) fn indices(&self) -> &[u32] {
        &self.indices
    }

    /// The position of vertex `idx`, or the origin when `idx` is out of range
    /// (a malformed index never panics — it degrades to a point at the origin).
    pub(crate) fn position(&self, idx: u32) -> [f32; 3] {
        self.positions
            .get(idx as usize)
            .copied()
            .unwrap_or([0.0; 3])
    }

    /// The uv of vertex `idx`, or the origin when `idx` is out of range or the
    /// geometry carries no uvs (a CPU-skinned pose).
    pub(crate) fn uv(&self, idx: u32) -> [f32; 2] {
        self.uvs.get(idx as usize).copied().unwrap_or([0.0; 2])
    }

    /// The linear RGBA colour of vertex `idx`, or opaque white when out of range.
    pub(crate) fn color(&self, idx: u32) -> [f32; 4] {
        self.colors.get(idx as usize).copied().unwrap_or([1.0; 4])
    }
}

/// A cache of uploaded mesh geometry, keyed by mesh id.
#[derive(Debug, Default)]
pub(crate) struct MeshCache {
    meshes: HashMap<u64, MeshGeometry>,
}

impl MeshCache {
    /// Build a cache from meshes in the GPU backend's `(mesh_id, 12-float
    /// interleaved vertices, indices)` upload form.
    pub(crate) fn load(meshes: &[(u64, Vec<f32>, Vec<u32>)]) -> Self {
        let meshes = meshes
            .iter()
            .map(|(id, verts, indices)| (*id, MeshGeometry::from_interleaved(verts, indices)))
            .collect();
        MeshCache { meshes }
    }

    /// The geometry uploaded under `mesh_id`, or `None` if none was.
    pub(crate) fn get(&self, mesh_id: u64) -> Option<&MeshGeometry> {
        self.meshes.get(&mesh_id)
    }

    /// Replace one mesh's geometry mid-loop (the streaming-terrain path), in the
    /// same 12-float interleaved form as [`Self::load`].
    pub(crate) fn replace(&mut self, mesh_id: u64, vertices: &[f32], indices: &[u32]) {
        self.meshes
            .insert(mesh_id, MeshGeometry::from_interleaved(vertices, indices));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One vertex's 12 interleaved floats: position, normal, uv, colour.
    fn vertex(pos: [f32; 3], color: [f32; 4]) -> [f32; 12] {
        [
            pos[0], pos[1], pos[2], // position
            0.0, 1.0, 0.0, // normal (ignored)
            pos[0], pos[2], // uv
            color[0], color[1], color[2], color[3], // colour
        ]
    }

    fn tri_mesh() -> MeshGeometry {
        let mut verts = Vec::new();
        verts.extend_from_slice(&vertex([0.0, 0.0, 0.0], [1.0, 0.0, 0.0, 1.0]));
        verts.extend_from_slice(&vertex([1.0, 0.0, 0.0], [0.0, 1.0, 0.0, 1.0]));
        verts.extend_from_slice(&vertex([0.0, 1.0, 0.0], [0.0, 0.0, 1.0, 1.0]));
        MeshGeometry::from_interleaved(&verts, &[0, 1, 2])
    }

    #[test]
    fn from_interleaved_splits_position_and_colour() {
        let m = tri_mesh();
        assert_eq!(m.indices(), &[0, 1, 2]);
        assert_eq!(m.position(0), [0.0, 0.0, 0.0]);
        assert_eq!(m.position(1), [1.0, 0.0, 0.0]);
        assert_eq!(m.position(2), [0.0, 1.0, 0.0]);
        assert_eq!(m.color(0), [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(m.color(2), [0.0, 0.0, 1.0, 1.0]);
        // Floats 6..8 are the uv, kept so a CPU-evaluated surface has the second
        // of the two geometry-supplied evaluation-context inputs.
        assert_eq!(m.uv(0), [0.0, 0.0]);
        assert_eq!(m.uv(1), [1.0, 0.0]);
        assert_eq!(m.uv(2), [0.0, 0.0]);
    }

    #[test]
    fn out_of_range_index_degrades_to_origin_and_white() {
        let m = tri_mesh();
        assert_eq!(m.position(99), [0.0, 0.0, 0.0]);
        assert_eq!(m.color(99), [1.0, 1.0, 1.0, 1.0]);
        assert_eq!(m.uv(99), [0.0, 0.0]);
    }

    #[test]
    fn a_posed_mesh_carries_one_origin_uv_per_vertex() {
        let posed = MeshGeometry::from_posed(
            vec![[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]],
            vec![[1.0; 4], [1.0; 4]],
            vec![0, 1, 0],
        );
        assert_eq!(posed.uv(0), [0.0, 0.0]);
        assert_eq!(posed.uv(1), [0.0, 0.0]);
        assert_eq!(posed.position(1), [4.0, 5.0, 6.0]);
    }

    #[test]
    fn default_geometry_is_empty() {
        let m = MeshGeometry::default();
        assert!(m.indices().is_empty());
        assert_eq!(m, MeshGeometry::default());
        assert!(format!("{m:?}").contains("MeshGeometry"));
    }

    fn quad_verts() -> Vec<f32> {
        let mut verts = Vec::new();
        verts.extend_from_slice(&vertex([0.0, 0.0, 0.0], [1.0, 1.0, 1.0, 1.0]));
        verts.extend_from_slice(&vertex([1.0, 0.0, 0.0], [1.0, 1.0, 1.0, 1.0]));
        verts.extend_from_slice(&vertex([0.0, 1.0, 0.0], [1.0, 1.0, 1.0, 1.0]));
        verts
    }

    #[test]
    fn cache_load_and_get() {
        let cache = MeshCache::load(&[(7, quad_verts(), vec![0, 1, 2])]);
        assert!(cache.get(7).is_some());
        assert!(cache.get(99).is_none());
        assert_eq!(cache.get(7).map(|g| g.indices().len()), Some(3));
        assert!(format!("{cache:?}").contains("MeshCache"));
    }

    #[test]
    fn replace_swaps_one_mesh_geometry() {
        let mut cache = MeshCache::load(&[(7, quad_verts(), vec![0, 1, 2])]);
        let mut new_verts = Vec::new();
        new_verts.extend_from_slice(&vertex([5.0, 5.0, 5.0], [0.0, 0.0, 0.0, 1.0]));
        cache.replace(7, &new_verts, &[0]);
        assert_eq!(cache.get(7).map(|g| g.position(0)), Some([5.0, 5.0, 5.0]));
        assert_eq!(cache.get(7).map(|g| g.indices().to_vec()), Some(vec![0]));
        // Replacing an unseen id inserts it.
        cache.replace(8, &new_verts, &[0]);
        assert!(cache.get(8).is_some());
    }
}
