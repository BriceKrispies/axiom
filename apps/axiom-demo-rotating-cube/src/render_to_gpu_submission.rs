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
//!
//! [`RenderCommandArtifact`] and [`GpuCommandArtifact`] are **tagged
//! structs**, not data-carrying enums: a `kind` code selects which fields are
//! meaningful, the rest hold a fixed default that is never read for the wrong
//! kind. Construction goes through the const constructors, inspection through
//! the branchless `as_*` accessors — so there is no `match` over the command
//! shape anywhere in this app.

use axiom_math::Mat4;

/// A plain-data mirror of one `axiom_render` render command.
///
/// The numeric kinds match `axiom_render::RenderApi::KIND_*` so the
/// orchestrator can build these directly from the indexed accessors.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderCommandArtifact {
    kind: u32,
    color: [f32; 4],
    view: Mat4,
    projection: Mat4,
    pipeline_id: u32,
    mesh_id: u64,
    material_id: u64,
    index_count: u32,
    world: Mat4,
}

impl RenderCommandArtifact {
    pub const KIND_CLEAR_FRAME: u32 = 1;
    pub const KIND_SET_CAMERA: u32 = 2;
    pub const KIND_SET_PIPELINE: u32 = 3;
    pub const KIND_SET_MESH: u32 = 4;
    pub const KIND_SET_MATERIAL: u32 = 5;
    pub const KIND_DRAW_INDEXED: u32 = 6;

    /// The fixed default every field holds while it is not meaningful for the
    /// command's `kind`. Because each `as_*` accessor is gated on `kind`, a
    /// default field value is never observable through the public API.
    const DEFAULT: Self = RenderCommandArtifact {
        kind: 0,
        color: [0.0; 4],
        view: Mat4::IDENTITY,
        projection: Mat4::IDENTITY,
        pipeline_id: 0,
        mesh_id: 0,
        material_id: 0,
        index_count: 0,
        world: Mat4::IDENTITY,
    };

    /// A `ClearFrame` command carrying its clear `color`.
    pub const fn clear_frame(color: [f32; 4]) -> Self {
        RenderCommandArtifact {
            kind: Self::KIND_CLEAR_FRAME,
            color,
            ..Self::DEFAULT
        }
    }

    /// A `SetCamera` command carrying its `view` and `projection` matrices.
    pub const fn set_camera(view: Mat4, projection: Mat4) -> Self {
        RenderCommandArtifact {
            kind: Self::KIND_SET_CAMERA,
            view,
            projection,
            ..Self::DEFAULT
        }
    }

    /// A `SetPipeline` command carrying its `pipeline_id`.
    pub const fn set_pipeline(pipeline_id: u32) -> Self {
        RenderCommandArtifact {
            kind: Self::KIND_SET_PIPELINE,
            pipeline_id,
            ..Self::DEFAULT
        }
    }

    /// A `SetMesh` command carrying its `mesh_id`.
    pub const fn set_mesh(mesh_id: u64) -> Self {
        RenderCommandArtifact {
            kind: Self::KIND_SET_MESH,
            mesh_id,
            ..Self::DEFAULT
        }
    }

    /// A `SetMaterial` command carrying its `material_id`.
    pub const fn set_material(material_id: u64) -> Self {
        RenderCommandArtifact {
            kind: Self::KIND_SET_MATERIAL,
            material_id,
            ..Self::DEFAULT
        }
    }

    /// A `DrawIndexed` command carrying its `index_count` and `world` matrix.
    pub const fn draw_indexed(index_count: u32, world: Mat4) -> Self {
        RenderCommandArtifact {
            kind: Self::KIND_DRAW_INDEXED,
            index_count,
            world,
            ..Self::DEFAULT
        }
    }

    /// This command's `kind` code (one of the `KIND_*` constants).
    pub const fn kind(&self) -> u32 {
        self.kind
    }

    /// Extract this command's `ClearFrame` `color`, or `None` for any other
    /// kind. Branchless: the `kind` tag gates the field with no `match`.
    pub fn as_clear_frame(&self) -> Option<[f32; 4]> {
        (self.kind == Self::KIND_CLEAR_FRAME).then_some(self.color)
    }

    /// Extract this command's `SetCamera` `(view, projection)`, or `None`.
    pub fn as_set_camera(&self) -> Option<(Mat4, Mat4)> {
        (self.kind == Self::KIND_SET_CAMERA).then_some((self.view, self.projection))
    }

    /// Extract this command's `SetPipeline` id, or `None`.
    pub fn as_set_pipeline(&self) -> Option<u32> {
        (self.kind == Self::KIND_SET_PIPELINE).then_some(self.pipeline_id)
    }

