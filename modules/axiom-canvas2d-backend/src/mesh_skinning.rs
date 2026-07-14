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

/// Vertex-clustering resolution for the software-backend decimation: each mesh's
/// bounding box is divided into this many cells **per axis** (an anisotropic
/// K×K×K grid over its own extent), all vertices in a cell weld to one
/// representative, and triangles that collapse to a line/point are dropped.
/// Higher = finer (more triangles kept). The athlete MetaSurface bodies bake to
/// tens of thousands of triangles — trivial for the GPU, but the CPU rasterizer
/// projects + culls every one and most are sub-pixel at the low-poly framebuffer
/// resolution. Clustering thins them to a budget the software path affords while
/// KEEPING coverage (unlike dropping triangles, which tears holes in a watertight
/// surface). Per-axis (not a single diagonal-derived) cell size is essential: a
/// thin feature (a neck, a leg) gets K cells across its *narrow* axis and so
/// survives, where an isotropic cell derived from the tall body diagonal would be
/// wider than the whole limb and collapse it to nothing. The Canvas 2D backend is
/// a low-poly degrade by design, so a chunkier athlete here is the intended trade.
const CLUSTER_CELLS_PER_AXIS: f32 = 16.0;

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
    /// interleaved vertices, indices)` form, **decimated** to the software
    /// backend's per-body triangle budget once at upload — so every later frame
    /// poses + rasterizes only the kept triangles (both costs drop proportionally),
    /// while the GPU keeps the full-resolution mesh.
    pub(crate) fn load(meshes: &[(u64, Vec<f32>, Vec<u32>)]) -> Self {
        SkinnedMeshCache {
            meshes: meshes
                .iter()
                .map(|(id, verts, indices)| (*id, decimate(verts, indices)))
                .collect(),
        }
    }

    /// Pose mesh `mesh_id` by `palette` into drawable [`MeshGeometry`] (world-space
    /// positions + pass-through colours), keeping only the **front-facing**
    /// triangles under `mvp` — the athlete bodies are closed solids, so their back
    /// faces are always occluded by the front; dropping them here roughly halves
    /// the software rasterizer's overdraw (candidate pixels + depth tests) at zero
    /// visual cost. `None` when the id is not cached (a skipped draw, like an
    /// ordinary cache miss).
    pub(crate) fn pose(
        &self,
        mesh_id: u64,
        palette: &[[f32; 16]],
        mvp: &[f32; 16],
    ) -> Option<MeshGeometry> {
        self.meshes
            .get(&mesh_id)
            .map(|(verts, indices)| pose_vertices(verts, indices, palette, mvp))
    }
}

/// Project a world position by a column-major `mvp` into `(ndc_x, ndc_y, w)` — the
/// perspective-divided screen point plus the clip `w` (positive when in front of
/// the camera). A triangle whose three `w` are all positive is fully in front, so
/// its screen winding is a valid front/back test.
fn project_ndc(mvp: &[f32; 16], p: [f32; 3]) -> (f32, f32, f32) {
    let x = mvp[0] * p[0] + mvp[4] * p[1] + mvp[8] * p[2] + mvp[12];
    let y = mvp[1] * p[0] + mvp[5] * p[1] + mvp[9] * p[2] + mvp[13];
    let w = mvp[3] * p[0] + mvp[7] * p[1] + mvp[11] * p[2] + mvp[15];
    let inv = 1.0 / w;
    (x * inv, y * inv, w)
}

/// Whether triangle `a,b,c` (world positions) faces the camera under `mvp`: its
/// projected screen winding is counter-clockwise (positive NDC signed area). A
/// triangle with any vertex at/behind the camera plane (`w <= 0`) is **kept**
/// (its projection is invalid, so back-face culling would be unreliable — better
/// to draw it than to wrongly drop a triangle straddling the near plane).
fn front_facing(a: [f32; 3], b: [f32; 3], c: [f32; 3], mvp: &[f32; 16]) -> bool {
    let (ax, ay, aw) = project_ndc(mvp, a);
    let (bx, by, bw) = project_ndc(mvp, b);
    let (cx, cy, cw) = project_ndc(mvp, c);
    let area = (bx - ax) * (cy - ay) - (by - ay) * (cx - ax);
    let all_front = (aw > 0.0) & (bw > 0.0) & (cw > 0.0);
    // Keep front-facing (area > 0) OR any near/behind-plane triangle.
    (area > 0.0) | !all_front
}

