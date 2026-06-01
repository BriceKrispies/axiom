//! The per-advance context handed to every [`crate::WorldSystem`].

/// The logical time of one world advance.
///
/// A [`crate::World`] advances its systems once per engine frame; `WorldStep`
/// carries the frame's monotonic tick (the engine frame index) so time-based
/// systems — animation, spin, anything that evolves with the clock — can be a
/// pure function of it and stay replay-deterministic. It deliberately exposes
/// only the tick: systems that need richer frame data take a higher-level
/// contract, not the raw frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorldStep {
    tick: u64,
}

impl WorldStep {
    /// Construct a step at `tick`.
    pub const fn new(tick: u64) -> Self {
        WorldStep { tick }
    }

    /// The monotonic frame tick this advance represents.
    pub const fn tick(&self) -> u64 {
        self.tick
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn carries_its_tick() {
        let s = WorldStep::new(7);
        assert_eq!(s.tick(), 7);
    }

    #[test]
    fn equality_is_by_tick() {
        assert_eq!(WorldStep::new(3), WorldStep::new(3));
        assert_ne!(WorldStep::new(3), WorldStep::new(4));
    }
}
