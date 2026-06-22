//! One render command.

use axiom_math::Mat4;

/// One backend-neutral render command.
///
/// Hidden behind [`crate::RenderApi`]; external callers never name
/// this type directly. The app inspects a [`crate::RenderCommandList`]
/// through `RenderApi`'s indexed accessors and the `KIND_*` codes it
/// exposes.
///
/// This is a **tagged struct**, not a data-carrying enum: `kind` selects
/// which fields are meaningful, and the rest hold a fixed default that is
/// never read for the wrong kind. Construction goes through the const
/// constructors (`clear_frame`, `set_camera`, …), and inspection through the
/// branchless `as_*` accessors — so there is no `match` over the command
/// shape anywhere in the module.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderCommand {
    kind: u32,
    color: [f32; 4],
    view: Mat4,
    projection: Mat4,
    pipeline_id: u32,
    mesh_id: u64,
    material_id: u64,
    material_texture_id: u64,
    object_id: u64,
    index_count: u32,
    world: Mat4,
}

impl RenderCommand {
    pub const KIND_CLEAR_FRAME: u32 = 1;
    pub const KIND_SET_CAMERA: u32 = 2;
    pub const KIND_SET_PIPELINE: u32 = 3;
    pub const KIND_SET_MESH: u32 = 4;
    pub const KIND_SET_MATERIAL: u32 = 5;
    pub const KIND_DRAW_INDEXED: u32 = 6;

    /// The fixed default every field holds while it is not meaningful for the
    /// command's `kind`. Chosen once and used consistently; because each `as_*`
    /// accessor is gated on `kind`, a default field value is never observable
    /// through the public API.
    const DEFAULT: Self = RenderCommand {
        kind: 0,
        color: [0.0; 4],
        view: Mat4::IDENTITY,
        projection: Mat4::IDENTITY,
        pipeline_id: 0,
        mesh_id: 0,
        material_id: 0,
        material_texture_id: 0,
        object_id: 0,
        index_count: 0,
        world: Mat4::IDENTITY,
    };

    /// A `ClearFrame` command carrying its clear `color`.
    pub const fn clear_frame(color: [f32; 4]) -> Self {
        RenderCommand {
            kind: Self::KIND_CLEAR_FRAME,
            color,
            ..Self::DEFAULT
        }
    }

    /// A `SetCamera` command carrying its `view` and `projection` matrices.
    pub const fn set_camera(view: Mat4, projection: Mat4) -> Self {
        RenderCommand {
            kind: Self::KIND_SET_CAMERA,
            view,
            projection,
            ..Self::DEFAULT
        }
    }

    /// A `SetPipeline` command carrying its `pipeline_id`.
    pub const fn set_pipeline(pipeline_id: u32) -> Self {
        RenderCommand {
            kind: Self::KIND_SET_PIPELINE,
            pipeline_id,
            ..Self::DEFAULT
        }
    }

    /// A `SetMesh` command carrying its `mesh_id`.
    pub const fn set_mesh(mesh_id: u64) -> Self {
        RenderCommand {
            kind: Self::KIND_SET_MESH,
            mesh_id,
            ..Self::DEFAULT
        }
    }

    /// A `SetMaterial` command carrying its `material_id` and the albedo
    /// `material_texture_id` it samples (`0` = untextured).
    pub const fn set_material(material_id: u64, material_texture_id: u64) -> Self {
        RenderCommand {
            kind: Self::KIND_SET_MATERIAL,
            material_id,
            material_texture_id,
            ..Self::DEFAULT
        }
    }

    /// A `DrawIndexed` command carrying its drawn object's `object_id`, its
    /// `index_count`, and its `world` matrix. The id rides on the command so a
    /// backend-neutral frame packet can preserve object identity from the
    /// command list alone.
    pub const fn draw_indexed(object_id: u64, index_count: u32, world: Mat4) -> Self {
        RenderCommand {
            kind: Self::KIND_DRAW_INDEXED,
            object_id,
            index_count,
            world,
            ..Self::DEFAULT
        }
    }

    pub const fn kind_code(&self) -> u32 {
        self.kind
    }

    /// Extract this command's `ClearFrame` payload, or `None` for any other
    /// kind. Branchless: the `kind` tag gates the field with no `match`.
    pub fn as_clear_color(&self) -> Option<[f32; 4]> {
        (self.kind == Self::KIND_CLEAR_FRAME).then_some(self.color)
    }

