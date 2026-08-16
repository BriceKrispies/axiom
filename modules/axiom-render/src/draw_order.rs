//! Deterministic per-draw ordering for the render command builder.
//!
//! The renderer composites with straight alpha, so draw order matters once a
//! frame carries translucency. This module resolves each [`RenderInput`] object
//! through the visibility + index gates and orders the survivors for correct
//! over-compositing: **opaque draws first** (submission order — they are
//! depth-tested, so front-to-back is fine), then **translucent draws**
//! (effective alpha `< 1`) sorted **back-to-front** by camera depth. The sort is
//! stable and ties break by submission index, so a tick's order is reproducible.
//! Without a camera every depth is `0`, so translucent draws keep submission order.

use axiom_math::Mat4;
use axiom_surface::LightingModel;

use crate::render_input::RenderInput;
use crate::render_pipeline_kind::RenderPipelineKind;

/// A resolved, ready-to-emit draw: the mesh/material/object identities and world
/// the command builder needs, plus its translucency class and the view-space
/// depth key [`ordered_draws`] sorts by.
#[derive(Debug, Clone)]
pub(crate) struct OrderedDraw {
    pub(crate) mesh_id: u64,
    pub(crate) material_id: u64,
    pub(crate) texture_id: u64,
    pub(crate) pipeline: u32,
    pub(crate) object_id: u64,
    pub(crate) object_tag: u32,
    pub(crate) index_count: u32,
    pub(crate) world: Mat4,
    translucent: bool,
    depth_key: f32,
}

/// Resolve and order a frame's drawable objects (see the module docs). Each
/// `Option`-combinator carries one gate: a failed gate drops the object.
pub(crate) fn ordered_draws(input: &RenderInput) -> Vec<OrderedDraw> {
    let mut out = Vec::new();
    ordered_draws_into(input, &mut out);
    out
}

