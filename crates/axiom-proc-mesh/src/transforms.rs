//! The one-input mesh operators: Transform, Extrude, Bevel, Bend, Displace,
//! UVProject, Triangulate.

use axiom_field::{EvalContext, FieldBuilder, FieldGraph, FieldId, FieldOp};
use axiom_kernel::Seconds;
use axiom_math::{Vec2, Vec3};
use axiom_proc_core::NodeEval;

use crate::mesh_buffer::MeshBuffer;

/// The centroid of a position list (origin for an empty list).
fn centroid(positions: &[Vec3]) -> Vec3 {
    let count = positions.len().max(1) as f32;
    let sum = positions.iter().fold(Vec3::ZERO, |acc, p| {
        Vec3::new(acc.x + p.x, acc.y + p.y, acc.z + p.z)
    });
    Vec3::new(sum.x / count, sum.y / count, sum.z / count)
}

/// **Transform** — translate then component-scale every vertex. Params:
/// `[tx, ty, tz, sx, sy, sz]`. Normals and UVs pass through.
pub(crate) fn transform(
    ctx: NodeEval<'_, MeshBuffer>,
    _fields: &[FieldGraph],
) -> Option<MeshBuffer> {
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
        MeshBuffer::from_parts(
            positions,
            src.normals().to_vec(),
            src.uvs().to_vec(),
            src.indices().to_vec(),
        )
    })
}

/// **Extrude** — thicken the mesh into a parallel shell: keep the input and add a
/// copy offset by `distance` along +Y. A deliberately minimal v0 extrude (no side
/// walls). Params: `[distance]`.
pub(crate) fn extrude(
    ctx: NodeEval<'_, MeshBuffer>,
    _fields: &[FieldGraph],
) -> Option<MeshBuffer> {
    let distance = ctx.params().first().map(|p| p.as_scalar().get());
    ctx.inputs().first().zip(distance).and_then(|(src, d)| {
        let base = src.vertex_count() as u32;
        let positions = src
            .positions()
            .iter()
            .copied()
            .chain(src.positions().iter().map(|p| Vec3::new(p.x, p.y + d, p.z)))
            .collect();
        let normals = src
            .normals()
            .iter()
            .copied()
            .chain(src.normals().iter().copied())
            .collect();
        let uvs = src
            .uvs()
            .iter()
            .copied()
            .chain(src.uvs().iter().copied())
            .collect();
        let indices = src
            .indices()
            .iter()
            .copied()
            .chain(src.indices().iter().map(|i| i + base))
            .collect();
        MeshBuffer::from_parts(positions, normals, uvs, indices)
    })
}

/// **Bevel** — pull every vertex toward the mesh centroid by `amount` (0..1), a
/// crude chamfer/inset. Params: `[amount]`. Normals/UVs pass through.
pub(crate) fn bevel(
    ctx: NodeEval<'_, MeshBuffer>,
    _fields: &[FieldGraph],
) -> Option<MeshBuffer> {
    let amount = ctx
        .params()
        .first()
        .map(|p| p.as_scalar().get().clamp(0.0, 1.0));
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
        MeshBuffer::from_parts(
            positions,
            src.normals().to_vec(),
            src.uvs().to_vec(),
            src.indices().to_vec(),
        )
    })
}

/// **Bend** — rotate each vertex about the Z axis by `angle × x`, bending a bar
/// laid along X. Params: `[angle]` (radians per unit x). Normals/UVs pass through.
pub(crate) fn bend(
    ctx: NodeEval<'_, MeshBuffer>,
    _fields: &[FieldGraph],
) -> Option<MeshBuffer> {
    let angle = ctx.params().first().map(|p| p.as_scalar().get());
    ctx.inputs().first().zip(angle).and_then(|(src, a)| {
        let positions = src
            .positions()
            .iter()
            .map(|p| {
                let theta = a * p.x;
                Vec3::new(
                    p.x * theta.cos() - p.y * theta.sin(),
                    p.x * theta.sin() + p.y * theta.cos(),
                    p.z,
                )
            })
            .collect();
        MeshBuffer::from_parts(
            positions,
            src.normals().to_vec(),
            src.uvs().to_vec(),
            src.indices().to_vec(),
        )
    })
}

