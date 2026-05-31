//! One backend submission command.

use axiom_math::Mat4;

/// One backend-level submission command.
///
/// Hidden behind [`crate::WebGpuApi`]. Translation from the
/// render layer's `RenderCommand` to this enum lives in the **app**
/// because modules may not import other modules.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GpuCommand {
    /// Clear the swap-chain target colour.
    ClearFrame { color: [f32; 4] },
    /// Bind a pipeline by id.
    SetPipeline { pipeline_id: u32 },
    /// Bind the camera uniforms.
    SetCamera { view: Mat4, projection: Mat4 },
    /// Bind a mesh's vertex/index buffers by opaque id.
    SetMesh { mesh_id: u64 },
    /// Bind a material's uniform group by opaque id.
    SetMaterial { material_id: u64 },
    /// Draw the currently-bound mesh with the supplied world matrix.
    DrawIndexed { index_count: u32, world: Mat4 },
    /// Present the swap-chain target.
    Present,
}

impl GpuCommand {
    pub const KIND_CLEAR_FRAME: u32 = 1;
    pub const KIND_SET_PIPELINE: u32 = 2;
    pub const KIND_SET_CAMERA: u32 = 3;
    pub const KIND_SET_MESH: u32 = 4;
    pub const KIND_SET_MATERIAL: u32 = 5;
    pub const KIND_DRAW_INDEXED: u32 = 6;
    pub const KIND_PRESENT: u32 = 7;

    pub const fn kind_code(&self) -> u32 {
        match self {
            GpuCommand::ClearFrame { .. } => Self::KIND_CLEAR_FRAME,
            GpuCommand::SetPipeline { .. } => Self::KIND_SET_PIPELINE,
            GpuCommand::SetCamera { .. } => Self::KIND_SET_CAMERA,
            GpuCommand::SetMesh { .. } => Self::KIND_SET_MESH,
            GpuCommand::SetMaterial { .. } => Self::KIND_SET_MATERIAL,
            GpuCommand::DrawIndexed { .. } => Self::KIND_DRAW_INDEXED,
            GpuCommand::Present => Self::KIND_PRESENT,
        }
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
            GpuCommand::ClearFrame {
                color: [0.0, 0.0, 0.0, 1.0]
            },
            GpuCommand::Present
        );
    }

    #[test]
    fn kind_code_matches_variant() {
        assert_eq!(GpuCommand::Present.kind_code(), GpuCommand::KIND_PRESENT);
    }
}

#[cfg(test)]
mod cov {
    use super::*;

    #[test]
    fn kind_code_covers_every_variant() {
        assert_eq!(
            GpuCommand::ClearFrame {
                color: [0.0, 0.0, 0.0, 1.0]
            }
            .kind_code(),
            GpuCommand::KIND_CLEAR_FRAME
        );
        assert_eq!(
            GpuCommand::SetPipeline { pipeline_id: 1 }.kind_code(),
            GpuCommand::KIND_SET_PIPELINE
        );
        assert_eq!(
            GpuCommand::SetCamera {
                view: Mat4::IDENTITY,
                projection: Mat4::IDENTITY
            }
            .kind_code(),
            GpuCommand::KIND_SET_CAMERA
        );
        assert_eq!(
            GpuCommand::SetMesh { mesh_id: 5 }.kind_code(),
            GpuCommand::KIND_SET_MESH
        );
        assert_eq!(
            GpuCommand::SetMaterial { material_id: 9 }.kind_code(),
            GpuCommand::KIND_SET_MATERIAL
        );
        assert_eq!(
            GpuCommand::DrawIndexed {
                index_count: 36,
                world: Mat4::IDENTITY
            }
            .kind_code(),
            GpuCommand::KIND_DRAW_INDEXED
        );
    }
}