/// Decimate a skinned mesh by **vertex clustering** for the software backend:
/// snap every vertex to a per-axis grid ([`CLUSTER_CELLS_PER_AXIS`] cells across
/// each axis of the mesh's own bounding box), weld all vertices in a cell to the
/// first one seen, remap the triangles, and drop any that collapse to a line/point
/// (two corners in one cell). Unlike dropping whole triangles, welding preserves
/// the body's coverage — the surface stays closed, just chunkier — so it never
/// tears. The per-axis grid preserves thin limbs (a leg gets K cells across its
/// narrow axis) that a single diagonal-derived cell would erase. Clustering on the
/// BIND pose is sound: welded vertices share (or nearly share) joints, so they
/// stay welded once skinned. Runs once per mesh at upload.
fn decimate(verts: &[f32], indices: &[u32]) -> (Vec<f32>, Vec<u32>) {
    // Bounding box of the bind positions (first three floats per vertex).
    let big = f32::INFINITY;
    let (min, max) =
        verts
            .chunks_exact(SKINNED_VERTEX_STRIDE)
            .fold(([big; 3], [-big; 3]), |(mn, mx), v| {
                (
                    [mn[0].min(v[0]), mn[1].min(v[1]), mn[2].min(v[2])],
                    [mx[0].max(v[0]), mx[1].max(v[1]), mx[2].max(v[2])],
                )
            });
    // Per-axis cell size: each axis's extent split into `CLUSTER_CELLS_PER_AXIS`
    // cells (a K×K×K grid over the mesh's own box), with a tiny floor so a flat/
    // degenerate axis never divides by zero. A narrow axis therefore gets a narrow
    // cell — the key to preserving thin limbs the diagonal cell used to erase.
    let cell = [
        ((max[0] - min[0]) / CLUSTER_CELLS_PER_AXIS).max(1e-4),
        ((max[1] - min[1]) / CLUSTER_CELLS_PER_AXIS).max(1e-4),
        ((max[2] - min[2]) / CLUSTER_CELLS_PER_AXIS).max(1e-4),
    ];
    let key = |v: &[f32]| {
        (
            ((v[0] - min[0]) / cell[0]) as i32,
            ((v[1] - min[1]) / cell[1]) as i32,
            ((v[2] - min[2]) / cell[2]) as i32,
        )
    };
    // Each original vertex → its cell representative's new index (first vertex in
    // the cell becomes the representative and is copied into `out_verts`).
    let mut reps: HashMap<(i32, i32, i32), u32> = HashMap::new();
    let mut out_verts: Vec<f32> = Vec::new();
    let vert_to_rep: Vec<u32> = verts
        .chunks_exact(SKINNED_VERTEX_STRIDE)
        .map(|v| {
            *reps.entry(key(v)).or_insert_with(|| {
                let idx = (out_verts.len() / SKINNED_VERTEX_STRIDE) as u32;
                out_verts.extend_from_slice(v);
                idx
            })
        })
        .collect();
    // Remap each triangle to representatives; keep only non-degenerate ones.
    let out_indices: Vec<u32> = indices
        .chunks_exact(3)
        .filter_map(|tri| {
            let a = vert_to_rep[tri[0] as usize];
            let b = vert_to_rep[tri[1] as usize];
            let c = vert_to_rep[tri[2] as usize];
            ((a != b) & (b != c) & (a != c)).then_some([a, b, c])
        })
        .flatten()
        .collect();
    (out_verts, out_indices)
}

