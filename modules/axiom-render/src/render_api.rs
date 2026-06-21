//! The single public facade of the `axiom-render` module.

use axiom_kernel::{FrameIndex, Ratio, Tick};
use axiom_math::{Mat4, Vec2, Vec3, Vec4};

use crate::render_camera::RenderCamera;
use crate::render_command::RenderCommand;
use crate::render_command_list::RenderCommandList;
use crate::render_input::RenderInput;
use crate::render_light::{RenderLight, RenderLightKind};
use crate::render_material::RenderMaterial;
use crate::render_mesh::RenderMesh;
use crate::render_object::RenderObject;
use crate::render_pipeline_kind::RenderPipelineKind;
use crate::render_receipt::RenderReceipt;

/// The only public export of `axiom-render`.
///
/// Owns:
///  - the builder for [`RenderInput`] (camera, lights, meshes,
///    materials, objects),
///  - the conversion from [`RenderInput`] to [`RenderCommandList`],
///  - the indexed inspection of a `RenderCommandList` so the app can
///    translate commands into the WebGPU backend's input without
///    naming any render-internal enum.
///
/// `RenderApi` never imports scene or resources; the app pre-translates.
#[derive(Debug, Clone, Copy, Default)]
pub struct RenderApi {
    _sealed: (),
}

impl RenderApi {
    pub const fn new() -> Self {
        RenderApi { _sealed: () }
    }

    /// Pipeline marker for the only pipeline the vertical slice
    /// supports today.
    pub const PIPELINE_BASIC_LIT: u32 = RenderPipelineKind::BASIC_LIT;

    /// Command kind codes (mirrors [`RenderCommand`]'s internal
    /// discriminants so callers can switch on `u32`).
    pub const KIND_CLEAR_FRAME: u32 = RenderCommand::KIND_CLEAR_FRAME;
    pub const KIND_SET_CAMERA: u32 = RenderCommand::KIND_SET_CAMERA;
    pub const KIND_SET_PIPELINE: u32 = RenderCommand::KIND_SET_PIPELINE;
    pub const KIND_SET_MESH: u32 = RenderCommand::KIND_SET_MESH;
    pub const KIND_SET_MATERIAL: u32 = RenderCommand::KIND_SET_MATERIAL;
    pub const KIND_DRAW_INDEXED: u32 = RenderCommand::KIND_DRAW_INDEXED;

    // --- Input construction ---

    pub fn new_input(&self, viewport_width: u32, viewport_height: u32) -> RenderInput {
        RenderInput::new(viewport_width, viewport_height)
    }

    pub fn set_input_clear_color(&self, input: &mut RenderInput, color: [f32; 4]) {
        input.set_clear_color(color);
    }

    pub fn set_input_camera(&self, input: &mut RenderInput, view: Mat4, projection: Mat4) {
        input.set_camera(RenderCamera::new(view, projection));
    }

    pub fn add_input_directional_light(
        &self,
        input: &mut RenderInput,
        direction_world: Vec3,
        color: Vec3,
        intensity: Ratio,
    ) {
        input.add_light(RenderLight::new(
            RenderLightKind::Directional,
            direction_world,
            color,
            intensity,
        ));
    }

    pub fn add_input_point_light(
        &self,
        input: &mut RenderInput,
        position_world: Vec3,
        color: Vec3,
        intensity: Ratio,
    ) {
        input.add_light(RenderLight::new(
            RenderLightKind::Point,
            position_world,
            color,
            intensity,
        ));
    }

    pub fn add_input_mesh(
        &self,
        input: &mut RenderInput,
        id: u64,
        positions: Vec<Vec3>,
        normals: Vec<Vec3>,
        uvs: Vec<Vec2>,
        indices: Vec<u32>,
    ) -> u32 {
        input.add_mesh(RenderMesh::new(id, positions, normals, uvs, indices))
    }

    pub fn add_input_basic_lit_material(
        &self,
        input: &mut RenderInput,
        id: u64,
        base_color: Vec4,
    ) -> u32 {
        input.add_material(RenderMaterial::new(id, base_color))
    }

    /// Register a basic-lit material that samples albedo texture `texture_id`
    /// (`0` = untextured).
    pub fn add_input_textured_material(
        &self,
        input: &mut RenderInput,
        id: u64,
        base_color: Vec4,
        texture_id: u64,
    ) -> u32 {
        input.add_material(RenderMaterial::new_textured(id, base_color, texture_id))
    }