    /// Extract this command's `SetMesh` id, or `None`.
    pub fn as_set_mesh(&self) -> Option<u64> {
        (self.kind == Self::KIND_SET_MESH).then_some(self.mesh_id)
    }

    /// Extract this command's `SetMaterial` id, or `None`.
    pub fn as_set_material(&self) -> Option<u64> {
        (self.kind == Self::KIND_SET_MATERIAL).then_some(self.material_id)
    }

    /// Extract this command's `DrawIndexed` `(index_count, world)`, or `None`.
    pub fn as_draw_indexed(&self) -> Option<(u32, Mat4)> {
        (self.kind == Self::KIND_DRAW_INDEXED).then_some((self.index_count, self.world))
    }
}

/// A plain-data mirror of an `axiom_render::RenderCommandList`.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderCommandListArtifact {
    pub commands: Vec<RenderCommandArtifact>,
}

/// A plain-data mirror of one `axiom_webgpu` GPU command.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GpuCommandArtifact {
    kind: u32,
    color: [f32; 4],
    view: Mat4,
    projection: Mat4,
    pipeline_id: u32,
    mesh_id: u64,
    material_id: u64,
    index_count: u32,
    world: Mat4,
}

impl GpuCommandArtifact {
    pub const KIND_CLEAR_FRAME: u32 = 1;
    pub const KIND_SET_CAMERA: u32 = 2;
    pub const KIND_SET_PIPELINE: u32 = 3;
    pub const KIND_SET_MESH: u32 = 4;
    pub const KIND_SET_MATERIAL: u32 = 5;
    pub const KIND_DRAW_INDEXED: u32 = 6;
    pub const KIND_PRESENT: u32 = 7;

    /// The fixed default every field holds while it is not meaningful for the
    /// command's `kind`. Gated reads make a default value unobservable.
    const DEFAULT: Self = GpuCommandArtifact {
        kind: 0,
        color: [0.0; 4],
        view: Mat4::IDENTITY,
        projection: Mat4::IDENTITY,
        pipeline_id: 0,
        mesh_id: 0,
        material_id: 0,
        index_count: 0,
        world: Mat4::IDENTITY,
    };

    /// A `ClearFrame` command carrying its clear `color`.
    pub const fn clear_frame(color: [f32; 4]) -> Self {
        GpuCommandArtifact {
            kind: Self::KIND_CLEAR_FRAME,
            color,
            ..Self::DEFAULT
        }
    }

    /// A `SetCamera` command carrying its `view` and `projection` matrices.
    pub const fn set_camera(view: Mat4, projection: Mat4) -> Self {
        GpuCommandArtifact {
            kind: Self::KIND_SET_CAMERA,
            view,
            projection,
            ..Self::DEFAULT
        }
    }

    /// A `SetPipeline` command carrying its `pipeline_id`.
    pub const fn set_pipeline(pipeline_id: u32) -> Self {
        GpuCommandArtifact {
            kind: Self::KIND_SET_PIPELINE,
            pipeline_id,
            ..Self::DEFAULT
        }
    }

    /// A `SetMesh` command carrying its `mesh_id`.
    pub const fn set_mesh(mesh_id: u64) -> Self {
        GpuCommandArtifact {
            kind: Self::KIND_SET_MESH,
            mesh_id,
            ..Self::DEFAULT
        }
    }

    /// A `SetMaterial` command carrying its `material_id`.
    pub const fn set_material(material_id: u64) -> Self {
        GpuCommandArtifact {
            kind: Self::KIND_SET_MATERIAL,
            material_id,
            ..Self::DEFAULT
        }
    }

    /// A `DrawIndexed` command carrying its `index_count` and `world` matrix.
    pub const fn draw_indexed(index_count: u32, world: Mat4) -> Self {
        GpuCommandArtifact {
            kind: Self::KIND_DRAW_INDEXED,
            index_count,
            world,
            ..Self::DEFAULT
        }
    }

    /// A `Present` command (no payload).
    pub const fn present() -> Self {
        GpuCommandArtifact {
            kind: Self::KIND_PRESENT,
            ..Self::DEFAULT
        }
    }

    /// This command's `kind` code (one of the `KIND_*` constants).
    pub const fn kind(&self) -> u32 {
        self.kind
    }

    /// Extract this command's `ClearFrame` `color`, or `None` for any other
    /// kind. Branchless: the `kind` tag gates the field with no `match`.
    pub fn as_clear_frame(&self) -> Option<[f32; 4]> {
        (self.kind == Self::KIND_CLEAR_FRAME).then_some(self.color)
    }

