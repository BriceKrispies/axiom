//! The per-tick deterministic output bundle of the demo app.

use axiom_math::Mat4;

/// All deterministic artifacts produced by one demo tick.
///
/// Plain data: scalar counts and matrices, no module-internal types.
/// Tests assert on these values to prove the vertical slice is
/// deterministic and that the boundaries between modules are stable.
#[derive(Debug, Clone, PartialEq)]
pub struct CubeFrame {
    pub tick: u64,
    pub engine_frame_index: u64,
    pub host_frame_sequence: u64,
    pub runtime_step_count: u32,
    /// Number of nodes in the captured scene snapshot.
    pub scene_node_count: u32,
    pub scene_renderable_count: u32,
    /// The full ordered list of render command kind codes (see
    /// `axiom_render::RenderApi::KIND_*`).
    pub render_command_kinds: Vec<u32>,
    pub render_clear_color: [f32; 4],
    pub render_camera_view: Mat4,
    pub render_camera_projection: Mat4,
    pub render_pipeline_id: u32,
    pub render_draw_index_count: u32,
    pub render_draw_world: Mat4,
    /// The full ordered list of GPU command kind codes (see
    /// `axiom_webgpu::WebGpuApi::KIND_*`).
    pub gpu_command_kinds: Vec<u32>,
    pub gpu_clear_count: u32,
    pub gpu_draw_count: u32,
    pub gpu_present_count: u32,
    pub gpu_target_width: u32,
    pub gpu_target_height: u32,
}