/// The two-node field `Noise(seed, Point)` — the graph that *is* the historical
/// hardcoded `Displace` height, written in the field language instead of in Rust.
///
/// `Point` is the vertex position and `Noise` is `axiom_noise::value_noise` at
/// that point, which is exactly what the operator used to compute inline. Because
/// both sides call the same `value_noise`, a recipe that names no field displaces
/// bit-for-bit as it always did; the difference is that the height is now a value
/// a caller can read, diff, replace, or serialize.
fn value_noise_field(seed: u64) -> FieldGraph {
    let (builder, point) =
        FieldBuilder::new(FieldId::of_name("proc-mesh/displace/value-noise"), 1).push(
            FieldOp::Point,
            Vec::new(),
            Vec::new(),
        );
    let (builder, height) = builder.push_noise(seed, point);
    builder.build(height)
}

/// **Displace** — push each vertex along its normal by `amount ×
/// field(position, normal)`. Params: `[amount]` or `[amount, field_index]`.
///
/// With a `field_index` the height is the field table entry it names, evaluated
/// with the field's `point` set to the vertex position and its `normal` to the
/// vertex normal (`uv` is zero and time is zero — a bake is not animated). With
/// no `field_index` the height is [`value_noise_field`], the two-node graph
/// equivalent of the noise this operator used to compute inline, seeded from the
/// node's entropy stream exactly as before. A `field_index` naming no table entry
/// fails the node rather than silently falling back.
pub(crate) fn displace(
    mut ctx: NodeEval<'_, MeshBuffer>,
    fields: &[FieldGraph],
) -> Option<MeshBuffer> {
    let seed = ctx.stream().next_u64();
    let p = ctx.params();
    let amount = p.first().map(|p| p.as_scalar().get());
    let graph = p.get(1).map_or_else(
        || Some(value_noise_field(seed)),
        |slot| fields.get(slot.as_int() as usize).cloned(),
    );
    ctx.inputs()
        .first()
        .zip(amount)
        .zip(graph)
        .and_then(|((src, amt), graph)| {
            let displaced: Option<Vec<Vec3>> = src
                .positions()
                .iter()
                .zip(src.normals())
                .map(|(pos, nrm)| {
                    graph
                        .evaluate(&EvalContext::new(
                            *pos,
                            Vec2::ZERO,
                            *nrm,
                            Seconds::finite_or_zero(0.0),
                        ))
                        .ok()
                        .map(|height| {
                            let n = height.as_scalar().get() * amt;
                            Vec3::new(pos.x + nrm.x * n, pos.y + nrm.y * n, pos.z + nrm.z * n)
                        })
                })
                .collect();
            displaced.and_then(|positions| {
                MeshBuffer::from_parts(
                    positions,
                    src.normals().to_vec(),
                    src.uvs().to_vec(),
                    src.indices().to_vec(),
                )
            })
        })
}

/// **UVProject** — replace UVs with a planar XZ projection scaled by `scale`.
/// Params: `[scale]`. Positions/normals/indices pass through.
pub(crate) fn uv_project(
    ctx: NodeEval<'_, MeshBuffer>,
    _fields: &[FieldGraph],
) -> Option<MeshBuffer> {
    let scale = ctx.params().first().map(|p| p.as_scalar().get());
    ctx.inputs().first().zip(scale).and_then(|(src, s)| {
        let uvs = src
            .positions()
            .iter()
            .map(|p| Vec2::new(p.x * s, p.z * s))
            .collect();
        MeshBuffer::from_parts(
            src.positions().to_vec(),
            src.normals().to_vec(),
            uvs,
            src.indices().to_vec(),
        )
    })
}

