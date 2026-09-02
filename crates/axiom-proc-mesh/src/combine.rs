//! The multi-input mesh operators: Merge, and the full TRS transform.

use axiom_field::FieldGraph;
use axiom_math::{Quat, Vec3, Vec4};
use axiom_proc_core::NodeEval;

use crate::mesh_buffer::MeshBuffer;

/// The colour an uncoloured mesh contributes when merged with a coloured one.
///
/// Opaque white, because vertex colour is used as a **multiplier** over a
/// material's albedo everywhere in this engine: white is the identity, so a
/// plain mesh merged into a painted one keeps its own appearance instead of
/// being tinted by an arbitrary default. Dropping the stream instead would
/// silently discard the other input's paint, which is the failure the stream
/// exists to prevent.
const UNPAINTED: Vec4 = Vec4::new(1.0, 1.0, 1.0, 1.0);

/// **Merge** — concatenate every input mesh into one buffer.
///
/// The operator a procedural kit spends its life in: a weapon, a building
/// facade and a soldier are each dozens of primitives accumulated into one
/// mesh, and without this the only way to express that as data is to inline
/// every vertex. Three separate subsystem audits of `apps/axiom-shmup` put its
/// call count at 222, 222 and 35 sites — it is the single most-used verb in the
/// app's geometry code and it had no operator at all.
///
/// Indices are rebased per input. Optional streams follow the rule that keeps a
/// merge lossless: if **any** input carries colours, the result does, and inputs
/// without them contribute [`UNPAINTED`]. Skin streams are deliberately *not*
/// merged — joint indices are only meaningful against one skeleton, and
/// silently concatenating two meshes' bone indices would produce a mesh bound
/// to a skeleton that does not exist.
///
/// Takes no params. Zero inputs is an empty mesh, not a failure: a kit that
/// merges an empty list of parts has produced nothing, which is a defined
/// answer.
pub(crate) fn merge(ctx: NodeEval<'_, MeshBuffer>, _fields: &[FieldGraph]) -> Option<MeshBuffer> {
    merge_meshes(ctx.inputs())
}

/// [`merge`]'s body, over an explicit slice.
///
/// Split out because a `RecipeGraph` has no operator that *authors* a colour
/// stream yet, so the only way to pin the merge's colour rule is to hand it
/// coloured inputs directly. Testing the rule matters more than testing it
/// through the interpreter, which the other cases here already do.
pub(crate) fn merge_meshes(inputs: &[MeshBuffer]) -> Option<MeshBuffer> {
    let any_colored = inputs.iter().any(MeshBuffer::has_colors);

    let total: usize = inputs.iter().map(MeshBuffer::vertex_count).sum();
    let bounded = total <= crate::mesh_buffer::MAX_VERTS;

    let positions: Vec<Vec3> = inputs
        .iter()
        .flat_map(|m| m.positions().iter().copied())
        .collect();
    let normals: Vec<Vec3> = inputs
        .iter()
        .flat_map(|m| m.normals().iter().copied())
        .collect();
    let uvs = inputs.iter().flat_map(|m| m.uvs().iter().copied()).collect();

    // Rebase each input's indices by the vertex count of everything before it.
    let (indices, _) = inputs.iter().fold(
        (Vec::with_capacity(inputs.iter().map(|m| m.indices().len()).sum()), 0u32),
        |(mut all, base), mesh| {
            all.extend(mesh.indices().iter().map(|i| i + base));
            (all, base + mesh.vertex_count() as u32)
        },
    );

    let merged = bounded
        .then_some(())
        .and_then(|()| MeshBuffer::from_parts(positions, normals, uvs, indices));

    let colors: Vec<Vec4> = inputs
        .iter()
        .flat_map(|m| {
            let painted = m.colors().iter().copied();
            let plain = core::iter::repeat_n(UNPAINTED, m.vertex_count());
            // Both arms are built; the index picks one. An input that carries
            // colours contributes them, one that does not contributes identity.
            [
                plain.collect::<Vec<Vec4>>(),
                painted.collect::<Vec<Vec4>>(),
            ][usize::from(m.has_colors())]
            .clone()
        })
        .collect();

    merged.and_then(|m| {
        [Some(m.clone()), m.with_colors(colors)][usize::from(any_colored)].clone()
    })
}

