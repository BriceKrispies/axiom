//! The one-input mesh operators: Transform, Extrude, Bevel, Bend, Displace,
//! UVProject, Triangulate.

use axiom_math::{Vec2, Vec3};
use axiom_noise::value_noise;
use axiom_proc_core::NodeEval;

use crate::mesh_buffer::MeshBuffer;

/// The centroid of a position list (origin for an empty list).
fn centroid(positions: &[Vec3]) -> Vec3 {
    let count = positions.len().max(1) as f32;
    let sum = positions
        .iter()
        .fold(Vec3::ZERO, |acc, p| Vec3::new(acc.x + p.x, acc.y + p.y, acc.z + p.z));
    Vec3::new(sum.x / count, sum.y / count, sum.z / count)
}

/// **Transform** — translate then component-scale every vertex. Params:
/// `[tx, ty, tz, sx, sy, sz]`. Normals and UVs pass through.
pub(crate) fn transform(ctx: NodeEval<'_, MeshBuffer>) -> Option<MeshBuffer> {
    let p = ctx.params();
    let ready = (p.len() >= 6).then_some(());
    ctx.inputs().first().zip(ready).and_then(|(src, ())| {
        let positions = src
            .positions()
            .iter()
            .map(|v| {
                Vec3::new(
                    v.x * p[3].as_scalar().get() + p[0].as_scalar().get(),
                    v.y * p[4].as_scalar().get() + p[1].as_scalar().get(),
                    v.z * p[5].as_scalar().get() + p[2].as_scalar().get(),
                )
            })
            .collect();
        MeshBuffer::from_parts(positions, src.normals().to_vec(), src.uvs().to_vec(), src.indices().to_vec())
    })
}

/// **Extrude** — thicken the mesh into a parallel shell: keep the input and add a
/// copy offset by `distance` along +Y. A deliberately minimal v0 extrude (no side
/// walls). Params: `[distance]`.
pub(crate) fn extrude(ctx: NodeEval<'_, MeshBuffer>) -> Option<MeshBuffer> {
    let distance = ctx.params().first().map(|p| p.as_scalar().get());
    ctx.inputs().first().zip(distance).and_then(|(src, d)| {
        let base = src.vertex_count() as u32;
        let positions = src
            .positions()
            .iter()
            .copied()
            .chain(src.positions().iter().map(|p| Vec3::new(p.x, p.y + d, p.z)))
            .collect();
        let normals = src.normals().iter().copied().chain(src.normals().iter().copied()).collect();
        let uvs = src.uvs().iter().copied().chain(src.uvs().iter().copied()).collect();
        let indices = src.indices().iter().copied().chain(src.indices().iter().map(|i| i + base)).collect();
        MeshBuffer::from_parts(positions, normals, uvs, indices)
    })
}

/// **Bevel** — pull every vertex toward the mesh centroid by `amount` (0..1), a
/// crude chamfer/inset. Params: `[amount]`. Normals/UVs pass through.
pub(crate) fn bevel(ctx: NodeEval<'_, MeshBuffer>) -> Option<MeshBuffer> {
    let amount = ctx.params().first().map(|p| p.as_scalar().get().clamp(0.0, 1.0));
    ctx.inputs().first().zip(amount).and_then(|(src, t)| {
        let mid = centroid(src.positions());
        let positions = src
            .positions()
            .iter()
            .map(|p| {
                Vec3::new(
                    p.x + (mid.x - p.x) * t,
                    p.y + (mid.y - p.y) * t,
                    p.z + (mid.z - p.z) * t,
                )
            })
            .collect();
        MeshBuffer::from_parts(positions, src.normals().to_vec(), src.uvs().to_vec(), src.indices().to_vec())
    })
}

/// **Bend** — rotate each vertex about the Z axis by `angle × x`, bending a bar
/// laid along X. Params: `[angle]` (radians per unit x). Normals/UVs pass through.
pub(crate) fn bend(ctx: NodeEval<'_, MeshBuffer>) -> Option<MeshBuffer> {
    let angle = ctx.params().first().map(|p| p.as_scalar().get());
    ctx.inputs().first().zip(angle).and_then(|(src, a)| {
        let positions = src
            .positions()
            .iter()
            .map(|p| {
                let theta = a * p.x;
                Vec3::new(p.x * theta.cos() - p.y * theta.sin(), p.x * theta.sin() + p.y * theta.cos(), p.z)
            })
            .collect();
        MeshBuffer::from_parts(positions, src.normals().to_vec(), src.uvs().to_vec(), src.indices().to_vec())
    })
}

/// **Displace** — push each vertex along its normal by `amount × noise(position)`,
/// the noise seeded from the node's entropy stream. Params: `[amount]`.
pub(crate) fn displace(mut ctx: NodeEval<'_, MeshBuffer>) -> Option<MeshBuffer> {
    let seed = ctx.stream().next_u64();
    let amount = ctx.params().first().map(|p| p.as_scalar().get());
    ctx.inputs().first().zip(amount).and_then(|(src, amt)| {
        let positions = src
            .positions()
            .iter()
            .zip(src.normals())
            .map(|(pos, nrm)| {
                let n = value_noise(seed, *pos).get() * amt;
                Vec3::new(pos.x + nrm.x * n, pos.y + nrm.y * n, pos.z + nrm.z * n)
            })
            .collect();
        MeshBuffer::from_parts(positions, src.normals().to_vec(), src.uvs().to_vec(), src.indices().to_vec())
    })
}

