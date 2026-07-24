//! A mutable, ordered sequence of [`crate::GpuCommand`]s.

use axiom_math::Mat4;

use crate::gpu_command::GpuCommand;

/// A mutable, ordered sequence of GPU submission commands the app
/// builds before calling [`crate::WebGpuApi::submit`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GpuSubmission {
    commands: Vec<GpuCommand>,
    target_width: u32,
    target_height: u32,
}

impl GpuSubmission {
    pub const fn new(target_width: u32, target_height: u32) -> Self {
        GpuSubmission {
            commands: Vec::new(),
            target_width,
            target_height,
        }
    }

    pub(crate) fn push(&mut self, command: GpuCommand) {
        self.commands.push(command);
    }

    /// Clear the command list (reusing its capacity) and retarget the viewport —
    /// the per-frame reuse entry point. A retained submission is `reset` then
    /// refilled each frame instead of allocated fresh, which is what keeps the
    /// render pipeline from churning wasm linear memory.
    pub fn reset(&mut self, target_width: u32, target_height: u32) {
        self.commands.clear();
        self.target_width = target_width;
        self.target_height = target_height;
    }

    /// Append a clear-frame command (public counterparts of the `pub(crate)`
    /// [`Self::push`], so a composing feature module can fill a retained
    /// submission it holds by reference without naming [`GpuCommand`]).
    pub fn clear_frame(&mut self, color: [f32; 4]) {
        self.push(GpuCommand::clear_frame(color));
    }
    pub fn set_pipeline(&mut self, pipeline_id: u32) {
        self.push(GpuCommand::set_pipeline(pipeline_id));
    }
    pub fn set_camera(&mut self, view: Mat4, projection: Mat4) {
        self.push(GpuCommand::set_camera(view, projection));
    }
    pub fn set_mesh(&mut self, mesh_id: u64) {
        self.push(GpuCommand::set_mesh(mesh_id));
    }
    pub fn set_material(&mut self, material_id: u64, material_texture_id: u64) {
        self.push(GpuCommand::set_material(material_id, material_texture_id));
    }
    pub fn draw_indexed(&mut self, index_count: u32, world: Mat4) {
        self.push(GpuCommand::draw_indexed(index_count, world));
    }
    pub fn present(&mut self) {
        self.push(GpuCommand::present());
    }

    pub fn len(&self) -> usize {
        self.commands.len()
    }

    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    pub fn commands(&self) -> &[GpuCommand] {
        &self.commands
    }

    pub const fn target_width(&self) -> u32 {
        self.target_width
    }

    pub const fn target_height(&self) -> u32 {
        self.target_height
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_submission_is_empty() {
        let s = GpuSubmission::new(800, 600);
        assert!(s.is_empty());
        assert_eq!(s.target_width(), 800);
        assert_eq!(s.target_height(), 600);
    }

    #[test]
    fn populated_submission_is_not_empty() {
        let mut s = GpuSubmission::new(1, 1);
        s.push(GpuCommand::present());
        assert!(!s.is_empty());
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn push_records_commands_in_order() {
        let mut s = GpuSubmission::new(1, 1);
        s.push(GpuCommand::clear_frame([0.0, 0.0, 0.0, 1.0]));
        s.push(GpuCommand::present());
        assert_eq!(s.len(), 2);
        assert_eq!(s.commands()[0].kind_code(), GpuCommand::KIND_CLEAR_FRAME);
        assert_eq!(s.commands()[1].kind_code(), GpuCommand::KIND_PRESENT);
    }

    #[test]
    fn reset_then_the_public_helpers_refill_one_buffer_in_order() {
        let mut s = GpuSubmission::new(1, 1);
        s.present();
        // Reset retargets + clears; the public helpers refill the same buffer.
        s.reset(800, 600);
        assert!(s.is_empty());
        assert_eq!((s.target_width(), s.target_height()), (800, 600));
        s.clear_frame([0.1, 0.2, 0.3, 1.0]);
        s.set_pipeline(7);
        s.set_camera(Mat4::IDENTITY, Mat4::IDENTITY);
        s.set_mesh(11);
        s.set_material(22, 33);
        s.draw_indexed(36, Mat4::IDENTITY);
        s.present();
        let kinds: Vec<u32> = s.commands().iter().map(GpuCommand::kind_code).collect();
        assert_eq!(
            kinds,
            vec![
                GpuCommand::KIND_CLEAR_FRAME,
                GpuCommand::KIND_SET_PIPELINE,
                GpuCommand::KIND_SET_CAMERA,
                GpuCommand::KIND_SET_MESH,
                GpuCommand::KIND_SET_MATERIAL,
                GpuCommand::KIND_DRAW_INDEXED,
                GpuCommand::KIND_PRESENT,
            ]
        );
    }
}