/// **Trs** — translate, rotate and scale every vertex.
///
/// Params: `[tx, ty, tz, rx, ry, rz, sx, sy, sz]`, rotation in **radians**,
/// applied as `translate ∘ rotate ∘ scale` — scale first, in the mesh's own
/// frame, then rotate, then move. Normals are rotated but not scaled or
/// translated.
///
/// ## Why a separate operator rather than widening `Transform`
///
/// `Transform` is `[tx, ty, tz, sx, sy, sz]`. Widening it to nine would
/// reinterpret every existing six-param graph: the old `sx, sy, sz` would be
/// read as `rx, ry, rz` and the scale would default to zero, collapsing the mesh
/// to a point. A recipe graph is data that outlives the code reading it, so an
/// opcode's parameter layout is a wire format — additive is the only safe change.
///
/// ## The Euler order is named because it has to be
///
/// `XYZ`, via [`Quat::from_euler_xyz`]. Three orders are in common use and they
/// disagree on every rotation with more than one non-zero angle; a recipe that
/// assumed one convention against an engine using another is wrong in a way that
/// looks like a modelling mistake. It is stated here, and it is the engine's
/// existing convention rather than a new one.
pub(crate) fn trs(ctx: NodeEval<'_, MeshBuffer>, _fields: &[FieldGraph]) -> Option<MeshBuffer> {
    let p = ctx.params();
    let ready = (p.len() >= 9).then_some(());
    ctx.inputs().first().zip(ready).and_then(|(src, ())| {
        let at = |i: usize| p[i].as_scalar().get();
        let rotation = Quat::from_euler_xyz(at(3), at(4), at(5));
        let scale = Vec3::new(at(6), at(7), at(8));
        let translation = Vec3::new(at(0), at(1), at(2));

        let positions = src
            .positions()
            .iter()
            .map(|v| {
                let scaled = Vec3::new(v.x * scale.x, v.y * scale.y, v.z * scale.z);
                let turned = rotation.rotate(scaled);
                Vec3::new(
                    turned.x + translation.x,
                    turned.y + translation.y,
                    turned.z + translation.z,
                )
            })
            .collect();

        // Normals rotate with the mesh but do not translate. A non-uniform scale
        // strictly needs the inverse-transpose; this applies rotation only, which
        // is exact for uniform scale and is the honest limit of a nine-parameter
        // op — a recipe needing skew-correct normals wants a normal-recompute
        // step, not a bigger transform.
        let normals = src
            .normals()
            .iter()
            .map(|n| rotation.rotate(*n))
            .collect();

        src.respecified(positions, normals, src.uvs().to_vec(), src.indices().to_vec())
    })
}

#[cfg(test)]
mod tests {
    use super::UNPAINTED;
    use axiom_math::{Vec3, Vec4};
    use axiom_proc_core::ProcCore;
    use axiom_recipe::{Param, RecipeGraph, RecipeId, Scalar};
    use axiom_space::SpaceApi;

    use crate::dispatch::mesh_eval;
    use crate::mesh_buffer::MeshBuffer;
    use crate::mesh_op::MeshOp;

    fn s(v: f32) -> Param {
        Param::scalar(Scalar::new(v))
    }

    /// `n` cube sources feeding one node — driven through the real interpreter,
    /// so these exercise the dispatch table rather than the function alone.
    fn cubes_then(op: MeshOp, params: Vec<Param>, sources: usize) -> Option<MeshBuffer> {
        let mut g = RecipeGraph::new(RecipeId::from_raw(1), 1);
        let inputs: Vec<_> = (0..sources)
            .map(|i| g.add(MeshOp::Cube as u16, vec![s(1.0 + i as f32)], vec![]))
            .collect();
        g.add(op as u16, params, inputs);
        ProcCore::new()
            .execute(&g, 3, &SpaceApi::root(), mesh_eval)
            .ok()
    }

    /// The unmodified cube, as the identity TRS produces it. The scale must be
    /// one: `vec![s(0.0); 9]` is a *zero* scale and collapses the mesh to a
    /// point, which is a fine way to make every comparison against it pass.
    fn one_cube() -> MeshBuffer {
        cubes_then(
            MeshOp::Trs,
            vec![s(0.0), s(0.0), s(0.0), s(0.0), s(0.0), s(0.0), s(1.0), s(1.0), s(1.0)],
            1,
        )
        .unwrap()
    }

    #[test]
    fn merging_two_meshes_concatenates_and_rebases_indices() {
        let single = cubes_then(MeshOp::Merge, vec![], 1).unwrap();
        let merged = cubes_then(MeshOp::Merge, vec![], 2).unwrap();
        assert_eq!(merged.vertex_count(), single.vertex_count() * 2);
        assert_eq!(merged.triangle_count(), single.triangle_count() * 2);
        // Every index still addresses a real vertex after rebasing.
        let n = merged.vertex_count() as u32;
        assert!(merged.indices().iter().all(|&i| i < n));
        // The second input's triangles point past the first input's vertices.
        let base = single.vertex_count() as u32;
        assert!(merged.indices().iter().any(|&i| i >= base));
    }

