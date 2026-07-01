//! One backend submission command.

use axiom_math::Mat4;

/// One backend-level submission command.
///
/// Hidden behind [`crate::WebGpuApi`]. Translation from the
/// render layer's `RenderCommand` to this struct lives in the **app**.
///
/// This is a **tagged struct**, not a data-carrying enum: a `kind` code
/// selects which payload fields are meaningful, and every payload field is
/// carried inline. Reading the discriminant or a payload is then a field
/// access (or a `then_some` on a discriminant equality), never a `match` —
/// the per-variant branching is gone. Const constructors stand in for the
/// former enum variants and keep every field invariant the variants had
/// (e.g. only `ClearFrame` carries a colour; `Present` carries nothing).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GpuCommand {
    kind: u32,
    color: [f32; 4],
    pipeline_id: u32,
    view: Mat4,
    projection: Mat4,
    mesh_id: u64,
    material_id: u64,
    material_texture_id: u64,
    index_count: u32,
    world: Mat4,
}

impl GpuCommand {
    pub const KIND_CLEAR_FRAME: u32 = 1;
    pub const KIND_SET_PIPELINE: u32 = 2;
    pub const KIND_SET_CAMERA: u32 = 3;
    pub const KIND_SET_MESH: u32 = 4;
    pub const KIND_SET_MATERIAL: u32 = 5;
    pub const KIND_DRAW_INDEXED: u32 = 6;
    pub const KIND_PRESENT: u32 = 7;

    /// A fully-zeroed payload. Each constructor overwrites only the fields its
    /// kind defines, leaving the rest at this neutral baseline so equality and
    /// determinism match the old fieldless variants exactly.
    const ZEROED: Self = GpuCommand {
        kind: 0,
        color: [0.0; 4],
        pipeline_id: 0,
        view: Mat4::IDENTITY,
        projection: Mat4::IDENTITY,
        mesh_id: 0,
        material_id: 0,
        material_texture_id: 0,
        index_count: 0,
        world: Mat4::IDENTITY,
    };

    /// Clear the swap-chain target colour.
    pub const fn clear_frame(color: [f32; 4]) -> Self {
        GpuCommand {
            kind: Self::KIND_CLEAR_FRAME,
            color,
            ..Self::ZEROED
        }
    }

    /// Bind a pipeline by id.
    pub const fn set_pipeline(pipeline_id: u32) -> Self {
        GpuCommand {
            kind: Self::KIND_SET_PIPELINE,
            pipeline_id,
            ..Self::ZEROED
        }
    }

    /// Bind the camera uniforms.
    pub const fn set_camera(view: Mat4, projection: Mat4) -> Self {
        GpuCommand {
            kind: Self::KIND_SET_CAMERA,
            view,
            projection,
            ..Self::ZEROED
        }
    }

    /// Bind a mesh's vertex/index buffers by opaque id.
    pub const fn set_mesh(mesh_id: u64) -> Self {
        GpuCommand {
            kind: Self::KIND_SET_MESH,
            mesh_id,
            ..Self::ZEROED
        }
    }

    /// Bind a material's uniform group by opaque id, together with the albedo
    /// texture id it samples (`0` = untextured). The recording backend captures
    /// both so a receipt reflects which texture each draw bound.
    pub const fn set_material(material_id: u64, material_texture_id: u64) -> Self {
        GpuCommand {
            kind: Self::KIND_SET_MATERIAL,
            material_id,
            material_texture_id,
            ..Self::ZEROED
        }
    }

    /// Draw the currently-bound mesh with the supplied world matrix.
    pub const fn draw_indexed(index_count: u32, world: Mat4) -> Self {
        GpuCommand {
            kind: Self::KIND_DRAW_INDEXED,
            index_count,
            world,
            ..Self::ZEROED
        }
    }

    /// Present the swap-chain target.
    pub const fn present() -> Self {
        GpuCommand {
            kind: Self::KIND_PRESENT,
            ..Self::ZEROED
        }
    }

    /// The stable kind code for this command — now a field read, not a match.
    pub const fn kind_code(&self) -> u32 {
        self.kind
    }

    /// The clear colour, if this is a `clear_frame` command.
    pub fn as_clear_frame(&self) -> Option<[f32; 4]> {
        (self.kind == Self::KIND_CLEAR_FRAME).then_some(self.color)
    }

    /// The pipeline id, if this is a `set_pipeline` command.
    pub fn as_set_pipeline(&self) -> Option<u32> {
        (self.kind == Self::KIND_SET_PIPELINE).then_some(self.pipeline_id)
    }