/// **UVProject** — replace UVs with a planar XZ projection scaled by `scale`.
/// Params: `[scale]`. Positions/normals/indices pass through.
pub(crate) fn uv_project(ctx: NodeEval<'_, MeshBuffer>) -> Option<MeshBuffer> {
    let scale = ctx.params().first().map(|p| p.as_scalar().get());
    ctx.inputs().first().zip(scale).and_then(|(src, s)| {
        let uvs = src.positions().iter().map(|p| Vec2::new(p.x * s, p.z * s)).collect();
        MeshBuffer::from_parts(src.positions().to_vec(), src.normals().to_vec(), uvs, src.indices().to_vec())
    })
}

/// **Triangulate** — the explicit gate that a mesh is a valid triangle list. Our
/// generators already emit triangles, so it re-wraps the input (and fails a
/// non-triangular buffer). No params.
pub(crate) fn triangulate(ctx: NodeEval<'_, MeshBuffer>) -> Option<MeshBuffer> {
    ctx.inputs().first().and_then(|src| {
        MeshBuffer::from_parts(src.positions().to_vec(), src.normals().to_vec(), src.uvs().to_vec(), src.indices().to_vec())
    })
}

#[cfg(test)]
mod tests {
    use crate::dispatch::mesh_eval;
    use crate::mesh_buffer::MeshBuffer;
    use crate::mesh_op::MeshOp;
    use axiom_proc_core::ProcCore;
    use axiom_recipe::{Param, RecipeGraph, RecipeId, Scalar};
    use axiom_space::SpaceApi;

    fn s(v: f32) -> Param {
        Param::scalar(Scalar::new(v))
    }

    /// A cube (op source) feeding a one-input op, `input_count` links.
    fn cube_then(op: MeshOp, params: Vec<Param>, input_count: usize) -> Option<MeshBuffer> {
        let mut g = RecipeGraph::new(RecipeId::from_raw(1), 1);
        let c = g.add(MeshOp::Cube as u16, vec![s(2.0)], vec![]);
        let inputs = (0..input_count).map(|_| c).collect();
        g.add(op as u16, params, inputs);
        ProcCore::new().execute(&g, 3, &SpaceApi::root(), mesh_eval).ok()
    }

    #[test]
    fn transform_translates_and_scales_and_needs_six_params() {
        let m = cube_then(MeshOp::Transform, vec![s(10.0), s(0.0), s(0.0), s(1.0), s(1.0), s(1.0)], 1).unwrap();
        // Every vertex shifted +10 in x.
        assert!(m.positions().iter().all(|p| p.x >= 9.0));
        assert!(cube_then(MeshOp::Transform, vec![s(1.0)], 1).is_none());
        assert!(cube_then(MeshOp::Transform, vec![s(0.0), s(0.0), s(0.0), s(1.0), s(1.0), s(1.0)], 0).is_none());
    }

    #[test]
    fn extrude_doubles_the_geometry() {
        let m = cube_then(MeshOp::Extrude, vec![s(1.0)], 1).unwrap();
        assert_eq!(m.vertex_count(), 48); // 24 * 2
        assert_eq!(m.triangle_count(), 24);
        assert!(cube_then(MeshOp::Extrude, vec![], 1).is_none());
    }

    #[test]
    fn bevel_pulls_vertices_inward() {
        let m = cube_then(MeshOp::Bevel, vec![s(1.0)], 1).unwrap();
        // amount 1.0 collapses everything to the centroid (origin).
        assert!(m.positions().iter().all(|p| p.x.abs() < 1e-5 && p.y.abs() < 1e-5 && p.z.abs() < 1e-5));
        // amount clamps into [0, 1]: above 1 still collapses, below 0 is identity.
        let over = cube_then(MeshOp::Bevel, vec![s(2.0)], 1).unwrap();
        assert!(over.positions().iter().all(|p| p.x.abs() < 1e-5));
        let under = cube_then(MeshOp::Bevel, vec![s(-1.0)], 1).unwrap();
        let plain = cube_then(MeshOp::Triangulate, vec![], 1).unwrap();
        assert_eq!(under.positions(), plain.positions());
        assert!(cube_then(MeshOp::Bevel, vec![], 1).is_none());
    }

    #[test]
    fn bend_curves_and_needs_an_angle() {
        assert!(cube_then(MeshOp::Bend, vec![s(0.5)], 1).is_some());
        // Zero angle is the identity.
        let flat = cube_then(MeshOp::Bend, vec![s(0.0)], 1).unwrap();
        let plain = cube_then(MeshOp::Triangulate, vec![], 1).unwrap();
        assert_eq!(flat.positions(), plain.positions());
        assert!(cube_then(MeshOp::Bend, vec![], 1).is_none());
    }

    #[test]
    fn displace_moves_along_normals_deterministically() {
        let a = cube_then(MeshOp::Displace, vec![s(0.3)], 1).unwrap();
        let b = cube_then(MeshOp::Displace, vec![s(0.3)], 1).unwrap();
        assert_eq!(a.positions(), b.positions());
        assert!(cube_then(MeshOp::Displace, vec![], 1).is_none());
    }

    #[test]
    fn uv_project_replaces_uvs() {
        let m = cube_then(MeshOp::UVProject, vec![s(0.5)], 1).unwrap();
        assert_eq!(m.vertex_count(), 24);
        assert!(cube_then(MeshOp::UVProject, vec![], 1).is_none());
    }

    #[test]
    fn triangulate_passes_a_triangle_mesh_through() {
        let m = cube_then(MeshOp::Triangulate, vec![], 1).unwrap();
        assert_eq!(m.triangle_count(), 12);
        assert!(cube_then(MeshOp::Triangulate, vec![], 0).is_none());
    }
}