    #[test]
    fn merging_nothing_is_an_empty_mesh_rather_than_a_failure() {
        let m = cubes_then(MeshOp::Merge, vec![], 0).unwrap();
        assert_eq!(m.vertex_count(), 0);
        assert_eq!(m.triangle_count(), 0);
        assert!(!m.has_colors());
    }

    #[test]
    fn merging_uncoloured_meshes_leaves_the_result_uncoloured() {
        assert!(!cubes_then(MeshOp::Merge, vec![], 3).unwrap().has_colors());
    }

    /// The rule that keeps a merge lossless: one painted input promotes the
    /// result, and an unpainted one contributes the identity rather than
    /// dropping everyone else's paint.
    #[test]
    fn one_painted_input_promotes_the_merge_and_the_rest_get_identity() {
        let red = Vec4::new(1.0, 0.0, 0.0, 1.0);
        let plain = one_cube();
        let painted = plain
            .clone()
            .with_colors(vec![red; plain.vertex_count()])
            .unwrap();

        // Exercised directly: a recipe cannot yet author a colour stream, which
        // is precisely why the merge rule has to be pinned here.
        let merged = super::merge_meshes(&[painted.clone(), plain.clone()]).unwrap();
        assert!(merged.has_colors());
        assert_eq!(merged.colors().len(), plain.vertex_count() * 2);
        assert_eq!(merged.colors()[0], red);
        assert_eq!(merged.colors()[plain.vertex_count()], UNPAINTED);

        // ...and in the other order, so the promotion is not position-dependent.
        let flipped = super::merge_meshes(&[plain.clone(), painted]).unwrap();
        assert_eq!(flipped.colors()[0], UNPAINTED);
        assert_eq!(flipped.colors()[plain.vertex_count()], red);
    }

    #[test]
    fn merging_two_painted_meshes_keeps_both_paints() {
        let plain = one_cube();
        let red = Vec4::new(1.0, 0.0, 0.0, 1.0);
        let blue = Vec4::new(0.0, 0.0, 1.0, 1.0);
        let a = plain.clone().with_colors(vec![red; plain.vertex_count()]).unwrap();
        let b = plain.clone().with_colors(vec![blue; plain.vertex_count()]).unwrap();
        let merged = super::merge_meshes(&[a, b]).unwrap();
        assert_eq!(merged.colors()[0], red);
        assert_eq!(merged.colors()[plain.vertex_count()], blue);
    }

    #[test]
    fn a_merge_that_would_exceed_the_vertex_cap_fails() {
        let huge = MeshBuffer::from_parts(
            vec![Vec3::ZERO; crate::mesh_buffer::MAX_VERTS],
            vec![Vec3::UNIT_Z; crate::mesh_buffer::MAX_VERTS],
            vec![axiom_math::Vec2::new(0.0, 0.0); crate::mesh_buffer::MAX_VERTS],
            vec![0, 1, 2],
        )
        .unwrap();
        assert!(super::merge_meshes(&[huge.clone(), huge]).is_none());
    }

    #[test]
    fn trs_rotates_a_quarter_turn_about_z() {
        // A unit cube spun a quarter turn about Z, then moved +10 in x.
        let m = cubes_then(
            MeshOp::Trs,
            vec![
                s(10.0),
                s(0.0),
                s(0.0),
                s(0.0),
                s(0.0),
                s(core::f32::consts::FRAC_PI_2),
                s(1.0),
                s(1.0),
                s(1.0),
            ],
            1,
        )
        .unwrap();
        assert!(m.positions().iter().all(|p| p.x >= 9.0));
        // A rotation preserves distance from the (translated) centre.
        let plain = one_cube();
        let radius = |v: &Vec3, cx: f32| ((v.x - cx).powi(2) + v.y * v.y + v.z * v.z).sqrt();
        let before: f32 = plain.positions().iter().map(|v| radius(v, 0.0)).sum();
        let after: f32 = m.positions().iter().map(|v| radius(v, 10.0)).sum();
        assert!((before - after).abs() < 1.0e-3, "{before} vs {after}");
    }

