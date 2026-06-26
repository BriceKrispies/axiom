//! The single public facade of the `axiom-render` module.

use axiom_host::{
    FrameCamera, FrameDrawItem, FrameFeatureSet, FrameLight, FramePacket, FrameViewport,
};
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

    /// Add an object to draw. `id` is a stable, caller-supplied identity (e.g. a
    /// scene node id) that rides through to the object's `DrawIndexed` command
    /// and into the backend-neutral frame packet, so a backend can preserve
    /// object identity for picking/hit-testing.
    pub fn add_input_object(
        &self,
        input: &mut RenderInput,
        id: u64,
        world: Mat4,
        mesh_idx: u32,
        material_idx: u32,
        visible: bool,
    ) {
        input.add_object(RenderObject::new(
            id,
            world,
            mesh_idx,
            material_idx,
            visible,
        ));
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
        input.camera().into_iter().for_each(|camera| {
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
                .into_iter()
                .for_each(|(object, mesh, material)| {
                    list.push(RenderCommand::set_mesh(mesh.id()));
                    list.push(RenderCommand::set_material(
                        material.id(),
                        material.texture_id(),
                    ));
                    list.push(RenderCommand::draw_indexed(
                        object.id(),
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
}

/// Object-identity inspection, backend-neutral frame-packet derivation, and the
/// engine-owned receipt capture. A second `impl RenderApi` block so neither
/// block exceeds the engine's per-impl item budget (`engine_no_large_impl_blocks`).
impl RenderApi {
    /// The stable object id carried by the `DrawIndexed` command at `idx`, or
    /// `None` for any other kind. Lets a caller thread object identity from the
    /// command list into a backend frame packet.
    pub fn command_draw_object_id_at(&self, list: &RenderCommandList, idx: usize) -> Option<u64> {
        list.at(idx).and_then(RenderCommand::as_draw_object_id)
    }

    // --- Backend-neutral frame packet (derived from the command list) ---

    /// Compile a [`RenderInput`] to a deterministic
    /// [`axiom_host::FramePacket`] — the single backend-neutral artifact the GPU
    /// backend consumes today and the Canvas 2D backend will consume later.
    ///
    /// The packet is **derived by walking the [`RenderCommandList`]**, not by
    /// reaching around it into scene/resource state: `ClearFrame` supplies the
    /// clear colour, `SetCamera` the camera (its `view_proj = projection *
    /// view`), `SetMesh`/`SetMaterial` thread the current mesh/material id, and
    /// each `DrawIndexed` emits one [`FrameDrawItem`] carrying its object id, the
    /// threaded mesh/material, its world matrix, `mvp = view_proj * world`, and
    /// the material's resolved base colour. Draw order is exactly the command
    /// list's order.
    ///
    /// Matrices are backend-neutral (`projection * view * world`, no clip-space
    /// depth remap); applying a backend's depth convention is the consumer's job.
    /// `frame_index`, `tick`, and the directional shadow caster's
    /// `light_view_proj` are supplied by the caller (frame identity and shadow
    /// setup are owned above the render module).
    pub fn build_frame_packet(
        &self,
        input: &RenderInput,
        frame_index: u64,
        tick: u64,
        light_view_proj: [f32; 16],
    ) -> FramePacket {
        let list = self.build_command_list(input);
        let count = list.len();

        // The neutral camera + the view-projection used to fold MVPs. No camera
        // => identity view-projection (so mvp == world) and a `None` camera.
        let view_proj = input
            .camera()
            .map(|c| c.projection().multiply(c.view()))
            .unwrap_or(Mat4::IDENTITY);
        let camera = input.camera().map(|c| {
            FrameCamera::new(
                c.view().as_cols_array(),
                c.projection().as_cols_array(),
                c.projection().multiply(c.view()).as_cols_array(),
            )
        });

        // Walk the command list, threading the current mesh/material id; each
        // draw emits one item. A `fold` carries `(current_mesh, current_material,
        // draws)` branchlessly — a mesh/material command replaces its value, a
        // draw appends, every other command is a no-op.
        let (_, _, draws): (u64, u64, Vec<FrameDrawItem>) = (0..count).fold(
            (0_u64, 0_u64, Vec::new()),
            |(current_mesh, current_material, mut acc), i| {
                let next_mesh = self.command_mesh_id_at(&list, i).unwrap_or(current_mesh);
                let next_material = self
                    .command_material_id_at(&list, i)
                    .unwrap_or(current_material);
                self.command_draw_indexed_at(&list, i)
                    .zip(self.command_draw_object_id_at(&list, i))
                    .into_iter()
                    .for_each(|((_, world), object_id)| {
                        acc.push(FrameDrawItem::new(
                            object_id,
                            next_mesh,
                            next_material,
                            world.as_cols_array(),
                            view_proj.multiply(world).as_cols_array(),
                            material_base_color(input, next_material),
                            // The render-input draw stream carries no contact-shadow
                            // marker (it is gameplay/scene metadata the render layer
                            // is intentionally blind to); the live canvas path that
                            // grounds objects builds its packet from the per-draw
                            // scene data instead. This producer defaults to `false`.
                            false,
                        ));
                    });
                (next_mesh, next_material, acc)
            },
        );

        let lights = input
            .lights()
            .iter()
            .map(|light| {
                let v = light.direction_or_position_world();
                let c = light.color();
                FrameLight::new(
                    u32::from(light.kind() == RenderLightKind::Point),
                    [v.x, v.y, v.z],
                    [c.x, c.y, c.z, light.intensity().get()],
                )
            })
            .collect();

        let directional_lights = input
            .lights()
            .iter()
            .filter(|l| l.kind() == RenderLightKind::Directional)
            .count() as u32;
        let point_lights = input
            .lights()
            .iter()
            .filter(|l| l.kind() == RenderLightKind::Point)
            .count() as u32;
        let features = FrameFeatureSet::new(
            input.materials().iter().any(|m| m.texture_id() != 0),
            directional_lights > 0,
            directional_lights,
            point_lights,
        );

        FramePacket::new(
            frame_index,
            tick,
            FrameViewport::new(input.viewport_width(), input.viewport_height()),
            input.clear_color(),
            camera,
            draws,
            lights,
            light_view_proj,
            features,
        )
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

/// The linear RGBA base colour of the material with `material_id` in `input`,
/// or opaque white when no such material exists. Resolved from the render
/// input's material table the command's `SetMaterial` selected by id — the same
/// fallback the render pipeline uses (`material_color.get(id).unwrap_or([1.0;
/// 4])`).
fn material_base_color(input: &RenderInput, material_id: u64) -> [f32; 4] {
    input
        .materials()
        .iter()
        .find(|m| m.id() == material_id)
        .map(|m| {
            let c = m.base_color();
            [c.x, c.y, c.z, c.w]
        })
        .unwrap_or([1.0; 4])
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
        api().add_input_object(&mut input, 7, Mat4::IDENTITY, mesh_idx, mat_idx, true);
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
        api().add_input_object(&mut input, 1, Mat4::IDENTITY, mesh_idx, mat_idx, false);
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
        api().add_input_object(&mut input, 1, Mat4::IDENTITY, 99, 99, true);
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
        api().add_input_object(&mut input, 1, Mat4::IDENTITY, mesh_idx, 99, true);
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
        api.add_input_object(&mut input, 1, Mat4::IDENTITY, mesh_idx, mat_idx, true);
        let list = api.build_command_list(&input);
        // ClearFrame + SetPipeline + SetMesh + SetMaterial + DrawIndexed.
        assert_eq!(api.command_material_id_at(&list, 3), Some(5));
        assert_eq!(api.command_material_texture_id_at(&list, 3), Some(77));
    }
}

#[cfg(test)]
mod frame_packet_cov {
    use super::*;

    fn api() -> RenderApi {
        RenderApi::new()
    }

    /// A single triangle object with a known mesh/material/colour, plus a camera
    /// and one directional light — exercises every populated packet field.
    fn one_object_input() -> RenderInput {
        let mut input = api().new_input(800, 600);
        api().set_input_clear_color(&mut input, [0.1, 0.2, 0.3, 1.0]);
        api().set_input_camera(&mut input, Mat4::IDENTITY, Mat4::IDENTITY);
        api().add_input_directional_light(
            &mut input,
            Vec3::new(0.0, -1.0, 0.0),
            Vec3::ONE,
            Ratio::new(1.0).unwrap(),
        );
        let mesh = api().add_input_mesh(&mut input, 42, vec![], vec![], vec![], vec![0, 1, 2]);
        let mat = api().add_input_basic_lit_material(&mut input, 99, Vec4::new(0.5, 0.5, 0.5, 1.0));
        api().add_input_object(&mut input, 7, Mat4::IDENTITY, mesh, mat, true);
        input
    }

    #[test]
    fn packet_is_derived_from_the_command_list() {
        let input = one_object_input();
        let packet = api().build_frame_packet(&input, 4, 240, [9.0; 16]);

        // Identity carried through: one draw, object id 7, mesh 42, material 99.
        assert_eq!(packet.draws().len(), 1);
        let draw = packet.draws()[0];
        assert_eq!(draw.object_id(), 7);
        assert_eq!(draw.mesh_id(), 42);
        assert_eq!(draw.material_id(), 99);
        // Identity camera => mvp == world == identity; colour is the material base.
        assert_eq!(draw.world(), Mat4::IDENTITY.as_cols_array());
        assert_eq!(draw.mvp(), Mat4::IDENTITY.as_cols_array());
        assert_eq!(draw.color(), [0.5, 0.5, 0.5, 1.0]);

        // Frame identity + viewport + clear + shadow VP carried verbatim.
        assert_eq!(packet.frame_index(), 4);
        assert_eq!(packet.tick(), 240);
        assert_eq!(packet.viewport(), FrameViewport::new(800, 600));
        assert_eq!(packet.clear_color(), [0.1, 0.2, 0.3, 1.0]);
        assert_eq!(packet.light_view_proj(), [9.0; 16]);

        // Camera present with view_proj = projection * view (identity here).
        let cam = packet.camera().expect("camera present");
        assert_eq!(cam.view(), Mat4::IDENTITY.as_cols_array());
        assert_eq!(cam.projection(), Mat4::IDENTITY.as_cols_array());
        assert_eq!(cam.view_proj(), Mat4::IDENTITY.as_cols_array());

        // One directional light → kind 0; features: no textures, shadows on.
        assert_eq!(packet.lights().len(), 1);
        assert_eq!(packet.lights()[0].kind(), 0);
        let f = packet.features();
        assert!(!f.uses_textures());
        assert!(f.uses_shadows());
        assert_eq!(f.directional_lights(), 1);
        assert_eq!(f.point_lights(), 0);
    }

    #[test]
    fn packet_draw_count_equals_draw_indexed_command_count() {
        let input = one_object_input();
        let list = api().build_command_list(&input);
        let draw_cmds = (0..list.len())
            .filter(|i| api().command_kind_at(&list, *i) == Some(RenderApi::KIND_DRAW_INDEXED))
            .count();
        let packet = api().build_frame_packet(&input, 0, 0, [0.0; 16]);
        assert_eq!(packet.draws().len(), draw_cmds);
        assert_eq!(packet.draws().len(), 1);
    }

    #[test]
    fn packet_object_ids_and_order_match_the_command_list() {
        // Three objects with distinct ids → three draws in input order.
        let mut input = api().new_input(100, 100);
        let mesh = api().add_input_mesh(&mut input, 1, vec![], vec![], vec![], vec![0, 1, 2]);
        let mat = api().add_input_basic_lit_material(&mut input, 1, Vec4::ONE);
        api().add_input_object(&mut input, 100, Mat4::IDENTITY, mesh, mat, true);
        api().add_input_object(&mut input, 200, Mat4::IDENTITY, mesh, mat, true);
        api().add_input_object(&mut input, 300, Mat4::IDENTITY, mesh, mat, true);

        let list = api().build_command_list(&input);
        // Object ids straight off the DrawIndexed commands, in list order.
        let cmd_ids: Vec<u64> = (0..list.len())
            .filter_map(|i| api().command_draw_object_id_at(&list, i))
            .collect();
        assert_eq!(cmd_ids, vec![100, 200, 300]);

        let packet = api().build_frame_packet(&input, 0, 0, [0.0; 16]);
        let packet_ids: Vec<u64> = packet.draws().iter().map(|d| d.object_id()).collect();
        assert_eq!(packet_ids, cmd_ids);
    }

    #[test]
    fn command_draw_object_id_at_is_some_only_on_draws() {
        let input = one_object_input();
        let list = api().build_command_list(&input);
        // Index 0 is ClearFrame → None; the final command is the draw → Some(7).
        assert_eq!(api().command_draw_object_id_at(&list, 0), None);
        assert_eq!(
            api().command_draw_object_id_at(&list, list.len() - 1),
            Some(7)
        );
    }

    #[test]
    fn empty_input_yields_a_cameraless_drawless_packet() {
        let input = api().new_input(320, 240);
        let packet = api().build_frame_packet(&input, 1, 2, [0.0; 16]);
        assert!(packet.camera().is_none());
        assert!(packet.draws().is_empty());
        assert!(packet.lights().is_empty());
        let f = packet.features();
        assert!(!f.uses_textures());
        assert!(!f.uses_shadows());
        assert_eq!(f.directional_lights(), 0);
        assert_eq!(f.point_lights(), 0);
        assert_eq!(packet.viewport(), FrameViewport::new(320, 240));
    }

    #[test]
    fn features_count_both_light_kinds_and_detect_textures() {
        let mut input = api().new_input(100, 100);
        api().add_input_directional_light(
            &mut input,
            Vec3::new(0.0, -1.0, 0.0),
            Vec3::ONE,
            Ratio::new(1.0).unwrap(),
        );
        api().add_input_point_light(&mut input, Vec3::ZERO, Vec3::ONE, Ratio::new(0.5).unwrap());
        let mesh = api().add_input_mesh(&mut input, 1, vec![], vec![], vec![], vec![0, 1, 2]);
        // A textured material flips uses_textures on.
        let mat = api().add_input_textured_material(&mut input, 5, Vec4::ONE, 77);
        api().add_input_object(&mut input, 1, Mat4::IDENTITY, mesh, mat, true);

        let packet = api().build_frame_packet(&input, 0, 0, [0.0; 16]);
        let f = packet.features();
        assert!(f.uses_textures());
        assert!(f.uses_shadows());
        assert_eq!(f.directional_lights(), 1);
        assert_eq!(f.point_lights(), 1);
        // Light kinds map directional→0, point→1, in input order.
        let kinds: Vec<u32> = packet.lights().iter().map(|l| l.kind()).collect();
        assert_eq!(kinds, vec![0, 1]);
        // The point light's colour+intensity ride through unchanged ([r,g,b,i]).
        assert_eq!(packet.lights()[1].color_intensity(), [1.0, 1.0, 1.0, 0.5]);
    }

    #[test]
    fn material_base_color_resolves_by_id_with_white_fallback() {
        let mut input = api().new_input(10, 10);
        api().add_input_basic_lit_material(&mut input, 9, Vec4::new(0.2, 0.4, 0.6, 1.0));
        // A present material id resolves to its base colour…
        assert_eq!(material_base_color(&input, 9), [0.2, 0.4, 0.6, 1.0]);
        // …and an absent id falls back to opaque white.
        assert_eq!(material_base_color(&input, 404), [1.0, 1.0, 1.0, 1.0]);
    }
}
