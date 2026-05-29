//! A single deterministic runtime step's identity.

use axiom_kernel::{FrameIndex, Tick};

/// Immutable identity for one runtime step.
///
/// Carries kernel-typed frame and tick identities plus the deterministic fixed
/// delta and a runtime-owned monotonic sequence number. Two runs with the same
/// configuration and the same number of `step()` calls produce byte-identical
/// step values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RuntimeStep {
    frame: FrameIndex,
    tick: Tick,
    fixed_delta_nanos: u64,
    sequence: u64,
}

impl RuntimeStep {
    /// Construct a step identity. Normally produced by [`crate::runtime_timeline::RuntimeTimeline`].
    pub const fn new(frame: FrameIndex, tick: Tick, fixed_delta_nanos: u64, sequence: u64) -> Self {
        RuntimeStep {
            frame,
            tick,
            fixed_delta_nanos,
            sequence,
        }
    }

    /// The kernel-typed frame index of this step.
    pub const fn frame(&self) -> FrameIndex {
        self.frame
    }

    /// The kernel-typed simulation tick of this step.
    pub const fn tick(&self) -> Tick {
        self.tick
    }

    /// The fixed simulated delta in integer nanoseconds.
    pub const fn fixed_delta_nanos(&self) -> u64 {
        self.fixed_delta_nanos
    }

    /// The runtime-owned monotonic sequence number. Increments by exactly 1
    /// per successful `step()` and is independent of the kernel tick (it lets
    /// replays distinguish *which* step record they are looking at without
    /// relying on tick equality).
    pub const fn sequence(&self) -> u64 {
        self.sequence
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accessors_return_constructed_parts() {
        let s = RuntimeStep::new(FrameIndex::new(2), Tick::new(7), 16_666_667, 3);
        assert_eq!(s.frame(), FrameIndex::new(2));
        assert_eq!(s.tick(), Tick::new(7));
        assert_eq!(s.fixed_delta_nanos(), 16_666_667);
        assert_eq!(s.sequence(), 3);
    }

    #[test]
    fn equal_inputs_produce_equal_steps() {
        let a = RuntimeStep::new(FrameIndex::new(1), Tick::new(1), 1000, 1);
        let b = RuntimeStep::new(FrameIndex::new(1), Tick::new(1), 1000, 1);
        assert_eq!(a, b);
    }
}