    /// Extract this command's `SetCamera` `(view, projection)`, or `None`.
    pub fn as_camera(&self) -> Option<(Mat4, Mat4)> {
        (self.kind == Self::KIND_SET_CAMERA).then_some((self.view, self.projection))
    }

    /// Extract this command's `SetPipeline` id, or `None`.
    pub fn as_pipeline(&self) -> Option<u32> {
        (self.kind == Self::KIND_SET_PIPELINE).then_some(self.pipeline_id)
    }

    /// Extract this command's `SetMesh` id, or `None`.
    pub fn as_mesh_id(&self) -> Option<u64> {
        (self.kind == Self::KIND_SET_MESH).then_some(self.mesh_id)
    }

    /// Extract this command's `SetMaterial` id, or `None`.
    pub fn as_material_id(&self) -> Option<u64> {
        (self.kind == Self::KIND_SET_MATERIAL).then_some(self.material_id)
    }

    /// Extract this command's `SetMaterial` albedo texture id (`0` =
    /// untextured), or `None` for any other kind.
    pub fn as_material_texture_id(&self) -> Option<u64> {
        (self.kind == Self::KIND_SET_MATERIAL).then_some(self.material_texture_id)
    }

    /// Extract this command's `DrawIndexed` `(index_count, world)`, or `None`.
    pub fn as_draw_indexed(&self) -> Option<(u32, Mat4)> {
        (self.kind == Self::KIND_DRAW_INDEXED).then_some((self.index_count, self.world))
    }

    /// Extract this command's `DrawIndexed` drawn-object id, or `None` for any
    /// other kind. Kept separate from [`Self::as_draw_indexed`] so existing
    /// callers that only need `(index_count, world)` are unaffected.
    pub fn as_draw_object_id(&self) -> Option<u64> {
        (self.kind == Self::KIND_DRAW_INDEXED).then_some(self.object_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_codes_are_stable() {
        assert_eq!(RenderCommand::KIND_CLEAR_FRAME, 1);
        assert_eq!(RenderCommand::KIND_DRAW_INDEXED, 6);
    }

    #[test]
    fn kind_code_matches_variant() {
        assert_eq!(
            RenderCommand::clear_frame([0.0, 0.0, 0.0, 1.0]).kind_code(),
            RenderCommand::KIND_CLEAR_FRAME
        );
        assert_eq!(
            RenderCommand::draw_indexed(7, 36, Mat4::IDENTITY).kind_code(),
            RenderCommand::KIND_DRAW_INDEXED
        );
    }

    #[test]
    fn variants_compare_by_payload() {
        let a = RenderCommand::set_pipeline(1);
        let b = RenderCommand::set_pipeline(1);
        let c = RenderCommand::set_pipeline(2);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn every_constructor_round_trips_through_its_accessor() {
        // New constructors: each carries its payload back out through the
        // matching accessor, and reports None through every other accessor
        // (the branchless kind gate). Covers the Some arm of each accessor for
        // the constructors not otherwise exercised by the facade tests.
        let camera = RenderCommand::set_camera(Mat4::ZERO, Mat4::IDENTITY);
        assert_eq!(camera.as_camera(), Some((Mat4::ZERO, Mat4::IDENTITY)));
        assert_eq!(camera.as_clear_color(), None);

        let mesh = RenderCommand::set_mesh(7);
        assert_eq!(mesh.as_mesh_id(), Some(7));
        assert_eq!(mesh.as_pipeline(), None);

        let material = RenderCommand::set_material(9, 4);
        assert_eq!(material.as_material_id(), Some(9));
        assert_eq!(material.as_material_texture_id(), Some(4));
        assert_eq!(material.as_mesh_id(), None);
        // The texture accessor is gated on the SetMaterial kind.
        assert_eq!(RenderCommand::set_mesh(7).as_material_texture_id(), None);

        let draw = RenderCommand::draw_indexed(13, 36, Mat4::IDENTITY);
        assert_eq!(draw.as_draw_indexed(), Some((36, Mat4::IDENTITY)));
        assert_eq!(draw.as_draw_object_id(), Some(13));
        assert_eq!(draw.as_material_id(), None);
        // The object-id accessor is gated on the DrawIndexed kind.
        assert_eq!(RenderCommand::set_mesh(7).as_draw_object_id(), None);
    }
}