/// **Triangulate** — the explicit gate that a mesh is a valid triangle list. Our
/// generators already emit triangles, so it re-wraps the input (and fails a
/// non-triangular buffer). No params.
pub(crate) fn triangulate(
    ctx: NodeEval<'_, MeshBuffer>,
    _fields: &[FieldGraph],
) -> Option<MeshBuffer> {
    ctx.inputs().first().and_then(|src| {
        MeshBuffer::from_parts(
            src.positions().to_vec(),
            src.normals().to_vec(),
            src.uvs().to_vec(),
            src.indices().to_vec(),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::{FieldBuilder, FieldGraph, FieldId, FieldOp, Vec3};
    use axiom_field::FieldValue;
    use crate::dispatch::mesh_eval;
    use crate::mesh_buffer::MeshBuffer;
    use crate::mesh_op::MeshOp;
    use crate::proc_mesh_api::ProcMeshApi;
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
        ProcCore::new()
            .execute(&g, 3, &SpaceApi::root(), mesh_eval)
            .ok()
    }

    #[test]
    fn transform_translates_and_scales_and_needs_six_params() {
        let m = cube_then(
            MeshOp::Transform,
            vec![s(10.0), s(0.0), s(0.0), s(1.0), s(1.0), s(1.0)],
            1,
        )
        .unwrap();
        // Every vertex shifted +10 in x.
        assert!(m.positions().iter().all(|p| p.x >= 9.0));
        assert!(cube_then(MeshOp::Transform, vec![s(1.0)], 1).is_none());
        assert!(cube_then(
            MeshOp::Transform,
            vec![s(0.0), s(0.0), s(0.0), s(1.0), s(1.0), s(1.0)],
            0
        )
        .is_none());
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
        assert!(m
            .positions()
            .iter()
            .all(|p| p.x.abs() < 1e-5 && p.y.abs() < 1e-5 && p.z.abs() < 1e-5));
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

    /// A sphere feeding a Displace, baked with `fields`. A UV sphere's vertices
    /// are off the noise lattice's integer corners, so the displacement is
    /// genuinely non-uniform — a cube's are not, and would hide a regression.
    fn displaced_sphere(params: Vec<Param>, fields: &[FieldGraph]) -> Option<MeshBuffer> {
        let mut g = RecipeGraph::new(RecipeId::from_raw(1), 1);
        let sphere = g.add(
            MeshOp::Sphere as u16,
            vec![s(1.0), Param::int(6), Param::int(8)],
            vec![],
        );
        g.add(MeshOp::Displace as u16, params, vec![sphere]);
        ProcMeshApi::new().bake_with_fields(&g, 3, fields).ok()
    }

    /// The FNV-1a digest of every position's `f32` bit pattern — a byte-identity
    /// pin, not an approximate one.
    fn position_digest(mesh: &MeshBuffer) -> u64 {
        mesh.positions()
            .iter()
            .flat_map(|p| [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()])
            .fold(0xcbf2_9ce4_8422_2325_u64, |h, word| {
                (h ^ u64::from(word)).wrapping_mul(0x0000_0100_0000_01B3)
            })
    }

    #[test]
    fn a_field_driven_displace_is_byte_identical_to_the_hardcoded_noise_it_replaced() {
        // The golden was recorded from the pre-field implementation
        // (`value_noise(stream_seed, position) * amount` along the normal) for
        // this exact recipe, seed and address. Manifest 05 retargeted Displace at
        // a field graph; this digest is what proves no recipe changed meaning.
        let baked = displaced_sphere(vec![s(0.3)], &[]).unwrap();
        assert_eq!(position_digest(&baked), 0xB195_5AFD_14D3_6614);
        assert_eq!(baked.vertex_count(), 63);
        // The displacement is real, not a no-op that would digest identically.
        let far = baked
            .positions()
            .iter()
            .map(|p| (p.length() - 1.0).abs())
            .fold(0.0_f32, f32::max);
        assert!(far > 0.1, "the noise actually moved vertices, got {far}");
        // `bake` and `bake_with_fields` with an empty table agree.
        let mut g = RecipeGraph::new(RecipeId::from_raw(1), 1);
        let sphere = g.add(
            MeshOp::Sphere as u16,
            vec![s(1.0), Param::int(6), Param::int(8)],
            vec![],
        );
        g.add(MeshOp::Displace as u16, vec![s(0.3)], vec![sphere]);
        assert_eq!(ProcMeshApi::new().bake(&g, 3).unwrap(), baked);
    }

    #[test]
    fn the_displacement_is_the_field_value_times_the_amount_along_the_normal() {
        // A constant height pins the arithmetic exactly: every vertex moves by
        // `field * amount` along its own normal, evaluated in that order — the
        // formula the hardcoded noise implementation applied, now applied to a
        // value the caller chooses.
        let (builder, node) = FieldBuilder::new(FieldId::of_name("proc-mesh/test/const"), 1)
            .push_const(FieldValue::scalar(Scalar::new(0.3)));
        let graph = builder.build(node);
        assert_eq!(graph.validate(), Ok(()));

        let plain = displaced_sphere(vec![s(0.0)], &[]).unwrap();
        let moved = displaced_sphere(vec![s(0.7), Param::int(0)], &[graph]).unwrap();
        let expected: Vec<Vec3> = plain
            .positions()
            .iter()
            .zip(plain.normals())
            .map(|(pos, nrm)| {
                let n = 0.3_f32 * 0.7;
                Vec3::new(pos.x + nrm.x * n, pos.y + nrm.y * n, pos.z + nrm.z * n)
            })
            .collect();
        assert_eq!(moved.positions(), expected.as_slice());
    }

    #[test]
    fn a_field_index_naming_no_table_entry_fails_the_node() {
        assert!(displaced_sphere(vec![s(0.3), Param::int(2)], &[]).is_none());
        // A field that cannot evaluate fails the node too: this graph's declared
        // output names a node built by a different builder.
        let (_, node) = FieldBuilder::new(FieldId::of_name("proc-mesh/test/other"), 1).push(
            FieldOp::Point,
            Vec::new(),
            Vec::new(),
        );
        let dangling = FieldBuilder::new(FieldId::of_name("proc-mesh/test/dangling"), 1).build(node);
        assert!(displaced_sphere(vec![s(0.3), Param::int(0)], &[dangling]).is_none());
    }

    #[test]
    fn a_displace_field_reads_the_vertex_normal_as_well_as_its_position() {
        // `Component(Normal, 1)` — the height is the normal's Y, which no
        // position-only field could express. It is what proves the operator
        // supplies `normal` and not only `point`.
        let (builder, normal) = FieldBuilder::new(FieldId::of_name("proc-mesh/test/normal-y"), 1)
            .push(FieldOp::Normal, Vec::new(), Vec::new());
        let (builder, y) = builder.push(FieldOp::Component, vec![Param::int(1)], vec![normal]);
        let graph = builder.build(y);
        assert_eq!(graph.validate(), Ok(()));

        let plain = displaced_sphere(vec![s(0.0)], &[]).unwrap();
        let by_normal = displaced_sphere(vec![s(1.0), Param::int(0)], &[graph]).unwrap();
        let expected: Vec<Vec3> = plain
            .positions()
            .iter()
            .zip(plain.normals())
            .map(|(pos, nrm)| {
                let n = nrm.y;
                Vec3::new(pos.x + nrm.x * n, pos.y + nrm.y * n, pos.z + nrm.z * n)
            })
            .collect();
        assert_eq!(by_normal.positions(), expected.as_slice());
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
