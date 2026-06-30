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

use crate::render_input::RenderInput;

/// A resolved, ready-to-emit draw: the mesh/material/object identities and world
/// the command builder needs, plus its translucency class and the view-space
/// depth key [`ordered_draws`] sorts by.
pub(crate) struct OrderedDraw {
    pub(crate) mesh_id: u64,
    pub(crate) material_id: u64,
    pub(crate) texture_id: u64,
    pub(crate) object_id: u64,
    pub(crate) index_count: u32,
    pub(crate) world: Mat4,
    translucent: bool,
    depth_key: f32,
}

/// Resolve and order a frame's drawable objects (see the module docs). Each
/// `Option`-combinator carries one gate: a failed gate drops the object.
pub(crate) fn ordered_draws(input: &RenderInput) -> Vec<OrderedDraw> {
    // The camera view orders translucent draws by view-space depth; absent a
    // camera every depth is `0`, so the stable sort leaves submission order.
    let view = input.camera().map(|c| c.view());

    let mut ordered: Vec<OrderedDraw> = input
        .objects()
        .iter()
        .filter_map(|object| {
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
                        texture_id: material.texture_id(),
                        object_id: object.id(),
                        index_count: mesh.indices().len() as u32,
                        world: object.world(),
                        translucent,
                        // Opaque draws carry depth key `0` so the stable sort
                        // keeps them in submission order; translucent draws carry
                        // their camera depth so they sort far→near.
                        depth_key: [0.0, depth][usize::from(translucent)],
                    }
                })
        })
        .collect();

    // Class key (opaque `0` < translucent `1`) groups opaque first; within a
    // class the depth key orders translucent far→near and leaves opaque untouched
    // (all `0`). A stable sort ties-breaks by submission index.
    ordered.sort_by(|a, b| {
        (a.translucent as u8)
            .cmp(&(b.translucent as u8))
            .then_with(|| a.depth_key.total_cmp(&b.depth_key))
    });
    ordered
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
        let mesh = api.add_input_mesh(&mut input, 1, vec![], vec![], vec![], vec![0, 1, 2]);
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
        let mesh = api.add_input_mesh(&mut input, 1, vec![], vec![], vec![], vec![0, 1, 2]);
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
        let mesh = api.add_input_mesh(&mut input, 1, vec![], vec![], vec![], vec![0, 1, 2]);
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
        let mesh = api.add_input_mesh(&mut input, 1, vec![], vec![], vec![], vec![0, 1, 2]);
        let opaque = api.add_input_basic_lit_material(&mut input, 10, Vec4::ONE);
        // Opaque draws at varying depth keep submission order (front-to-back is
        // fine, depth-tested) — the depth key stays 0 for every opaque draw.
        api.add_input_object(&mut input, 100, at_z(-2.0), mesh, opaque, true);
        api.add_input_object(&mut input, 200, at_z(-5.0), mesh, opaque, true);
        assert_eq!(order(&input), vec![100, 200]);
    }

    #[test]
    fn gates_drop_invisible_and_unresolved_objects() {
        let api = api();
        let mut input = api.new_input(64, 64);
        let mesh = api.add_input_mesh(&mut input, 1, vec![], vec![], vec![], vec![0, 1, 2]);
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
