//! CPU linear-blend skinning — the software peer of the GPU skinning pass
//! (`axiom_gpu_backend`'s `vs_skinned`). The GPU deforms skinned athlete meshes on
//! the vertex stage from a per-draw joint-matrix palette; this backend has no
//! shader, so it poses the same meshes on the CPU into ordinary
//! [`MeshGeometry`] the flat-shading rasterizer already draws.
//!
//! A skinned mesh is uploaded once (bind pose) in the 20-float vertex stream the
//! GPU takes — `position(3) · normal(3) · uv(2) · colour(4) · joints(4) ·
//! weights(4)` — and each frame a draw supplies its palette. For every vertex the
//! blended world position is `Σ weightᵢ · (palette[jointᵢ] · [pos, 1])`, mirroring
//! `vs_skinned` exactly: the palette bakes each part's `current_world · bind⁻¹`, so
//! the posed positions are in world space and the draw's `mvp`/`world` then apply
//! unchanged (the rasterizer projects by `mvp` and lights by `world`, the same
//! convention the GPU uses). Normals/uvs are dropped — the rasterizer flat-shades
//! per triangle and [`MeshGeometry`] carries only positions + colours.

use std::collections::HashMap;

use crate::mesh_cache::MeshGeometry;

/// Floats per **skinned** vertex: the 12 standard floats + joints(4) + weights(4)
/// — the GPU backend's `SKINNED_VERTEX_STRIDE`.
const SKINNED_VERTEX_STRIDE: usize = 20;

/// A column-major identity matrix, the safe fallback for an out-of-range joint
/// index (a malformed palette reference poses the vertex at its bind position
/// rather than reading past the palette).
const IDENTITY_MAT4: [f32; 16] = [
    1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
];

/// The bind-pose skinned meshes uploaded once at bind (the 20-float streams),
/// keyed by mesh id — the skinned peer of [`crate::mesh_cache::MeshCache`]. Each
/// frame [`Self::pose`] deforms one by a draw's palette into drawable geometry.
#[derive(Debug, Default)]
pub(crate) struct SkinnedMeshCache {
    meshes: HashMap<u64, (Vec<f32>, Vec<u32>)>,
}

impl SkinnedMeshCache {
    /// Upload the skinned mesh set in the GPU backend's `(mesh_id, 20-float
    /// interleaved vertices, indices)` form, so windowing hands both backends the
    /// identical skinning geometry.
    pub(crate) fn load(meshes: &[(u64, Vec<f32>, Vec<u32>)]) -> Self {
        SkinnedMeshCache {
            meshes: meshes
                .iter()
                .map(|(id, verts, indices)| (*id, (verts.clone(), indices.clone())))
                .collect(),
        }
    }

    /// Pose mesh `mesh_id` by `palette` into drawable [`MeshGeometry`] (world-space
    /// positions + pass-through colours + the mesh's indices). `None` when the id
    /// is not in the cache — the caller counts it as a skipped draw, exactly as a
    /// cache miss on the ordinary mesh cache does.
    pub(crate) fn pose(&self, mesh_id: u64, palette: &[[f32; 16]]) -> Option<MeshGeometry> {
        self.meshes
            .get(&mesh_id)
            .map(|(verts, indices)| pose_vertices(verts, indices, palette))
    }
}

/// Linear-blend-skin one 20-float vertex stream by `palette` into [`MeshGeometry`].
/// Pure; the sole skinning math, unit-tested against known palettes.
fn pose_vertices(verts: &[f32], indices: &[u32], palette: &[[f32; 16]]) -> MeshGeometry {
    let (positions, colors): (Vec<[f32; 3]>, Vec<[f32; 4]>) = verts
        .chunks_exact(SKINNED_VERTEX_STRIDE)
        .map(|v| {
            let pos = [v[0], v[1], v[2]];
            let color = [v[8], v[9], v[10], v[11]];
            let joints = [v[12], v[13], v[14], v[15]];
            let weights = [v[16], v[17], v[18], v[19]];
            // Blend the four bone-posed positions by their weights (LBS).
            let posed = (0..4).fold([0.0_f32; 3], |acc, k| {
                let m = palette
                    .get(joints[k] as usize)
                    .copied()
                    .unwrap_or(IDENTITY_MAT4);
                let t = transform_point(&m, pos);
                let w = weights[k];
                [acc[0] + w * t[0], acc[1] + w * t[1], acc[2] + w * t[2]]
            });
            (posed, color)
        })
        .unzip();
    MeshGeometry::from_posed(positions, colors, indices.to_vec())
}