/// Resolve and order a frame's drawable objects INTO `out`, reusing its
/// allocated capacity (clear + refill + sort) instead of allocating a fresh
/// `Vec` each frame — the per-frame reuse path. [`ordered_draws`] delegates.
pub(crate) fn ordered_draws_into(input: &RenderInput, out: &mut Vec<OrderedDraw>) {
    // The camera view orders translucent draws by view-space depth; absent a
    // camera every depth is `0`, so the stable sort leaves submission order.
    let view = input.camera().map(|c| c.view());

    out.clear();
    out.extend(input.objects().iter().filter_map(|object| {
        object
            .visible()
            .then_some(object)
            .and_then(|object| {
                input
                    .meshes()
                    .get(object.mesh_idx() as usize)
                    .map(|mesh| (object, mesh))
            })
            .and_then(|(object, mesh)| {
                input
                    .materials()
                    .get(object.material_idx() as usize)
                    .map(|material| (object, mesh, material))
            })
            .map(|(object, mesh, material)| {
                // Effective per-draw alpha = base-colour alpha × opacity;
                // a value `< 1` makes the draw translucent.
                let alpha = material.base_color().w * material.opacity().get();
                let translucent = alpha < 1.0;
                // View-space z of the object's origin: column 3 of `view *
                // world` is `view` applied to the world translation (w = 1),
                // so its z is the camera-space depth.
                let depth = view
                    .map(|v| v.multiply(object.world()).as_cols_array()[14])
                    .unwrap_or(0.0);
                OrderedDraw {
                    mesh_id: mesh.id(),
                    material_id: material.id(),
                    // Per-object albedo override wins when set (`!= 0`); else
                    // the material's own texture — a branchless table select.
                    texture_id: [material.texture_id(), object.texture_id()]
                        [usize::from(object.texture_id() != 0)],
                    // **The pipeline marker, derived rather than merely carried.**
                    // An unlit MATERIAL selects `UNLIT`; anything else keeps the
                    // object's own selection (which defaults to `BASIC_LIT`).
                    // Same branchless table select as the texture override above,
                    // and the same precedence rule: what the material says about
                    // its own appearance wins, because the object's marker is a
                    // per-instance override of a per-material fact and only the
                    // material knows how its surface is lit.
                    //
                    // This is the seam `RenderPipelineKind::UNLIT` has been
                    // waiting on since it was written: the marker was emitted,
                    // run-length-encoded into `SetPipeline`, and never selected by
                    // anything. Now an `axiom_surface::LightingModel::Unlit`
                    // material selects it, and the GPU backend derives the same
                    // marker from the same discriminant on its own side.
                    pipeline: [object.pipeline(), RenderPipelineKind::UNLIT]
                        [usize::from(material.lighting() == LightingModel::Unlit)],
                    object_id: object.id(),
                    object_tag: object.tag(),
                    index_count: mesh.index_count(),
                    world: object.world(),
                    translucent,
                    // Opaque draws carry depth key `0` so the stable sort
                    // keeps them in submission order; translucent draws carry
                    // their camera depth so they sort far→near.
                    depth_key: [0.0, depth][usize::from(translucent)],
                }
            })
    }));

    // Class key (opaque `0` < translucent `1`) groups opaque first; within a
    // class the depth key orders translucent far→near and leaves opaque untouched
    // (all `0`). A stable sort ties-breaks by submission index.
    out.sort_by(|a, b| {
        (a.translucent as u8)
            .cmp(&(b.translucent as u8))
            .then_with(|| a.depth_key.total_cmp(&b.depth_key))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render_api::RenderApi;
    use axiom_kernel::Ratio;
    use axiom_math::{Mat4, Vec3, Vec4};

    fn api() -> RenderApi {
        RenderApi::new()
    }

    fn half() -> Ratio {
        Ratio::new(0.5).expect("finite")
    }

    fn one() -> Ratio {
        Ratio::new(1.0).expect("finite")
    }

    /// A world matrix translated to `z` along the camera axis (identity view maps
    /// world z straight to view-space depth).
    fn at_z(z: f32) -> Mat4 {
        let mut cols = Mat4::IDENTITY.as_cols_array();
        cols[14] = z;
        Mat4::from_cols_array(cols)
    }

    /// The resolved draw object ids, in emit order.
    fn order(input: &crate::render_input::RenderInput) -> Vec<u64> {
        ordered_draws(input).iter().map(|d| d.object_id).collect()
    }

    #[test]
    fn translucent_draws_sort_back_to_front_after_opaque() {
        let api = api();
        let mut input = api.new_input(64, 64);
        // Identity view → world z is the view-space depth (more negative = farther).
        api.set_input_camera(&mut input, Mat4::IDENTITY, Mat4::IDENTITY);
        let mesh = api.add_input_mesh(&mut input, 1, 3);
        let opaque = api.add_input_basic_lit_material(&mut input, 10, Vec4::ONE);
        let glass =
            api.add_input_lit_material(&mut input, 20, Vec4::ONE, Vec3::ZERO, one(), half(), 0);
        // Submission order: opaque, then a NEAR translucent, then a FAR translucent.
        api.add_input_object(&mut input, 100, at_z(-3.0), mesh, opaque, true);
        api.add_input_object(&mut input, 200, at_z(-2.0), mesh, glass, true);
        api.add_input_object(&mut input, 300, at_z(-5.0), mesh, glass, true);
        // Opaque first (submission order), then translucent far→near: 300 then 200.
        assert_eq!(order(&input), vec![100, 300, 200]);
    }

    #[test]
    fn translucent_ties_keep_submission_order() {
        let api = api();
        let mut input = api.new_input(64, 64);
        api.set_input_camera(&mut input, Mat4::IDENTITY, Mat4::IDENTITY);
        let mesh = api.add_input_mesh(&mut input, 1, 3);
        let glass =
            api.add_input_lit_material(&mut input, 20, Vec4::ONE, Vec3::ZERO, one(), half(), 0);
        // Two translucent draws at the SAME depth → the stable sort keeps order.
        api.add_input_object(&mut input, 200, at_z(-4.0), mesh, glass, true);
        api.add_input_object(&mut input, 300, at_z(-4.0), mesh, glass, true);
        assert_eq!(order(&input), vec![200, 300]);
    }

    #[test]
    fn without_a_camera_translucent_keeps_submission_order() {
        let api = api();
        let mut input = api.new_input(64, 64);
        // No camera → every depth resolves to 0, so the stable sort is a no-op.
        let mesh = api.add_input_mesh(&mut input, 1, 3);
        let glass =
            api.add_input_lit_material(&mut input, 20, Vec4::ONE, Vec3::ZERO, one(), half(), 0);
        api.add_input_object(&mut input, 200, at_z(-2.0), mesh, glass, true);
        api.add_input_object(&mut input, 300, at_z(-5.0), mesh, glass, true);
        assert_eq!(order(&input), vec![200, 300]);
    }

    #[test]
    fn an_all_opaque_scene_keeps_submission_order() {
        let api = api();
        let mut input = api.new_input(64, 64);
        api.set_input_camera(&mut input, Mat4::IDENTITY, Mat4::IDENTITY);
        let mesh = api.add_input_mesh(&mut input, 1, 3);
        let opaque = api.add_input_basic_lit_material(&mut input, 10, Vec4::ONE);
        // Opaque draws at varying depth keep submission order (front-to-back is
        // fine, depth-tested) — the depth key stays 0 for every opaque draw.
        api.add_input_object(&mut input, 100, at_z(-2.0), mesh, opaque, true);
        api.add_input_object(&mut input, 200, at_z(-5.0), mesh, opaque, true);
        assert_eq!(order(&input), vec![100, 200]);
    }

    #[test]
    fn per_object_texture_override_pipeline_and_tag_are_carried() {
        let api = api();
        let mut input = api.new_input(64, 64);
        let mesh = api.add_input_mesh(&mut input, 1, 3);
        // A material carrying its own albedo texture (id 5).
        let mat = api.add_input_textured_material(&mut input, 10, Vec4::ONE, 5);
        // Object A inherits the material's texture (override 0); object B overrides
        // it with texture 9, selects the UNLIT pipeline, and carries tag 3.
        api.add_input_bound_object(
            &mut input,
            100,
            Mat4::IDENTITY,
            mesh,
            mat,
            0,
            RenderApi::PIPELINE_BASIC_LIT,
            0,
            true,
        );
        api.add_input_bound_object(
            &mut input,
            200,
            Mat4::IDENTITY,
            mesh,
            mat,
            9,
            RenderApi::PIPELINE_UNLIT,
            3,
            true,
        );
        let draws = ordered_draws(&input);
        assert_eq!(draws.len(), 2);
        // A: no override → inherits the material texture (5), default pipeline, tag 0.
        assert_eq!(draws[0].texture_id, 5);
        assert_eq!(draws[0].pipeline, RenderApi::PIPELINE_BASIC_LIT);
        assert_eq!(draws[0].object_tag, 0);
        // B: override wins (9), UNLIT pipeline, tag 3.
        assert_eq!(draws[1].texture_id, 9);
        assert_eq!(draws[1].pipeline, RenderApi::PIPELINE_UNLIT);
        assert_eq!(draws[1].object_tag, 3);
    }

    /// **`RenderPipelineKind::UNLIT` is selected by a material for the first
    /// time.** An unlit surface's material puts every draw of it on the unlit
    /// marker whatever the object asked for, and a lit material leaves the
    /// object's own selection exactly as it was — so no existing content moves.
    #[test]
    fn an_unlit_materials_draws_select_the_unlit_pipeline_marker() {
        let api = api();
        let mut input = api.new_input(64, 64);
        let mesh = api.add_input_mesh(&mut input, 1, 3);
        let unlit = input.push_surface_material(
            10,
            Vec4::ONE,
            0xFEED,
            axiom_surface::LightingModel::Unlit,
        );
        let lambert = input.push_surface_material(
            11,
            Vec4::ONE,
            0xBEEF,
            axiom_surface::LightingModel::Lambert,
        );
        let plain = api.add_input_basic_lit_material(&mut input, 12, Vec4::ONE);
        // Every object asks for BASIC_LIT; only the material decides otherwise.
        [unlit, lambert, plain]
            .iter()
            .enumerate()
            .for_each(|(index, material)| {
                api.add_input_bound_object(
                    &mut input,
                    100 + index as u64,
                    Mat4::IDENTITY,
                    mesh,
                    *material,
                    0,
                    RenderApi::PIPELINE_BASIC_LIT,
                    0,
                    true,
                );
            });
        let draws = ordered_draws(&input);
        assert_eq!(draws[0].pipeline, RenderApi::PIPELINE_UNLIT);
        assert_eq!(draws[1].pipeline, RenderApi::PIPELINE_BASIC_LIT);
        assert_eq!(draws[2].pipeline, RenderApi::PIPELINE_BASIC_LIT);
        // And an object that asked for UNLIT under a LIT material keeps its own
        // selection: the derivation adds a reason to be unlit, it removes none.
        let mut kept = api.new_input(64, 64);
        let mesh = api.add_input_mesh(&mut kept, 1, 3);
        let lit = api.add_input_basic_lit_material(&mut kept, 13, Vec4::ONE);
        api.add_input_bound_object(
            &mut kept,
            1,
            Mat4::IDENTITY,
            mesh,
            lit,
            0,
            RenderApi::PIPELINE_UNLIT,
            0,
            true,
        );
        assert_eq!(ordered_draws(&kept)[0].pipeline, RenderApi::PIPELINE_UNLIT);
    }

    #[test]
    fn gates_drop_invisible_and_unresolved_objects() {
        let api = api();
        let mut input = api.new_input(64, 64);
        let mesh = api.add_input_mesh(&mut input, 1, 3);
        let mat = api.add_input_basic_lit_material(&mut input, 10, Vec4::ONE);
        // Invisible, out-of-range mesh, and out-of-range material are all dropped;
        // only the fully-resolved visible object survives.
        api.add_input_object(&mut input, 1, Mat4::IDENTITY, mesh, mat, false);
        api.add_input_object(&mut input, 2, Mat4::IDENTITY, 99, mat, true);
        api.add_input_object(&mut input, 3, Mat4::IDENTITY, mesh, 99, true);
        api.add_input_object(&mut input, 4, Mat4::IDENTITY, mesh, mat, true);
        assert_eq!(order(&input), vec![4]);
    }
}
