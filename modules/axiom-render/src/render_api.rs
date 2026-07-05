//! The single public facade of the `axiom-render` module.

use axiom_host::{
    FrameCamera, FrameDrawItem, FrameFeatureSet, FrameLight, FramePacket, FrameViewport,
    SdfPrimitive, SdfScene,
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
use crate::render_sdf::RenderSdf;

/// The only public export of `axiom-render`.
/// Owns:
///  - the builder for [`RenderInput`] (camera, lights, meshes,
///    materials, objects),
///  - the conversion from [`RenderInput`] to [`RenderCommandList`],
///  - the indexed inspection of a `RenderCommandList` so the app can
///    translate commands into the WebGPU backend's input without
///    naming any render-internal enum.
/// `RenderApi` never imports scene or resources; the app pre-translates.
#[derive(Debug, Clone, Copy, Default)]
pub struct RenderApi {
    _sealed: (),
}

impl RenderApi {
    pub const fn new() -> Self {
        RenderApi { _sealed: () }
    }

    /// Pipeline marker for the default basic-lit forward pipeline.
    pub const PIPELINE_BASIC_LIT: u32 = RenderPipelineKind::BASIC_LIT;

    /// Command kind codes (mirrors [`RenderCommand`]'s internal
    /// discriminants so callers can switch on `u32`).
    pub const KIND_CLEAR_FRAME: u32 = RenderCommand::KIND_CLEAR_FRAME;
    pub const KIND_SET_CAMERA: u32 = RenderCommand::KIND_SET_CAMERA;
    pub const KIND_SET_PIPELINE: u32 = RenderCommand::KIND_SET_PIPELINE;
    pub const KIND_SET_MESH: u32 = RenderCommand::KIND_SET_MESH;
    pub const KIND_SET_MATERIAL: u32 = RenderCommand::KIND_SET_MATERIAL;
    pub const KIND_DRAW_INDEXED: u32 = RenderCommand::KIND_DRAW_INDEXED;


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

    /// Register a fully-specified lit material: `base_color`, `emissive`
    /// self-illumination, `roughness`, `opacity` (`1` opaque — folded into the
    /// per-draw alpha so a translucent material blends), and an albedo
    /// `texture_id` (`0` = untextured). This is the render-layer authoring
    /// surface for the SPEC-11 material catalog, including the **opacity** the
    /// umbrella `Material` carries but whose asset-registration boundary does not
    /// yet thread to the renderer.
    #[allow(clippy::too_many_arguments)]
    pub fn add_input_lit_material(
        &self,
        input: &mut RenderInput,
        id: u64,
        base_color: Vec4,
        emissive: Vec3,
        roughness: Ratio,
        opacity: Ratio,
        texture_id: u64,
    ) -> u32 {
        input.add_material(RenderMaterial::new_lit(
            id, base_color, emissive, roughness, opacity, texture_id,
        ))
    }

    /// Add an object to draw with the default basic-lit pipeline and no
    /// per-object texture override or tag. `id` is a stable, caller-supplied
    /// identity (e.g. a scene node id) that rides through to the object's
    /// `DrawIndexed` command and into the backend-neutral frame packet, so a
    /// backend can preserve object identity for picking/hit-testing.
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

    /// Add a raymarched SDF shape to `input`: a `kind` discriminant (sphere `0` /
    /// box `1` / plane `2`, matching the backend SDF primitive kinds), the full
    /// `world` transform that places it, its **local** `dims` (sphere radius in
    /// `x`; box half-extents; plane unused), and its linear-RGBA `color`.
    pub fn add_input_sdf(
        &self,
        input: &mut RenderInput,
        kind: u32,
        world: Mat4,
        dims: Vec3,
        color: Vec4,
    ) {
        input.add_sdf_shape(RenderSdf::new(kind, world, dims, color));
    }


    /// Build a deterministic [`RenderCommandList`] from a [`RenderInput`]:
    /// `ClearFrame`, then `SetCamera` (if present), and the drawable objects as
    /// `SetMesh` / `SetMaterial` / `DrawIndexed` — **alpha-ordered** by
    /// [`crate::draw_order`] (opaque first in submission order, then translucent
    /// back-to-front by camera depth; stable, so a tick is reproducible).
    ///
    /// A `SetPipeline` is emitted **run-length**: before a draw whose pipeline
    /// differs from the previous draw's (and before the first draw). The pipeline
    /// id comes from each object (defaulting to
    /// [`RenderPipelineKind::BASIC_LIT`]) — **not** a hardcoded frame-wide pipeline
    /// (audit M1), so the contract carries a genuine per-object selection. A
    /// uniform-pipeline frame emits exactly one `SetPipeline` (its first draw), so
    /// its command count is unchanged from the single-pipeline slice; a frame that
    /// mixes pipelines emits one `SetPipeline` per switch; a drawless frame emits
    /// none.
    pub fn build_command_list(&self, input: &RenderInput) -> RenderCommandList {
        let mut list = RenderCommandList::with_capacity(2 + input.objects().len() * 4);
        list.push(RenderCommand::clear_frame(input.clear_color()));
        input.camera().into_iter().for_each(|camera| {
            list.push(RenderCommand::set_camera(
                camera.view(),
                camera.projection(),
            ));
        });
        let _last_pipeline = crate::draw_order::ordered_draws(input).iter().fold(
            None::<u32>,
            |prev_pipeline, d| {
                (prev_pipeline != Some(d.pipeline))
                    .then(|| list.push(RenderCommand::set_pipeline(d.pipeline)));
                list.push(RenderCommand::set_mesh(d.mesh_id));
                list.push(RenderCommand::set_material(d.material_id, d.texture_id));
                list.push(RenderCommand::draw_indexed(
                    d.object_id,
                    d.object_tag,
                    d.index_count,
                    d.world,
                ));
                Some(d.pipeline)
            },
        );
        list
    }


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


    /// Compile a [`RenderInput`] to a deterministic
    /// [`axiom_host::FramePacket`] — the single backend-neutral artifact the GPU
    /// backend consumes today and the Canvas 2D backend will consume later.
    /// The packet is **derived by walking the [`RenderCommandList`]**, not by
    /// reaching around it into scene/resource state: `ClearFrame` supplies the
    /// clear colour, `SetCamera` the camera (its `view_proj = projection *
    /// view`), `SetMesh`/`SetMaterial` thread the current mesh/material id, and
    /// each `DrawIndexed` emits one [`FrameDrawItem`] carrying its object id, the
    /// threaded mesh/material, its world matrix, `mvp = view_proj * world`, and
    /// the material's resolved base colour. Draw order is exactly the command
    /// list's order.
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

        let packet = FramePacket::new(
            frame_index,
            tick,
            FrameViewport::new(input.viewport_width(), input.viewport_height()),
            input.clear_color(),
            camera,
            draws,
            lights,
            light_view_proj,
            features,
        );
        // Attach the frame's SDF scene, if any (0-or-1 fold over the Option — no
        // branch, no clone; an SDF-less frame returns the packet unchanged). The
        // camera's neutral `projection * view` and world position drive the rays.
        let sdf = input.camera().and_then(|c| {
            let view_proj = c.projection().multiply(c.view());
            let eye = c.view().inverse().unwrap_or(Mat4::IDENTITY).as_cols_array();
            let camera_world_pos = Vec3::new(eye[12], eye[13], eye[14]);
            let shapes: Vec<(u32, Mat4, Vec3, Vec4)> = input
                .sdf_shapes()
                .iter()
                .map(|s| (s.kind(), s.world(), s.dims(), s.color()))
                .collect();
            self.build_sdf_scene(view_proj, camera_world_pos, &shapes)
        });
        sdf.into_iter().fold(packet, |p, scene| p.with_sdf(scene))
    }

    /// Build the backend-neutral [`SdfScene`] for a frame from its camera and SDF
    /// shapes — the single source of truth for SDF-scene assembly, shared by
    /// [`Self::build_frame_packet`] and any composition tier (the render pipeline,
    /// an app) that drives a backend from neutral data.
    /// `view_proj` is the **same** column-major view-projection used to build the
    /// frame's draw MVPs (so SDF depth composites with the meshes); `view_proj` is
    /// inverted to unproject each pixel into a world ray. `camera_world_pos` is the
    /// ray origin. Each shape is `(kind, world, dims, color)`: `world` is inverted
    /// into the backend's world→local matrix and its uniform scale (the length of
    /// the transform's first basis column) is carried in `params[3]` so the backend
    /// rescales the local distance to world units. `None` when `shapes` is empty.
    pub fn build_sdf_scene(
        &self,
        view_proj: Mat4,
        camera_world_pos: Vec3,
        shapes: &[(u32, Mat4, Vec3, Vec4)],
    ) -> Option<SdfScene> {
        (!shapes.is_empty()).then(|| {
            let inv_view_proj = view_proj.inverse().unwrap_or(Mat4::IDENTITY).as_cols_array();
            let primitives = shapes
                .iter()
                .map(|(kind, world, dims, color)| {
                    let cols = world.as_cols_array();
                    let scale = Vec3::new(cols[0], cols[1], cols[2]).length();
                    let inv_transform = world.inverse().unwrap_or(Mat4::IDENTITY).as_cols_array();
                    SdfPrimitive::new(
                        *kind,
                        inv_transform,
                        [dims.x, dims.y, dims.z, scale],
                        [color.x, color.y, color.z, color.w],
                    )
                })
                .collect();
            SdfScene::new(
                primitives,
                view_proj.as_cols_array(),
                inv_view_proj,
                [camera_world_pos.x, camera_world_pos.y, camera_world_pos.z],
                [SDF_MAX_DISTANCE, SDF_HIT_EPSILON, 0.0, 0.0],
            )
        })
    }


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

/// The object-binding channels the render contract carries per object: a fuller
/// object builder, the second pipeline marker, and the per-object tag accessor.
/// A third `impl RenderApi` block so no block exceeds the engine's per-impl item
/// budget (`engine_no_large_impl_blocks`).
impl RenderApi {
    /// Pipeline marker for the unlit/emissive forward pipeline — the second
    /// pipeline a per-object selection can carry.
    pub const PIPELINE_UNLIT: u32 = RenderPipelineKind::UNLIT;

    /// Add a fully-bound object to draw: identity + transform + mesh/material
    /// indices + the per-object binding channels — `texture_id` (a per-object
    /// albedo override; `0` inherits the material's texture), `pipeline` (which
    /// pipeline draws it; use [`Self::PIPELINE_BASIC_LIT`] for the default), and
    /// `tag` (the semantic kind carried from the scene; `0` = untagged). The
    /// resolved texture and pipeline ride through to the object's `SetMaterial` /
    /// `SetPipeline` commands, and the tag onto its `DrawIndexed`, so a backend
    /// can select a pipeline and sample the bound texture from the command list.
    #[allow(clippy::too_many_arguments)]
    pub fn add_input_bound_object(
        &self,
        input: &mut RenderInput,
        id: u64,
        world: Mat4,
        mesh_idx: u32,
        material_idx: u32,
        texture_id: u64,
        pipeline: u32,
        tag: u32,
        visible: bool,
    ) {
        input.add_object(RenderObject::bound(
            id,
            world,
            mesh_idx,
            material_idx,
            texture_id,
            pipeline,
            tag,
            visible,
        ));
    }

    /// The semantic object tag carried by the `DrawIndexed` command at `idx`
    /// (`0` = untagged), or `None` for any other kind. Lets a caller thread the
    /// object's kind (rolled in from the scene `Tag`) from the command list into
    /// a backend submission.
    pub fn command_draw_object_tag_at(&self, list: &RenderCommandList, idx: usize) -> Option<u32> {
        list.at(idx).and_then(RenderCommand::as_draw_object_tag)
    }
}

/// The linear RGBA per-draw colour of the material with `material_id` in `input`,
/// or opaque white when no such material exists. The material's separate
/// `opacity` is **folded into the alpha** (`alpha = base_color.a × opacity`), so
/// the neutral `FrameDrawItem.color` every backend consumes carries the
/// translucency — the GPU's `base.a = albedo.a × instance_color.a` and the
/// Canvas 2D src-over composite both blend without further per-backend plumbing.
fn material_base_color(input: &RenderInput, material_id: u64) -> [f32; 4] {
    input
        .materials()
        .iter()
        .find(|m| m.id() == material_id)
        .map(|m| {
            let c = m.base_color();
            [c.x, c.y, c.z, c.w * m.opacity().get()]
        })
        .unwrap_or([1.0; 4])
}

/// The maximum world-space distance the SDF marcher walks before giving up.
const SDF_MAX_DISTANCE: f32 = 100.0;
/// The surface-hit threshold for the SDF marcher, in world units.
const SDF_HIT_EPSILON: f32 = 0.001;

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
        // ClearFrame only (no camera, no draws → no per-object pipeline command).
        assert_eq!(list.len(), 1);
        assert_eq!(
            api().command_kind_at(&list, 0),
            Some(RenderApi::KIND_CLEAR_FRAME)
        );
        assert_eq!(api().command_kind_at(&list, 1), None);
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
        // ClearFrame only — the invisible object emits no pipeline/mesh/draw.
        assert_eq!(list.len(), 1);
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
        // ClearFrame only — the unresolved object emits nothing.
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn same_pipeline_objects_share_one_set_pipeline_command() {
        // Two objects on the SAME (default) pipeline: run-length emits ONE
        // SetPipeline (before the first draw), so the count matches the original
        // single-pipeline slice — Clear + SetPipeline + 2×(Mesh, Mat, Draw) = 8
        // (no camera). This covers the run-length `prev == Some(pipeline)` arm.
        let mut input = api().new_input(64, 64);
        let mesh = api().add_input_mesh(&mut input, 1, vec![], vec![], vec![], vec![0, 1, 2]);
        let mat = api().add_input_basic_lit_material(&mut input, 1, Vec4::ONE);
        api().add_input_object(&mut input, 10, Mat4::IDENTITY, mesh, mat, true);
        api().add_input_object(&mut input, 20, Mat4::IDENTITY, mesh, mat, true);
        let list = api().build_command_list(&input);
        assert_eq!(list.len(), 8);
        let pipelines = (0..list.len())
            .filter(|i| api().command_kind_at(&list, *i) == Some(RenderApi::KIND_SET_PIPELINE))
            .count();
        assert_eq!(pipelines, 1, "one shared SetPipeline for a uniform frame");
    }

    #[test]
    fn per_object_pipeline_and_tag_thread_through_the_command_list() {
        // Two objects with distinct pipelines and tags: each draw is preceded by
        // its own SetPipeline, and its tag rides on the DrawIndexed command.
        let mut input = api().new_input(64, 64);
        let mesh = api().add_input_mesh(&mut input, 1, vec![], vec![], vec![], vec![0, 1, 2]);
        let mat = api().add_input_basic_lit_material(&mut input, 1, Vec4::ONE);
        api().add_input_bound_object(
            &mut input,
            10,
            Mat4::IDENTITY,
            mesh,
            mat,
            0,
            RenderApi::PIPELINE_BASIC_LIT,
            7,
            true,
        );
        api().add_input_bound_object(
            &mut input,
            20,
            Mat4::IDENTITY,
            mesh,
            mat,
            0,
            RenderApi::PIPELINE_UNLIT,
            9,
            true,
        );
        let list = api().build_command_list(&input);
        // ClearFrame + 2×(SetPipeline, SetMesh, SetMaterial, DrawIndexed) = 9.
        assert_eq!(list.len(), 9);
        assert_eq!(
            api().command_pipeline_at(&list, 1),
            Some(RenderApi::PIPELINE_BASIC_LIT)
        );
        assert_eq!(api().command_draw_object_id_at(&list, 4), Some(10));
        assert_eq!(api().command_draw_object_tag_at(&list, 4), Some(7));
        assert_eq!(
            api().command_pipeline_at(&list, 5),
            Some(RenderApi::PIPELINE_UNLIT)
        );
        assert_eq!(api().command_draw_object_id_at(&list, 8), Some(20));
        assert_eq!(api().command_draw_object_tag_at(&list, 8), Some(9));
        // The tag accessor is gated on the DrawIndexed kind.
        assert_eq!(api().command_draw_object_tag_at(&list, 0), None);
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
        // ClearFrame only; the object was dropped at material lookup.
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn command_count_matches_list_len() {
        let api = api();
        let empty = api.new_input(10, 10);
        let list = api.build_command_list(&empty);
        assert_eq!(api.command_count(&list), list.len());
        assert_eq!(api.command_count(&list), 1);
    }

    #[test]
    fn inspection_accessors_return_none_on_kind_mismatch() {
        use axiom_math::{Mat4, Vec4};
        let api = api();
        // A full list (ClearFrame, SetCamera, SetPipeline, SetMesh, SetMaterial,
        // DrawIndexed): each typed accessor against a command of a different kind
        // hits its `_ => None` arm.
        let mut input = api.new_input(10, 10);
        api.set_input_camera(&mut input, Mat4::IDENTITY, Mat4::IDENTITY);
        let mesh = api.add_input_mesh(&mut input, 1, vec![], vec![], vec![], vec![0, 1, 2]);
        let mat = api.add_input_basic_lit_material(&mut input, 1, Vec4::ONE);
        api.add_input_object(&mut input, 1, Mat4::IDENTITY, mesh, mat, true);
        let list = api.build_command_list(&input);
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
        let mat_idx = api.add_input_textured_material(&mut input, 5, Vec4::ONE, 77);
        api.add_input_object(&mut input, 1, Mat4::IDENTITY, mesh_idx, mat_idx, true);
        let list = api.build_command_list(&input);
        // ClearFrame + SetPipeline + SetMesh + SetMaterial + DrawIndexed.
        assert_eq!(api.command_material_id_at(&list, 3), Some(5));
        assert_eq!(api.command_material_texture_id_at(&list, 3), Some(77));
    }
}

#[cfg(test)]
mod frame_packet_cov;

/// SPEC-11 §3.4 translucency: the material `opacity`→per-draw alpha fold. (The
/// back-to-front translucent draw ordering is tested in `draw_order`.)
#[cfg(test)]
mod translucency_cov {
    use super::*;

    fn api() -> RenderApi {
        RenderApi::new()
    }

    fn half() -> Ratio {
        Ratio::new(0.5).expect("finite")
    }

    fn one() -> Ratio {
        Ratio::new(1.0).expect("finite")
    }

    #[test]
    fn opacity_folds_into_the_per_draw_alpha() {
        let api = api();
        let mut input = api.new_input(64, 64);
        api.set_input_camera(&mut input, Mat4::IDENTITY, Mat4::IDENTITY);
        let mesh = api.add_input_mesh(&mut input, 1, vec![], vec![], vec![], vec![0, 1, 2]);
        // base-colour alpha 1.0 × opacity 0.5 → folded draw alpha 0.5.
        let mat = api.add_input_lit_material(
            &mut input,
            7,
            Vec4::new(0.2, 0.4, 0.6, 1.0),
            Vec3::ZERO,
            one(),
            half(),
            0,
        );
        api.add_input_object(&mut input, 1, Mat4::IDENTITY, mesh, mat, true);
        assert_eq!(material_base_color(&input, 7), [0.2, 0.4, 0.6, 0.5]);
        let packet = api.build_frame_packet(&input, 0, 0, [0.0; 16]);
        assert_eq!(packet.draws()[0].color(), [0.2, 0.4, 0.6, 0.5]);
    }
}