    /// Extract this command's `SetCamera` `(view, projection)`, or `None`.
    pub fn as_set_camera(&self) -> Option<(Mat4, Mat4)> {
        (self.kind == Self::KIND_SET_CAMERA).then_some((self.view, self.projection))
    }

    /// Extract this command's `SetPipeline` id, or `None`.
    pub fn as_set_pipeline(&self) -> Option<u32> {
        (self.kind == Self::KIND_SET_PIPELINE).then_some(self.pipeline_id)
    }

    /// Extract this command's `SetMesh` id, or `None`.
    pub fn as_set_mesh(&self) -> Option<u64> {
        (self.kind == Self::KIND_SET_MESH).then_some(self.mesh_id)
    }

    /// Extract this command's `SetMaterial` id, or `None`.
    pub fn as_set_material(&self) -> Option<u64> {
        (self.kind == Self::KIND_SET_MATERIAL).then_some(self.material_id)
    }

    /// Extract this command's `DrawIndexed` `(index_count, world)`, or `None`.
    pub fn as_draw_indexed(&self) -> Option<(u32, Mat4)> {
        (self.kind == Self::KIND_DRAW_INDEXED).then_some((self.index_count, self.world))
    }

    /// `true` when this command is `Present`. Branchless kind read.
    pub fn is_present(&self) -> bool {
        self.kind == Self::KIND_PRESENT
    }
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
///
/// The per-command mapping is a branchless accessor chain: each render kind
/// flows into its GPU counterpart through the gated `as_*`/`map` pair, and
/// exactly one branch yields `Some` for a given command.
pub(crate) fn render_command_list_to_gpu_submission(
    list: &RenderCommandListArtifact,
    target_width: u32,
    target_height: u32,
) -> GpuSubmissionArtifact {
    let commands = list
        .commands
        .iter()
        .map(|command| {
            command
                .as_clear_frame()
                .map(GpuCommandArtifact::clear_frame)
                .or_else(|| {
                    command
                        .as_set_camera()
                        .map(|(view, projection)| GpuCommandArtifact::set_camera(view, projection))
                })
                .or_else(|| command.as_set_pipeline().map(GpuCommandArtifact::set_pipeline))
                .or_else(|| command.as_set_mesh().map(GpuCommandArtifact::set_mesh))
                .or_else(|| {
                    command
                        .as_set_material()
                        .map(GpuCommandArtifact::set_material)
                })
                .or_else(|| {
                    command
                        .as_draw_indexed()
                        .map(|(index_count, world)| {
                            GpuCommandArtifact::draw_indexed(index_count, world)
                        })
                })
                .unwrap_or(GpuCommandArtifact::DEFAULT)
        })
        .chain(std::iter::once(GpuCommandArtifact::present()))
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
                RenderCommandArtifact::clear_frame([0.0, 0.0, 0.0, 1.0]),
                RenderCommandArtifact::set_camera(Mat4::IDENTITY, Mat4::IDENTITY),
                RenderCommandArtifact::set_pipeline(1),
                RenderCommandArtifact::set_mesh(7),
                RenderCommandArtifact::set_material(9),
                RenderCommandArtifact::draw_indexed(36, Mat4::IDENTITY),
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
        assert_eq!(
            *sub.commands.last().expect("submission has commands"),
            GpuCommandArtifact::present()
        );
        let present_count = sub.commands.iter().filter(|c| c.is_present()).count();
        assert_eq!(present_count, 1);
    }

    #[test]
    fn each_render_command_maps_to_its_gpu_counterpart() {
        // The mapping carries every payload through to the matching GPU kind,
        // in order — the branchless accessor chain preserves both kind and data.
        let sub = render_command_list_to_gpu_submission(&cube_command_list(), 800, 600);
        assert_eq!(sub.commands[0].as_clear_frame(), Some([0.0, 0.0, 0.0, 1.0]));
        assert_eq!(
            sub.commands[1].as_set_camera(),
            Some((Mat4::IDENTITY, Mat4::IDENTITY))
        );
        assert_eq!(sub.commands[2].as_set_pipeline(), Some(1));
        assert_eq!(sub.commands[3].as_set_mesh(), Some(7));
        assert_eq!(sub.commands[4].as_set_material(), Some(9));
        assert_eq!(
            sub.commands[5].as_draw_indexed(),
            Some((36, Mat4::IDENTITY))
        );
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
        assert_eq!(sub.commands, vec![GpuCommandArtifact::present()]);
    }
}