    pub fn add_input_object(
        &self,
        input: &mut RenderInput,
        world: Mat4,
        mesh_idx: u32,
        material_idx: u32,
        visible: bool,
    ) {
        input.add_object(RenderObject::new(world, mesh_idx, material_idx, visible));
    }

    // --- Compilation ---

    /// Build a deterministic [`RenderCommandList`] from a
    /// [`RenderInput`]. The emitted order is:
    ///
    /// 1. `ClearFrame`
    /// 2. `SetCamera` (only if a camera is present)
    /// 3. `SetPipeline { BASIC_LIT }`
    /// 4. For each visible object in `input.objects()`, in input
    ///    order:
    ///    - `SetMesh`
    ///    - `SetMaterial`
    ///    - `DrawIndexed`
    pub fn build_command_list(&self, input: &RenderInput) -> RenderCommandList {
        let mut list = RenderCommandList::with_capacity(3 + input.objects().len() * 3);
        list.push(RenderCommand::clear_frame(input.clear_color()));
        input.camera().map(|camera| {
            list.push(RenderCommand::set_camera(
                camera.view(),
                camera.projection(),
            ));
        });
        list.push(RenderCommand::set_pipeline(RenderPipelineKind::BASIC_LIT));
        input.objects().iter().for_each(|object| {
            // An object emits commands only when it is visible AND both its
            // mesh and material indices resolve. `Option`-combinators carry
            // each gate: a failed gate yields `None` and pushes nothing.
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
                    list.push(RenderCommand::set_mesh(mesh.id()));
                    list.push(RenderCommand::set_material(
                        material.id(),
                        material.texture_id(),
                    ));
                    list.push(RenderCommand::draw_indexed(
                        mesh.indices().len() as u32,
                        object.world(),
                    ));
                });
        });
        list
    }

    // --- Command list inspection (boundary primitives only) ---

    pub fn command_count(&self, list: &RenderCommandList) -> usize {
        list.len()
    }

    pub fn command_kind_at(&self, list: &RenderCommandList, idx: usize) -> Option<u32> {
        list.at(idx).map(RenderCommand::kind_code)
    }

    pub fn command_clear_color_at(&self, list: &RenderCommandList, idx: usize) -> Option<[f32; 4]> {
        list.at(idx).and_then(RenderCommand::as_clear_color)
    }

    pub fn command_camera_at(&self, list: &RenderCommandList, idx: usize) -> Option<(Mat4, Mat4)> {
        list.at(idx).and_then(RenderCommand::as_camera)
    }

    pub fn command_pipeline_at(&self, list: &RenderCommandList, idx: usize) -> Option<u32> {
        list.at(idx).and_then(RenderCommand::as_pipeline)
    }

    pub fn command_mesh_id_at(&self, list: &RenderCommandList, idx: usize) -> Option<u64> {
        list.at(idx).and_then(RenderCommand::as_mesh_id)
    }

    pub fn command_material_id_at(&self, list: &RenderCommandList, idx: usize) -> Option<u64> {
        list.at(idx).and_then(RenderCommand::as_material_id)
    }

    /// The albedo texture id bound by the `SetMaterial` command at `idx` (`0` =
    /// untextured), or `None` for any other kind. Lets the app thread the
    /// material→texture binding into the backend's submission.
    pub fn command_material_texture_id_at(
        &self,
        list: &RenderCommandList,
        idx: usize,
    ) -> Option<u64> {
        list.at(idx).and_then(RenderCommand::as_material_texture_id)
    }

    pub fn command_draw_indexed_at(
        &self,
        list: &RenderCommandList,
        idx: usize,
    ) -> Option<(u32, Mat4)> {
        list.at(idx).and_then(RenderCommand::as_draw_indexed)
    }

    // --- Frame capture (engine-owned artifact; NOT pixel capture) ---

    /// Capture a deterministic [`RenderReceipt`] for one frame: the frame
    /// identity ([`FrameIndex`] + [`Tick`]) plus the ordered command list,
    /// serialized to a stable byte form. This captures the engine's render
    /// contract *before* platform presentation — no pixels, no GPU readback,
    /// no screenshot. See `render_receipt.rs`.
    pub fn capture_receipt(
        &self,
        frame_index: FrameIndex,
        tick: Tick,
        list: &RenderCommandList,
    ) -> RenderReceipt {
        RenderReceipt::capture(frame_index, tick, list)
    }

    /// The receipt's deterministic serialized bytes (for byte comparison).
    pub fn receipt_bytes<'a>(&self, receipt: &'a RenderReceipt) -> &'a [u8] {
        receipt.bytes()
    }

    /// The receipt's deterministic FNV-1a hash (for cheap comparison).
    pub fn receipt_hash(&self, receipt: &RenderReceipt) -> u64 {
        receipt.hash()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn api() -> RenderApi {
        RenderApi::new()
    }

    fn cube_input() -> RenderInput {
        let mut input = api().new_input(800, 600);
        api().set_input_clear_color(&mut input, [0.1, 0.2, 0.3, 1.0]);
        api().set_input_camera(&mut input, Mat4::IDENTITY, Mat4::IDENTITY);
        api().add_input_directional_light(
            &mut input,
            Vec3::new(0.0, -1.0, 0.0),
            Vec3::ONE,
            Ratio::new(1.0).unwrap(),
        );
        let mesh_idx = api().add_input_mesh(
            &mut input,
            42,
            vec![Vec3::ZERO; 24],
            vec![Vec3::UNIT_Y; 24],
            vec![Vec2::ZERO; 24],
            (0..36).collect(),
        );
        let mat_idx =
            api().add_input_basic_lit_material(&mut input, 99, Vec4::new(0.5, 0.5, 0.5, 1.0));
        api().add_input_object(&mut input, Mat4::IDENTITY, mesh_idx, mat_idx, true);
        input
    }

    #[test]
    fn new_and_default_facades_are_equivalent() {
        // Both construction paths build the same command list from equal input.
        let n = RenderApi::new();
        let d = RenderApi::default();
        assert_eq!(
            n.build_command_list(&n.new_input(100, 100)).len(),
            d.build_command_list(&d.new_input(100, 100)).len(),
        );
    }

    #[test]
    fn empty_input_produces_minimum_command_list() {
        let input = api().new_input(100, 100);
        let list = api().build_command_list(&input);
        // ClearFrame + SetPipeline (no camera).
        assert_eq!(list.len(), 2);
        assert_eq!(
            api().command_kind_at(&list, 0),
            Some(RenderApi::KIND_CLEAR_FRAME)
        );
        assert_eq!(
            api().command_kind_at(&list, 1),
            Some(RenderApi::KIND_SET_PIPELINE)
        );
    }

    #[test]
    fn cube_input_produces_six_commands() {
        let input = cube_input();
        let list = api().build_command_list(&input);
        // ClearFrame + SetCamera + SetPipeline + SetMesh + SetMaterial + DrawIndexed
        assert_eq!(list.len(), 6);
        assert_eq!(
            api().command_kind_at(&list, 0),
            Some(RenderApi::KIND_CLEAR_FRAME)
        );
        assert_eq!(
            api().command_kind_at(&list, 1),
            Some(RenderApi::KIND_SET_CAMERA)
        );
        assert_eq!(
            api().command_kind_at(&list, 2),
            Some(RenderApi::KIND_SET_PIPELINE)
        );
        assert_eq!(
            api().command_kind_at(&list, 3),
            Some(RenderApi::KIND_SET_MESH)
        );
        assert_eq!(
            api().command_kind_at(&list, 4),
            Some(RenderApi::KIND_SET_MATERIAL)
        );
        assert_eq!(
            api().command_kind_at(&list, 5),
            Some(RenderApi::KIND_DRAW_INDEXED)
        );
    }

    #[test]
    fn build_command_list_is_deterministic() {
        let a = api().build_command_list(&cube_input());
        let b = api().build_command_list(&cube_input());
        assert_eq!(a, b);
    }

    #[test]
    fn inspection_accessors_extract_command_payload() {
        let list = api().build_command_list(&cube_input());
        assert_eq!(
            api().command_clear_color_at(&list, 0),
            Some([0.1, 0.2, 0.3, 1.0])
        );
        assert_eq!(
            api().command_camera_at(&list, 1),
            Some((Mat4::IDENTITY, Mat4::IDENTITY))
        );
        assert_eq!(
            api().command_pipeline_at(&list, 2),
            Some(RenderApi::PIPELINE_BASIC_LIT)
        );
        assert_eq!(api().command_mesh_id_at(&list, 3), Some(42));
        assert_eq!(api().command_material_id_at(&list, 4), Some(99));
        let (count, world) = api().command_draw_indexed_at(&list, 5).unwrap();
        assert_eq!(count, 36);
        assert_eq!(world, Mat4::IDENTITY);
    }

    #[test]
    fn invisible_objects_are_skipped() {
        let mut input = api().new_input(100, 100);
        let mesh_idx = api().add_input_mesh(&mut input, 1, vec![], vec![], vec![], vec![0, 1, 2]);
        let mat_idx = api().add_input_basic_lit_material(&mut input, 1, Vec4::ONE);
        api().add_input_object(&mut input, Mat4::IDENTITY, mesh_idx, mat_idx, false);
        let list = api().build_command_list(&input);
        // ClearFrame + SetPipeline only.
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn build_input_round_trips_lights() {
        let mut input = api().new_input(100, 100);
        api().add_input_directional_light(
            &mut input,
            Vec3::new(0.0, -1.0, 0.0),
            Vec3::ONE,
            Ratio::new(1.0).unwrap(),
        );
        api().add_input_point_light(&mut input, Vec3::ZERO, Vec3::ONE, Ratio::new(0.5).unwrap());
        assert_eq!(input.lights().len(), 2);
    }

    #[test]
    fn out_of_range_object_indices_are_skipped() {
        let mut input = api().new_input(100, 100);
        api().add_input_object(&mut input, Mat4::IDENTITY, 99, 99, true);
        let list = api().build_command_list(&input);
        // ClearFrame + SetPipeline only.
        assert_eq!(list.len(), 2);
    }
}

#[cfg(test)]
mod cov {
    use super::*;

    fn api() -> RenderApi {
        RenderApi::new()
    }

    #[test]
    fn out_of_range_material_index_is_skipped() {
        // Valid mesh idx but out-of-range material idx exercises the
        // material `None => continue` arm specifically.
        let mut input = api().new_input(100, 100);
        let mesh_idx = api().add_input_mesh(&mut input, 1, vec![], vec![], vec![], vec![0, 1, 2]);
        api().add_input_object(&mut input, Mat4::IDENTITY, mesh_idx, 99, true);
        let list = api().build_command_list(&input);
        // ClearFrame + SetPipeline only; the object was dropped at material lookup.
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn command_count_matches_list_len() {
        let api = api();
        let empty = api.new_input(10, 10);
        let list = api.build_command_list(&empty);
        assert_eq!(api.command_count(&list), list.len());
        assert_eq!(api.command_count(&list), 2);
    }

    #[test]
    fn inspection_accessors_return_none_on_kind_mismatch() {
        let api = api();
        // A minimal list: index 0 is ClearFrame, index 1 is SetPipeline.
        let list = api.build_command_list(&api.new_input(10, 10));

        // Each typed accessor against a command of a different kind hits its
        // `_ => None` arm.
        assert_eq!(api.command_clear_color_at(&list, 1), None);
        assert_eq!(api.command_camera_at(&list, 0), None);
        assert_eq!(api.command_pipeline_at(&list, 0), None);
        assert_eq!(api.command_mesh_id_at(&list, 0), None);
        assert_eq!(api.command_material_id_at(&list, 0), None);
        assert_eq!(api.command_material_texture_id_at(&list, 0), None);
        assert_eq!(api.command_draw_indexed_at(&list, 0), None);
    }

    #[test]
    fn textured_material_threads_its_texture_into_the_set_material_command() {
        let api = api();
        let mut input = api.new_input(100, 100);
        let mesh_idx = api.add_input_mesh(&mut input, 1, vec![], vec![], vec![], vec![0, 1, 2]);
        // texture id 77 is carried on the material and surfaced on its command.
        let mat_idx = api.add_input_textured_material(&mut input, 5, Vec4::ONE, 77);
        api.add_input_object(&mut input, Mat4::IDENTITY, mesh_idx, mat_idx, true);
        let list = api.build_command_list(&input);
        // ClearFrame + SetPipeline + SetMesh + SetMaterial + DrawIndexed.
        assert_eq!(api.command_material_id_at(&list, 3), Some(5));
        assert_eq!(api.command_material_texture_id_at(&list, 3), Some(77));
    }
}
