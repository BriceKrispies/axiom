//! Deterministic time/step identity, built on the kernel's [`SimulationClock`].

use axiom_kernel::{FrameIndex, SimulationClock, Tick};

use crate::runtime_error::RuntimeError;
use crate::runtime_error_code::RuntimeErrorCode;
use crate::runtime_result::RuntimeResult;
use crate::runtime_step::RuntimeStep;

/// Wraps a kernel [`SimulationClock`] into a runtime-flavored time source.
///
/// The kernel `SimulationClock` is the ground truth for ticks, frames, and
/// elapsed nanoseconds. The timeline adds a runtime-owned monotonic sequence
/// number — incremented once per successful `advance` — so replays can refer
/// to a specific step record by its sequence id even if a future system later
/// changes how ticks map to frames.
#[derive(Debug, Clone, Copy)]
pub struct RuntimeTimeline {
    clock: SimulationClock,
    sequence: u64,
}

impl RuntimeTimeline {
    /// Wrap a freshly constructed kernel clock.
    pub const fn new(clock: SimulationClock) -> Self {
        RuntimeTimeline { clock, sequence: 0 }
    }

    /// The step identity *as of now* — without advancing.
    pub fn current_step(&self) -> RuntimeStep {
        RuntimeStep::new(
            self.clock.frame(),
            self.clock.tick(),
            self.clock.step().nanos(),
            self.sequence,
        )
    }

    /// Advance the kernel clock by one fixed step, bump the sequence number,
    /// and return the new step identity.
    ///
    /// Any kernel error (e.g. accumulated-nanoseconds overflow) is wrapped in
    /// a [`RuntimeError`] whose [`RuntimeError::kernel`] retains the original
    /// `(scope, code)` identity.
    pub fn advance(&mut self) -> RuntimeResult<RuntimeStep> {
        let advanced = self.clock.advance().map_err(|e| {
            RuntimeError::with_kernel(
                RuntimeErrorCode::KernelFailure,
                "kernel SimulationClock advance failed",
                e,
            )
        });
        advanced.map(|_| {
            self.sequence = self.sequence.saturating_add(1);
            self.current_step()
        })
    }

    /// The kernel-typed current frame index.
    pub fn frame(&self) -> FrameIndex {
        self.clock.frame()
    }

    /// The kernel-typed current tick.
    pub fn tick(&self) -> Tick {
        self.clock.tick()
    }

    /// The monotonic sequence number.
    pub const fn sequence(&self) -> u64 {
        self.sequence
    }

    /// Elapsed simulated nanoseconds since the wrapped clock was created.
    pub const fn elapsed_nanos(&self) -> u64 {
        self.clock.elapsed_nanos()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::KernelApi;

    fn timeline(step_nanos: u64) -> RuntimeTimeline {
        let api = KernelApi::new();
        let step = api.fixed_step(step_nanos).unwrap();
        RuntimeTimeline::new(api.simulation_clock(step))
    }

    #[test]
    fn fresh_timeline_is_at_zero() {
        let t = timeline(1000);
        let s = t.current_step();
        assert_eq!(s.tick(), Tick::new(0));
        assert_eq!(s.frame(), FrameIndex::new(0));
        assert_eq!(s.sequence(), 0);
        assert_eq!(t.elapsed_nanos(), 0);
    }

    #[test]
    fn each_advance_increments_tick_frame_and_sequence_by_one() {
        let mut t = timeline(1000);
        let s1 = t.advance().unwrap();
        let s2 = t.advance().unwrap();
        assert_eq!(s1.sequence(), 1);
        assert_eq!(s2.sequence(), 2);
        assert_eq!(s1.tick(), Tick::new(1));
        assert_eq!(s2.tick(), Tick::new(2));
        assert_eq!(s1.frame(), FrameIndex::new(1));
        assert_eq!(s2.frame(), FrameIndex::new(2));
        assert_eq!(t.elapsed_nanos(), 2000);
    }

    #[test]
    fn getters_return_advanced_values_not_constants() {
        // After three advances, sequence/frame/tick are all 3 — a value the
        // mutation constants (0, 1) cannot produce.
        let mut t = timeline(1000);
        t.advance().unwrap();
        t.advance().unwrap();
        t.advance().unwrap();
        assert_eq!(t.sequence(), 3);
        assert_eq!(t.frame(), FrameIndex::new(3));
        assert_eq!(t.tick(), Tick::new(3));
        // Guard the boundary the `with 0`/`with 1` constants would satisfy.
        assert_ne!(t.sequence(), 0);
        assert_ne!(t.sequence(), 1);
        assert_ne!(t.frame(), FrameIndex::default());
        assert_ne!(t.tick(), Tick::default());
    }

    #[test]
    fn two_timelines_advance_to_byte_identical_state() {
        let mut a = timeline(500);
        let mut b = timeline(500);
        for _ in 0..10 {
            a.advance().unwrap();
            b.advance().unwrap();
        }
        assert_eq!(a.current_step(), b.current_step());
    }
}