    /// The camera `(view, projection)` pair, if this is a `set_camera` command.
    pub fn as_set_camera(&self) -> Option<(Mat4, Mat4)> {
        (self.kind == Self::KIND_SET_CAMERA).then_some((self.view, self.projection))
    }

    /// The mesh id, if this is a `set_mesh` command.
    pub fn as_set_mesh(&self) -> Option<u64> {
        (self.kind == Self::KIND_SET_MESH).then_some(self.mesh_id)
    }

    /// The material id, if this is a `set_material` command.
    pub fn as_set_material(&self) -> Option<u64> {
        (self.kind == Self::KIND_SET_MATERIAL).then_some(self.material_id)
    }

    /// The albedo texture id (`0` = untextured) bound by this `set_material`
    /// command, or `None` for any other kind.
    pub fn as_set_material_texture(&self) -> Option<u64> {
        (self.kind == Self::KIND_SET_MATERIAL).then_some(self.material_texture_id)
    }

    /// The `(index_count, world)` pair, if this is a `draw_indexed` command.
    pub fn as_draw_indexed(&self) -> Option<(u32, Mat4)> {
        (self.kind == Self::KIND_DRAW_INDEXED).then_some((self.index_count, self.world))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_codes_are_stable() {
        assert_eq!(GpuCommand::KIND_CLEAR_FRAME, 1);
        assert_eq!(GpuCommand::KIND_PRESENT, 7);
    }

    #[test]
    fn variants_are_distinct() {
        assert_ne!(
            GpuCommand::clear_frame([0.0, 0.0, 0.0, 1.0]),
            GpuCommand::present()
        );
    }

    #[test]
    fn kind_code_matches_variant() {
        assert_eq!(GpuCommand::present().kind_code(), GpuCommand::KIND_PRESENT);
    }
}

#[cfg(test)]
mod cov {
    use super::*;

    #[test]
    fn kind_code_covers_every_variant() {
        assert_eq!(
            GpuCommand::clear_frame([0.0, 0.0, 0.0, 1.0]).kind_code(),
            GpuCommand::KIND_CLEAR_FRAME
        );
        assert_eq!(
            GpuCommand::set_pipeline(1).kind_code(),
            GpuCommand::KIND_SET_PIPELINE
        );
        assert_eq!(
            GpuCommand::set_camera(Mat4::IDENTITY, Mat4::IDENTITY).kind_code(),
            GpuCommand::KIND_SET_CAMERA
        );
        assert_eq!(
            GpuCommand::set_mesh(5).kind_code(),
            GpuCommand::KIND_SET_MESH
        );
        assert_eq!(
            GpuCommand::set_material(9, 0).kind_code(),
            GpuCommand::KIND_SET_MATERIAL
        );
        assert_eq!(
            GpuCommand::draw_indexed(36, Mat4::IDENTITY).kind_code(),
            GpuCommand::KIND_DRAW_INDEXED
        );
    }

    #[test]
    fn accessors_return_payload_for_matching_kind() {
        assert_eq!(
            GpuCommand::clear_frame([0.1, 0.2, 0.3, 1.0]).as_clear_frame(),
            Some([0.1, 0.2, 0.3, 1.0])
        );
        assert_eq!(GpuCommand::set_pipeline(7).as_set_pipeline(), Some(7));
        assert_eq!(
            GpuCommand::set_camera(Mat4::IDENTITY, Mat4::IDENTITY).as_set_camera(),
            Some((Mat4::IDENTITY, Mat4::IDENTITY))
        );
        assert_eq!(GpuCommand::set_mesh(5).as_set_mesh(), Some(5));
        let material = GpuCommand::set_material(9, 4);
        assert_eq!(material.as_set_material(), Some(9));
        assert_eq!(material.as_set_material_texture(), Some(4));
        assert_eq!(
            GpuCommand::draw_indexed(36, Mat4::IDENTITY).as_draw_indexed(),
            Some((36, Mat4::IDENTITY))
        );
    }

    #[test]
    fn accessors_return_none_for_mismatched_kind() {
        let present = GpuCommand::present();
        assert_eq!(present.as_clear_frame(), None);
        assert_eq!(present.as_set_pipeline(), None);
        assert_eq!(present.as_set_camera(), None);
        assert_eq!(present.as_set_mesh(), None);
        assert_eq!(present.as_set_material(), None);
        assert_eq!(present.as_set_material_texture(), None);
        assert_eq!(present.as_draw_indexed(), None);
    }
}
