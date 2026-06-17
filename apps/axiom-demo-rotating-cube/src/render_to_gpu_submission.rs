//! App-owned glue: `RenderCommandList → GpuSubmission`.
//!
//! `axiom-render` and `axiom-webgpu` never import one another. The render
//! command list and the GPU submission are not nameable outside their
//! modules, so the orchestrator in [`crate::vertical_slice`] reads the
//! command list through `RenderApi`'s indexed accessors into the plain-data
//! [`RenderCommandListArtifact`] here, and replays the resulting
//! [`GpuSubmissionArtifact`] into `WebGpuApi`.
//!
//! The semantic mapping — which GPU command each render command becomes,
//! and the trailing `Present` — lives in
//! [`render_command_list_to_gpu_submission`] and is unit-testable on plain
//! data.

use axiom_math::Mat4;

/// A plain-data mirror of one `axiom_render` render command.
///
/// The numeric kinds match `axiom_render::RenderApi::KIND_*` so the
/// orchestrator can build these directly from the indexed accessors.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RenderCommandArtifact {
    ClearFrame { color: [f32; 4] },
    SetCamera { view: Mat4, projection: Mat4 },
    SetPipeline { pipeline_id: u32 },
    SetMesh { mesh_id: u64 },
    SetMaterial { material_id: u64 },
    DrawIndexed { index_count: u32, world: Mat4 },
}

/// A plain-data mirror of an `axiom_render::RenderCommandList`.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderCommandListArtifact {
    pub commands: Vec<RenderCommandArtifact>,
}

/// A plain-data mirror of one `axiom_webgpu` GPU command.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GpuCommandArtifact {
    ClearFrame { color: [f32; 4] },
    SetCamera { view: Mat4, projection: Mat4 },
    SetPipeline { pipeline_id: u32 },
    SetMesh { mesh_id: u64 },
    SetMaterial { material_id: u64 },
    DrawIndexed { index_count: u32, world: Mat4 },
    Present,
}

/// A plain-data plan for an `axiom_webgpu::GpuSubmission`. The orchestrator
/// replays this plan into the real `WebGpuApi` to obtain the (un-nameable)
/// `GpuSubmission` and then submits it.
#[derive(Debug, Clone, PartialEq)]
pub struct GpuSubmissionArtifact {
    pub target_width: u32,
    pub target_height: u32,
    pub commands: Vec<GpuCommandArtifact>,
}

/// Translate a render command list into a GPU submission plan: each render
/// command maps to its GPU counterpart, and a single trailing `Present` is
/// appended. Pure: same inputs always produce the same plan.
pub(crate) fn render_command_list_to_gpu_submission(
    list: &RenderCommandListArtifact,
    target_width: u32,
    target_height: u32,
) -> GpuSubmissionArtifact {
    let commands = list
        .commands
        .iter()
        .map(|command| match *command {
            RenderCommandArtifact::ClearFrame { color } => GpuCommandArtifact::ClearFrame { color },
            RenderCommandArtifact::SetCamera { view, projection } => {
                GpuCommandArtifact::SetCamera { view, projection }
            }
            RenderCommandArtifact::SetPipeline { pipeline_id } => {
                GpuCommandArtifact::SetPipeline { pipeline_id }
            }
            RenderCommandArtifact::SetMesh { mesh_id } => GpuCommandArtifact::SetMesh { mesh_id },
            RenderCommandArtifact::SetMaterial { material_id } => {
                GpuCommandArtifact::SetMaterial { material_id }
            }
            RenderCommandArtifact::DrawIndexed { index_count, world } => {
                GpuCommandArtifact::DrawIndexed { index_count, world }
            }
        })
        .chain(std::iter::once(GpuCommandArtifact::Present))
        .collect();
    GpuSubmissionArtifact {
        target_width,
        target_height,
        commands,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cube_command_list() -> RenderCommandListArtifact {
        RenderCommandListArtifact {
            commands: vec![
                RenderCommandArtifact::ClearFrame {
                    color: [0.0, 0.0, 0.0, 1.0],
                },
                RenderCommandArtifact::SetCamera {
                    view: Mat4::IDENTITY,
                    projection: Mat4::IDENTITY,
                },
                RenderCommandArtifact::SetPipeline { pipeline_id: 1 },
                RenderCommandArtifact::SetMesh { mesh_id: 7 },
                RenderCommandArtifact::SetMaterial { material_id: 9 },
                RenderCommandArtifact::DrawIndexed {
                    index_count: 36,
                    world: Mat4::IDENTITY,
                },
            ],
        }
    }

    #[test]
    fn translation_is_pure_and_deterministic() {
        let list = cube_command_list();
        let a = render_command_list_to_gpu_submission(&list, 800, 600);
        let b = render_command_list_to_gpu_submission(&list, 800, 600);
        assert_eq!(a, b);
    }

    #[test]
    fn present_is_appended_once_at_the_end() {
        let sub = render_command_list_to_gpu_submission(&cube_command_list(), 800, 600);
        assert_eq!(sub.commands.len(), 7);
        assert_eq!(*sub.commands.last().unwrap(), GpuCommandArtifact::Present);
        let present_count = sub
            .commands
            .iter()
            .filter(|c| matches!(c, GpuCommandArtifact::Present))
            .count();
        assert_eq!(present_count, 1);
    }

    #[test]
    fn target_dimensions_carry_through() {
        let sub = render_command_list_to_gpu_submission(&cube_command_list(), 640, 480);
        assert_eq!(sub.target_width, 640);
        assert_eq!(sub.target_height, 480);
    }

    #[test]
    fn empty_list_still_presents() {
        let empty = RenderCommandListArtifact { commands: vec![] };
        let sub = render_command_list_to_gpu_submission(&empty, 1, 1);
        assert_eq!(sub.commands, vec![GpuCommandArtifact::Present]);
    }
}