/// Transform a point by a column-major 4×4 matrix (the `clip_coords`/`vs_skinned`
/// convention), returning the affine `xyz` (the row-3 `w` is 1 for a palette of
/// affine bone matrices, so no perspective divide is needed).
fn transform_point(m: &[f32; 16], p: [f32; 3]) -> [f32; 3] {
    [
        m[0] * p[0] + m[4] * p[1] + m[8] * p[2] + m[12],
        m[1] * p[0] + m[5] * p[1] + m[9] * p[2] + m[13],
        m[2] * p[0] + m[6] * p[1] + m[10] * p[2] + m[14],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One vertex at `pos`, coloured `col`, fully weighted to bone 0.
    fn one_vertex(pos: [f32; 3], col: [f32; 4]) -> Vec<f32> {
        vec![
            pos[0], pos[1], pos[2], // position
            0.0, 1.0, 0.0, // normal (unused)
            0.0, 0.0, // uv (unused)
            col[0], col[1], col[2], col[3], // colour
            0.0, 0.0, 0.0, 0.0, // joints (all bone 0)
            1.0, 0.0, 0.0, 0.0, // weights (all on bone 0)
        ]
    }

    fn translation(x: f32, y: f32, z: f32) -> [f32; 16] {
        [
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, x, y, z, 1.0,
        ]
    }

    #[test]
    fn identity_palette_returns_bind_positions_and_colours() {
        let verts = one_vertex([2.0, 3.0, 4.0], [0.1, 0.2, 0.3, 1.0]);
        let cache = SkinnedMeshCache::load(&[(7, verts, vec![0])]);
        let geo = cache.pose(7, &[IDENTITY_MAT4]).expect("mesh 7 is cached");
        assert_eq!(geo.position(0), [2.0, 3.0, 4.0]);
        assert_eq!(geo.color(0), [0.1, 0.2, 0.3, 1.0]);
        assert_eq!(geo.indices(), &[0]);
    }

    #[test]
    fn single_bone_translation_moves_the_vertex() {
        let verts = one_vertex([1.0, 0.0, 0.0], [1.0, 1.0, 1.0, 1.0]);
        let cache = SkinnedMeshCache::load(&[(0, verts, vec![0])]);
        let geo = cache.pose(0, &[translation(10.0, -5.0, 2.0)]).expect("cached");
        assert_eq!(geo.position(0), [11.0, -5.0, 2.0]);
    }

    #[test]
    fn two_bones_blend_by_weight() {
        // Vertex split 0.5/0.5 between bone 0 (translate +10 x) and bone 1 (+0).
        let mut v = one_vertex([0.0, 0.0, 0.0], [1.0, 1.0, 1.0, 1.0]);
        v[12] = 0.0; // joint 0 -> bone 0
        v[13] = 1.0; // joint 1 -> bone 1
        v[16] = 0.5; // weight 0
        v[17] = 0.5; // weight 1
        let cache = SkinnedMeshCache::load(&[(1, v, vec![0])]);
        let geo = cache
            .pose(1, &[translation(10.0, 0.0, 0.0), IDENTITY_MAT4])
            .expect("cached");
        // 0.5*(0+10) + 0.5*(0) = 5.0 on x.
        assert_eq!(geo.position(0), [5.0, 0.0, 0.0]);
    }

    #[test]
    fn out_of_range_joint_falls_back_to_identity() {
        // Joint index 9 with no such palette entry -> identity (bind position).
        let mut v = one_vertex([4.0, 4.0, 4.0], [1.0, 1.0, 1.0, 1.0]);
        v[12] = 9.0;
        let cache = SkinnedMeshCache::load(&[(2, v, vec![0])]);
        let geo = cache.pose(2, &[translation(100.0, 0.0, 0.0)]).expect("cached");
        assert_eq!(geo.position(0), [4.0, 4.0, 4.0]);
    }

    #[test]
    fn unknown_mesh_id_poses_to_none() {
        let cache = SkinnedMeshCache::load(&[(5, one_vertex([0.0; 3], [1.0; 4]), vec![0])]);
        assert!(cache.pose(999, &[IDENTITY_MAT4]).is_none());
    }

    #[test]
    fn default_cache_is_empty() {
        let cache = SkinnedMeshCache::default();
        assert!(cache.pose(0, &[IDENTITY_MAT4]).is_none());
    }
}
