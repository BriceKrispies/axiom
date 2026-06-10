//! One render command.

use axiom_math::Mat4;

/// One backend-neutral render command.
///
/// Hidden behind [`crate::RenderApi`]; external callers never name
/// this enum directly. The app inspects a [`crate::RenderCommandList`]
/// through `RenderApi`'s indexed accessors and the `KIND_*` codes it
/// exposes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RenderCommand {
    ClearFrame { color: [f32; 4] },
    SetCamera { view: Mat4, projection: Mat4 },
    SetPipeline { pipeline_id: u32 },
    SetMesh { mesh_id: u64 },
    SetMaterial { material_id: u64 },
    DrawIndexed { index_count: u32, world: Mat4 },
}

impl RenderCommand {
    pub const KIND_CLEAR_FRAME: u32 = 1;
    pub const KIND_SET_CAMERA: u32 = 2;
    pub const KIND_SET_PIPELINE: u32 = 3;
    pub const KIND_SET_MESH: u32 = 4;
    pub const KIND_SET_MATERIAL: u32 = 5;
    pub const KIND_DRAW_INDEXED: u32 = 6;

    pub const fn kind_code(&self) -> u32 {
        match self {
            RenderCommand::ClearFrame { .. } => Self::KIND_CLEAR_FRAME,
            RenderCommand::SetCamera { .. } => Self::KIND_SET_CAMERA,
            RenderCommand::SetPipeline { .. } => Self::KIND_SET_PIPELINE,
            RenderCommand::SetMesh { .. } => Self::KIND_SET_MESH,
            RenderCommand::SetMaterial { .. } => Self::KIND_SET_MATERIAL,
            RenderCommand::DrawIndexed { .. } => Self::KIND_DRAW_INDEXED,
        }
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
            RenderCommand::ClearFrame {
                color: [0.0, 0.0, 0.0, 1.0]
            }
            .kind_code(),
            RenderCommand::KIND_CLEAR_FRAME
        );
        assert_eq!(
            RenderCommand::DrawIndexed {
                index_count: 36,
                world: Mat4::IDENTITY,
            }
            .kind_code(),
            RenderCommand::KIND_DRAW_INDEXED
        );
    }

    #[test]
    fn variants_compare_by_payload() {
        let a = RenderCommand::SetPipeline { pipeline_id: 1 };
        let b = RenderCommand::SetPipeline { pipeline_id: 1 };
        let c = RenderCommand::SetPipeline { pipeline_id: 2 };
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