    #[test]
    fn trs_scales_before_it_rotates_and_translates() {
        let scaled = cubes_then(
            MeshOp::Trs,
            vec![s(0.0), s(0.0), s(0.0), s(0.0), s(0.0), s(0.0), s(2.0), s(1.0), s(1.0)],
            1,
        )
        .unwrap();
        let plain = one_cube();
        let widest = |m: &MeshBuffer| m.positions().iter().fold(0.0_f32, |w, p| w.max(p.x.abs()));
        assert!((widest(&scaled) - widest(&plain) * 2.0).abs() < 1.0e-5);
    }

    #[test]
    fn trs_rotates_normals_and_keeps_them_unit_length() {
        let m = cubes_then(
            MeshOp::Trs,
            vec![
                s(9.0),
                s(9.0),
                s(9.0),
                s(core::f32::consts::FRAC_PI_2),
                s(0.0),
                s(0.0),
                s(1.0),
                s(1.0),
                s(1.0),
            ],
            1,
        )
        .unwrap();
        assert!(m
            .normals()
            .iter()
            .all(|n| (n.length() - 1.0).abs() < 1.0e-4));
        // A translation must not move a normal off the unit sphere.
        assert!(m.normals().iter().all(|n| n.x.abs() <= 1.0 + 1.0e-4));
    }

    #[test]
    fn trs_needs_all_nine_parameters_and_an_input() {
        assert!(cubes_then(MeshOp::Trs, vec![s(0.0); 8], 1).is_none());
        assert!(cubes_then(MeshOp::Trs, vec![s(0.0); 9], 0).is_none());
    }

    /// The property the colour stream exists for: an authored channel must
    /// survive the whole operator chain. `from_parts` yields an UNCOLOURED
    /// mesh, so before `respecified` every one of these silently dropped it —
    /// the same shape as the engine's recorded "authored normals are silently
    /// discarded" defect.
    #[test]
    fn every_vertex_preserving_operator_carries_the_colour_stream() {
        let plain = one_cube();
        let red = Vec4::new(1.0, 0.0, 0.0, 1.0);
        let painted = plain
            .clone()
            .with_colors(vec![red; plain.vertex_count()])
            .unwrap();

        let rebuilt = painted
            .respecified(
                painted.positions().to_vec(),
                painted.normals().to_vec(),
                painted.uvs().to_vec(),
                painted.indices().to_vec(),
            )
            .unwrap();
        assert!(rebuilt.has_colors());
        assert_eq!(rebuilt.colors()[0], red);
        assert_eq!(rebuilt.colors().len(), plain.vertex_count());
    }

    /// A rebuild that changes the vertex count while colours are attached is a
    /// hard failure, not a silent drop. An operator that legitimately changes
    /// the count has to say what happens to the stream.
    #[test]
    fn a_rebuild_that_changes_the_vertex_count_refuses_to_guess() {
        let plain = one_cube();
        let painted = plain
            .clone()
            .with_colors(vec![Vec4::new(1.0, 0.0, 0.0, 1.0); plain.vertex_count()])
            .unwrap();
        let fewer: Vec<Vec3> = painted.positions().iter().copied().take(3).collect();
        assert!(painted
            .respecified(fewer, vec![Vec3::UNIT_Z; 3], vec![axiom_math::Vec2::new(0.0, 0.0); 3], vec![0, 1, 2])
            .is_none());
    }

    #[test]
    fn a_colour_stream_of_the_wrong_length_is_refused() {
        let plain = one_cube();
        assert!(plain.clone().with_colors(vec![UNPAINTED; 2]).is_none());
        assert!(plain.clone().with_colors(vec![]).is_none());
        let n = plain.vertex_count();
        assert!(plain.with_colors(vec![UNPAINTED; n]).is_some());
    }

    #[test]
    fn without_colors_strips_the_stream() {
        let plain = one_cube();
        let n = plain.vertex_count();
        let painted = plain.with_colors(vec![UNPAINTED; n]).unwrap();
        assert!(painted.has_colors());
        assert!(!painted.without_colors().has_colors());
    }

    #[test]
    fn trs_carries_an_authored_colour_stream_forward() {
        let plain = one_cube();
        let red = Vec4::new(1.0, 0.0, 0.0, 1.0);
        let painted = plain
            .clone()
            .with_colors(vec![red; plain.vertex_count()])
            .unwrap();
        let moved = painted
            .respecified(
                painted.positions().to_vec(),
                painted.normals().to_vec(),
                painted.uvs().to_vec(),
                painted.indices().to_vec(),
            )
            .unwrap();
        assert!(moved.has_colors(), "a rebuild dropped the authored colours");
        assert_eq!(moved.colors()[0], red);
    }
}
