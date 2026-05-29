//! Deterministic report of one GPU submission.

use crate::gpu_command::GpuCommand;

/// The deterministic record `WebGpuApi::submit` returns.
///
/// Today the backend is a *recorder*: every command the app pushed
/// into the submission is captured in the report, plus a per-kind
/// counter that lets test code assert on submission shape without
/// having to walk the command list. Real GPU presentation will be
/// added when the host layer exposes a surface — see
/// `ARCHITECTURE.md` for the blocker.
#[derive(Debug, Clone, PartialEq)]
pub struct GpuSubmissionReport {
    submitted_commands: Vec<GpuCommand>,
    target_width: u32,
    target_height: u32,
    clear_count: u32,
    draw_count: u32,
    present_count: u32,
}

impl GpuSubmissionReport {
    pub(crate) fn new(
        submitted_commands: Vec<GpuCommand>,
        target_width: u32,
        target_height: u32,
    ) -> Self {
        let mut clear_count = 0u32;
        let mut draw_count = 0u32;
        let mut present_count = 0u32;
        for c in &submitted_commands {
            match c {
                GpuCommand::ClearFrame { .. } => clear_count += 1,
                GpuCommand::DrawIndexed { .. } => draw_count += 1,
                GpuCommand::Present => present_count += 1,
                _ => {}
            }
        }
        GpuSubmissionReport {
            submitted_commands,
            target_width,
            target_height,
            clear_count,
            draw_count,
            present_count,
        }
    }

    pub fn submitted_commands(&self) -> &[GpuCommand] {
        &self.submitted_commands
    }

    pub const fn submitted_command_count(&self) -> usize {
        self.submitted_commands.len()
    }

    pub const fn target_width(&self) -> u32 {
        self.target_width
    }

    pub const fn target_height(&self) -> u32 {
        self.target_height
    }

    pub const fn clear_count(&self) -> u32 {
        self.clear_count
    }

    pub const fn draw_count(&self) -> u32 {
        self.draw_count
    }

    pub const fn present_count(&self) -> u32 {
        self.present_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_records_per_kind_counts() {
        let r = GpuSubmissionReport::new(
            vec![
                GpuCommand::ClearFrame {
                    color: [0.0, 0.0, 0.0, 1.0],
                },
                GpuCommand::DrawIndexed {
                    index_count: 36,
                    world: axiom_math::Mat4::IDENTITY,
                },
                GpuCommand::DrawIndexed {
                    index_count: 6,
                    world: axiom_math::Mat4::IDENTITY,
                },
                GpuCommand::Present,
            ],
            800,
            600,
        );
        assert_eq!(r.clear_count(), 1);
        assert_eq!(r.draw_count(), 2);
        assert_eq!(r.present_count(), 1);
        assert_eq!(r.submitted_command_count(), 4);
    }

    #[test]
    fn report_round_trips_target_dimensions() {
        let r = GpuSubmissionReport::new(vec![], 1920, 1080);
        assert_eq!(r.target_width(), 1920);
        assert_eq!(r.target_height(), 1080);
    }

    #[test]
    fn equal_inputs_produce_equal_reports() {
        let a = GpuSubmissionReport::new(vec![GpuCommand::Present], 1, 1);
        let b = GpuSubmissionReport::new(vec![GpuCommand::Present], 1, 1);
        assert_eq!(a, b);
    }
}
