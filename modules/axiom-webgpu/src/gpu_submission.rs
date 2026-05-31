//! A mutable, ordered sequence of [`crate::GpuCommand`]s.

use crate::gpu_command::GpuCommand;

/// A mutable, ordered sequence of GPU submission commands the app
/// builds before calling [`crate::WebGpuApi::submit`].
#[derive(Debug, Clone, PartialEq)]
pub struct GpuSubmission {
    commands: Vec<GpuCommand>,
    target_width: u32,
    target_height: u32,
}

impl GpuSubmission {
    pub fn new(target_width: u32, target_height: u32) -> Self {
        GpuSubmission {
            commands: Vec::new(),
            target_width,
            target_height,
        }
    }

    pub(crate) fn push(&mut self, command: GpuCommand) {
        self.commands.push(command);
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
        s.push(GpuCommand::Present);
        // Distinguishes `is_empty -> true`: a submission with a command is NOT empty.
        assert!(!s.is_empty());
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn push_records_commands_in_order() {
        let mut s = GpuSubmission::new(1, 1);
        s.push(GpuCommand::ClearFrame {
            color: [0.0, 0.0, 0.0, 1.0],
        });
        s.push(GpuCommand::Present);
        assert_eq!(s.len(), 2);
        assert_eq!(
            s.commands()[0].kind_code(),
            GpuCommand::KIND_CLEAR_FRAME
        );
        assert_eq!(s.commands()[1].kind_code(), GpuCommand::KIND_PRESENT);
    }
}
