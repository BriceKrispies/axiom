//! The deterministic result of one engine frame.

/// One drawn object: its wgpu-ready model-view-projection matrix (column-major,
/// 16 floats) and its linear RGBA colour.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrawData {
    mvp: [f32; 16],
    color: [f32; 4],
}

impl DrawData {
    pub(crate) const fn new(mvp: [f32; 16], color: [f32; 4]) -> Self {
        DrawData { mvp, color }
    }

    /// The column-major model-view-projection matrix.
    pub const fn mvp(&self) -> [f32; 16] {
        self.mvp
    }

    /// The linear RGBA colour.
    pub const fn color(&self) -> [f32; 4] {
        self.color
    }
}

/// The deterministic summary of one [`crate::prelude::App`] frame: the tick, the
/// GPU command count, the clear colour, the per-object draw data, and the
/// backend flags. Equal inputs at the same tick produce an equal `FrameOutcome`.
#[derive(Debug, Clone, PartialEq)]
pub struct FrameOutcome {
    tick: u64,
    command_count: usize,
    clear_color: [f32; 4],
    draws: Vec<DrawData>,
    presented: bool,
    recorded: bool,
}

impl FrameOutcome {
    pub(crate) fn new(
        tick: u64,
        command_count: usize,
        clear_color: [f32; 4],
        draws: Vec<DrawData>,
        presented: bool,
        recorded: bool,
    ) -> Self {
        FrameOutcome {
            tick,
            command_count,
            clear_color,
            draws,
            presented,
            recorded,
        }
    }

    /// A simulation-only outcome (rendering disabled): no commands, no draws.
    pub(crate) fn simulation_only(tick: u64, clear_color: [f32; 4]) -> Self {
        FrameOutcome::new(tick, 0, clear_color, Vec::new(), false, false)
    }

    /// The tick this outcome was produced at.
    pub const fn tick(&self) -> u64 {
        self.tick
    }

    /// The number of GPU commands the frame submitted.
    pub const fn command_count(&self) -> usize {
        self.command_count
    }

    /// The frame's clear colour.
    pub const fn clear_color(&self) -> [f32; 4] {
        self.clear_color
    }

    /// The per-object draw data, in submission order.
    pub fn draws(&self) -> &[DrawData] {
        &self.draws
    }

    /// Whether the backend presented real pixels.
    pub const fn presented(&self) -> bool {
        self.presented
    }

    /// Whether a recording backend produced this outcome.
    pub const fn recorded(&self) -> bool {
        self.recorded
    }

    /// Pack the per-object draws into the live backend's instance layout: each
    /// draw contributes its 16 MVP floats followed by its 4 colour floats (20
    /// floats per instance), in submission order. This is the plain data the
    /// windowing run loop presents each frame.
    pub fn instance_floats(&self) -> Vec<f32> {
        let mut out = Vec::with_capacity(self.draws.len() * 20);
        for draw in &self.draws {
            out.extend_from_slice(&draw.mvp);
            out.extend_from_slice(&draw.color);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_floats_pack_mvp_then_colour_per_draw() {
        let outcome = FrameOutcome::new(
            0,
            0,
            [0.0; 4],
            vec![
                DrawData::new([1.0; 16], [0.1, 0.2, 0.3, 1.0]),
                DrawData::new([2.0; 16], [0.4, 0.5, 0.6, 1.0]),
            ],
            false,
            true,
        );
        let floats = outcome.instance_floats();
        assert_eq!(floats.len(), 40); // 2 draws x (16 + 4)
        assert_eq!(&floats[0..16], &[1.0; 16]);
        assert_eq!(&floats[16..20], &[0.1, 0.2, 0.3, 1.0]);
        assert_eq!(&floats[20..36], &[2.0; 16]);
        assert_eq!(&floats[36..40], &[0.4, 0.5, 0.6, 1.0]);
    }

    #[test]
    fn instance_floats_empty_when_no_draws() {
        assert!(FrameOutcome::simulation_only(3, [0.0; 4])
            .instance_floats()
            .is_empty());
    }
}