/// Linear-blend-skin one 20-float vertex stream by `palette` into [`MeshGeometry`],
/// then drop the back-facing triangles under `mvp` (see [`front_facing`]). Pure;
/// the skinning math is unit-tested against known palettes.
fn pose_vertices(
    verts: &[f32],
    indices: &[u32],
    palette: &[[f32; 16]],
    mvp: &[f32; 16],
) -> MeshGeometry {
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
    // Keep only front-facing triangles (the closed body's back faces are always
    // occluded), halving the software rasterizer's overdraw at no visual cost.
    let front: Vec<u32> = indices
        .chunks_exact(3)
        .filter(|tri| {
            front_facing(
                positions[tri[0] as usize],
                positions[tri[1] as usize],
                positions[tri[2] as usize],
                mvp,
            )
        })
        .flatten()
        .copied()
        .collect();
    MeshGeometry::from_posed(positions, colors, front)
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

    /// A triangle from three corners (each vertex bone-0 weighted). Corners far
    /// enough apart survive vertex clustering; coincident corners collapse.
    fn triangle(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> (Vec<f32>, Vec<u32>) {
        let mut v = Vec::new();
        [a, b, c]
            .iter()
            .for_each(|p| v.extend_from_slice(&one_vertex(*p, [1.0; 4])));
        (v, vec![0, 1, 2])
    }

    // --- the skinning math (pose_vertices, bypassing decimation) --------------

    // A front-facing (CCW) triangle in the XY plane under an identity mvp — its
    // three bind positions posed by bone 0, so `pose_vertices` keeps all three.
    fn ccw_triangle() -> (Vec<f32>, Vec<u32>) {
        triangle([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0])
    }

    #[test]
    fn pose_identity_palette_returns_bind_positions_and_colours() {
        let (v, i) = ccw_triangle();
        let geo = pose_vertices(&v, &i, &[IDENTITY_MAT4], &IDENTITY_MAT4);
        assert_eq!(geo.position(0), [0.0, 0.0, 0.0]);
        assert_eq!(geo.color(0), [1.0, 1.0, 1.0, 1.0]);
        assert_eq!(
            geo.indices(),
            &[0, 1, 2],
            "the CCW triangle is front-facing"
        );
    }

    #[test]
    fn pose_single_bone_translation_moves_the_vertex() {
        let v = one_vertex([1.0, 0.0, 0.0], [1.0; 4]);
        let geo = pose_vertices(&v, &[0], &[translation(10.0, -5.0, 2.0)], &IDENTITY_MAT4);
        assert_eq!(geo.position(0), [11.0, -5.0, 2.0]);
    }

    #[test]
    fn pose_two_bones_blend_by_weight() {
        // Vertex split 0.5/0.5 between bone 0 (translate +10 x) and bone 1 (+0).
        let mut v = one_vertex([0.0, 0.0, 0.0], [1.0; 4]);
        v[13] = 1.0; // joint 1 -> bone 1
        v[16] = 0.5; // weight 0
        v[17] = 0.5; // weight 1
        let geo = pose_vertices(
            &v,
            &[0],
            &[translation(10.0, 0.0, 0.0), IDENTITY_MAT4],
            &IDENTITY_MAT4,
        );
        assert_eq!(geo.position(0), [5.0, 0.0, 0.0]);
    }

    #[test]
    fn pose_out_of_range_joint_falls_back_to_identity() {
        // Joint index 9 with no such palette entry -> identity (bind position).
        let mut v = one_vertex([4.0, 4.0, 4.0], [1.0; 4]);
        v[12] = 9.0;
        let geo = pose_vertices(&v, &[0], &[translation(100.0, 0.0, 0.0)], &IDENTITY_MAT4);
        assert_eq!(geo.position(0), [4.0, 4.0, 4.0]);
    }

    // --- back-face culling (front_facing / project_ndc) -----------------------

    #[test]
    fn pose_culls_a_back_facing_triangle() {
        // Same corners as the CCW triangle but wound CW -> back-facing -> dropped.
        let (v, _) = ccw_triangle();
        let geo = pose_vertices(&v, &[0, 2, 1], &[IDENTITY_MAT4], &IDENTITY_MAT4);
        assert!(geo.indices().is_empty(), "the CW (back) triangle is culled");
    }

    #[test]
    fn pose_keeps_a_near_plane_triangle() {
        // An mvp whose clip-w = z (row 3 = [0,0,1,0]); a vertex at z <= 0 has w <= 0,
        // so its projection is invalid and the triangle is kept rather than risk a
        // wrong cull across the near plane.
        let near_mvp = [
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0,
        ];
        let (v, i) = triangle([0.0, 0.0, -1.0], [1.0, 0.0, 1.0], [0.0, 1.0, 1.0]);
        let geo = pose_vertices(&v, &i, &[IDENTITY_MAT4], &near_mvp);
        assert_eq!(geo.indices(), &[0, 1, 2], "a near-plane triangle is kept");
    }

    // --- the software-backend decimation (vertex clustering) ------------------

    #[test]
    fn decimate_keeps_a_spread_triangle() {
        // Corners far apart -> distinct cells -> the triangle survives whole.
        let (v, i) = triangle([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        let (ov, oi) = decimate(&v, &i);
        assert_eq!(oi.len(), 3, "the spread triangle is kept");
        assert_eq!(ov.len() / SKINNED_VERTEX_STRIDE, 3);
    }

    #[test]
    fn decimate_welds_a_dense_cluster_and_drops_degenerate_triangles() {
        // Many near-coincident triangles at the origin (far smaller than one cell)
        // all weld to a single representative -> every one is degenerate + dropped;
        // one far-away spread triangle keeps the bbox (hence the cell) large and is
        // the only survivor. This exercises the entry-already-present weld path and
        // the dropped (degenerate) filter_map arm.
        let mut v = Vec::new();
        let mut i = Vec::new();
        (0..50_u32).for_each(|t| {
            let e = t as f32 * 1e-5;
            [[e, 0.0, 0.0], [0.0, e, 0.0], [0.0, 0.0, e]]
                .iter()
                .for_each(|p| v.extend_from_slice(&one_vertex(*p, [1.0; 4])));
            i.extend_from_slice(&[t * 3, t * 3 + 1, t * 3 + 2]);
        });
        [[100.0, 0.0, 0.0], [100.0, 40.0, 0.0], [100.0, 0.0, 40.0]]
            .iter()
            .for_each(|p| v.extend_from_slice(&one_vertex(*p, [1.0; 4])));
        i.extend_from_slice(&[150, 151, 152]);
        let (_ov, oi) = decimate(&v, &i);
        assert_eq!(oi.len(), 3, "only the spread triangle survives");
    }

    #[test]
    fn decimate_of_a_degenerate_point_mesh_yields_no_triangles() {
        // All corners coincident -> zero diagonal -> the cell floor applies and the
        // three verts weld to one representative, so the triangle is dropped.
        let (v, i) = triangle([1.0, 1.0, 1.0], [1.0, 1.0, 1.0], [1.0, 1.0, 1.0]);
        let (ov, oi) = decimate(&v, &i);
        assert!(oi.is_empty());
        assert_eq!(ov.len() / SKINNED_VERTEX_STRIDE, 1);
    }

    // --- the cache (load decimates; pose looks up) ----------------------------

    #[test]
    fn cache_load_decimates_then_poses_a_real_triangle() {
        let (v, i) = ccw_triangle();
        let cache = SkinnedMeshCache::load(&[(3, v, i)]);
        let geo = cache
            .pose(3, &[IDENTITY_MAT4], &IDENTITY_MAT4)
            .expect("mesh 3 is cached");
        assert_eq!(geo.indices().len(), 3);
        assert_eq!(geo.position(0), [0.0, 0.0, 0.0]);
    }

    #[test]
    fn unknown_mesh_id_poses_to_none() {
        let (v, i) = ccw_triangle();
        let cache = SkinnedMeshCache::load(&[(5, v, i)]);
        assert!(cache.pose(999, &[IDENTITY_MAT4], &IDENTITY_MAT4).is_none());
    }

    #[test]
    fn default_cache_is_empty() {
        let cache = SkinnedMeshCache::default();
        assert!(cache.pose(0, &[IDENTITY_MAT4], &IDENTITY_MAT4).is_none());
    }
}
