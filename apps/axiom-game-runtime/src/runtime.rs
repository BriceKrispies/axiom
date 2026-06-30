//! `GameRuntime`: the deterministic variable-dt → fixed-step driver.
//!
//! This is the rlib core the wasm boundary wraps and the native slice tests
//! exercise. It owns a [`RunningApp`], a [`FrameAccumulator`], and a monotonic
//! tick counter, and exposes one core method — [`GameRuntime::advance`] — that
//! banks a real elapsed-time interval into whole fixed steps and runs exactly
//! that many deterministic `RunningApp::tick` calls.
//!
//! The accumulator (in the `frame` layer) decides *how many* steps; this runtime
//! drives them; the presentation boundary (the TS SDK) computes the `0..1`
//! interpolation fraction `remainder_nanos / fixed_step_nanos` from the returned
//! [`StepBudget`]. No wall-clock value ever crosses into a fixed tick — elapsed
//! time enters [`advance`] as explicit data, exactly as the accumulator demands.
//!
//! [`advance`]: GameRuntime::advance

use axiom::prelude::{FrameAccumulator, RunningApp, StepBudget};

/// Drives a [`RunningApp`] at a deterministic fixed step from a variable-rate
/// host clock. Construct it with the built app, the fixed step in nanoseconds,
/// and the per-frame step ceiling (the spiral-of-death clamp), then call
/// [`Self::advance`] once per host frame with that frame's elapsed nanoseconds.
#[derive(Debug)]
pub struct GameRuntime {
    app: RunningApp,
    accumulator: FrameAccumulator,
    max_steps: u32,
    tick: u64,
}

impl GameRuntime {
    /// Wrap a built [`RunningApp`] in a fixed-step driver. `fixed_step_nanos` is
    /// the simulation step (e.g. `16_666_667` for 60 Hz); `max_steps` caps how
    /// many ticks one `advance` may run so a long host stall cannot trigger an
    /// unbounded catch-up (the clamped time stays banked, never dropped).
    ///
    /// A zero `fixed_step_nanos` is meaningless (every frame would complete
    /// infinitely many steps) and the accumulator rejects it; this boundary
    /// surfaces that as a panic at construction, the single validated point that
    /// lets `advance` divide without a guard.
    pub fn new(app: RunningApp, fixed_step_nanos: u64, max_steps: u32) -> Self {
        let accumulator = FrameAccumulator::new(fixed_step_nanos)
            .expect("game runtime fixed step must be non-zero");
        GameRuntime {
            app,
            accumulator,
            max_steps,
            tick: 0,
        }
    }

    /// Bank `elapsed_nanos` of real host time, run exactly `budget.steps()`
    /// deterministic fixed *steps* on the wrapped app (advancing the monotonic
    /// tick counter), and return the [`StepBudget`] so the caller can compute its
    /// own presentation-only interpolation fraction.
    ///
    /// This drives [`RunningApp::step`](axiom::prelude::RunningApp) — the
    /// simulation half **without rendering** — not the fused `tick`. Rendering is
    /// the presentation layer's job (the TS SDK calls `render` once per presented
    /// frame), so an N-tick catch-up frame does N cheap steps and a single render,
    /// not N wasted renders. The tick loop is a `fold` over `0..budget.steps()` —
    /// no `for`/`while`/`if` — so the same elapsed-time sequence drives the same
    /// total tick count regardless of how it was chunked across frames, the
    /// invariant a deterministic replay relies on.
    pub fn advance(&mut self, elapsed_nanos: u64) -> StepBudget {
        let budget = self.accumulator.advance(elapsed_nanos, self.max_steps);
        let app = &mut self.app;
        self.tick = (0..budget.steps()).fold(self.tick, |tick, _step| {
            app.step(tick, &[], &[]);
            tick + 1
        });
        budget
    }

    /// The monotonic count of fixed ticks driven so far — the value `advance`
    /// passes to the next `RunningApp::tick`.
    pub fn tick(&self) -> u64 {
        self.tick
    }

    /// Borrow the wrapped [`RunningApp`] — the retained world the bridge reads
    /// (queries, component gets, children) between steps.
    pub fn app(&self) -> &RunningApp {
        &self.app
    }

    /// Mutably borrow the wrapped [`RunningApp`] — the retained world the bridge
    /// mutates (spawn / despawn / component set) between steps. The fixed-step
    /// loop owns *when* the app ticks; this exposes *what* world it ticks so the
    /// composing bridge can host SPEC-02's retained game world over it.
    pub fn app_mut(&mut self) -> &mut RunningApp {
        &mut self.app
    }

    /// Serialize the durable simulation state of the wrapped app (the scene
    /// world), so a caller can compare or replay it. Delegates to
    /// [`RunningApp::snapshot_sim`]; two runtimes fed identical elapsed-time
    /// sequences produce byte-identical snapshots.
    pub fn snapshot_sim(&self) -> Vec<u8> {
        self.app.snapshot_sim()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo_app;

    /// 1 ms fixed step — small enough that the test elapsed sequences cross many
    /// step boundaries.
    const STEP: u64 = 1_000_000;

    fn runtime(max_steps: u32) -> GameRuntime {
        GameRuntime::new(demo_app().build(), STEP, max_steps)
    }

    /// Deterministic FNV-1a over a byte buffer: a per-tick state fingerprint the
    /// determinism test compares across two independent runs.
    fn fnv1a(bytes: &[u8]) -> u64 {
        bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
        })
    }

    #[test]
    fn new_starts_at_tick_zero() {
        assert_eq!(runtime(8).tick(), 0);
    }

    #[test]
    fn total_ticks_are_independent_of_frame_chunking() {
        // The replay invariant: the same total elapsed time, drained to empty,
        // drives the same total tick count no matter how it was split into frames.
        let total = 10 * STEP + STEP / 4;
        let drive = |chunks: &[u64]| -> u64 {
            let mut rt = runtime(u32::MAX);
            chunks.iter().for_each(|&elapsed| {
                rt.advance(elapsed);
            });
            // Flush any banked whole steps the chunking left behind.
            rt.advance(0);
            rt.tick()
        };
        assert_eq!(drive(&[total]), 10);
        assert_eq!(drive(&[total]), drive(&[STEP, total - STEP]));
        assert_eq!(drive(&[total]), drive(&[1, 1, total - 2]));
    }

    #[test]
    fn the_returned_budget_carries_the_interpolation_remainder() {
        let mut rt = runtime(8);
        // Half a step of elapsed time completes no tick and banks the remainder,
        // which the presentation layer turns into alpha = remainder / step.
        let budget = rt.advance(STEP / 2);
        assert_eq!(budget.steps(), 0);
        assert_eq!(budget.remainder_nanos(), STEP / 2);
        assert_eq!(budget.fixed_step_nanos(), STEP);
        assert_eq!(rt.tick(), 0);
    }

    #[test]
    fn the_per_frame_step_count_is_clamped() {
        let mut rt = runtime(3);
        // 100 steps' worth of time arrives at once; only `max_steps` run.
        assert_eq!(rt.advance(100 * STEP).steps(), 3);
        assert_eq!(rt.tick(), 3);
    }

    #[test]
    fn identical_elapsed_sequences_produce_identical_snapshots() {
        // Two independent runtimes, fed the same elapsed sequence, end byte-equal:
        // same total tick count AND byte-identical simulation snapshots.
        let sequence = [STEP, 3 * STEP, STEP / 2, 2 * STEP + 10, 5 * STEP];
        let run = || -> (u64, Vec<u8>) {
            let mut rt = runtime(u32::MAX);
            sequence.iter().for_each(|&elapsed| {
                rt.advance(elapsed);
            });
            (rt.tick(), rt.snapshot_sim())
        };
        let (ticks_a, snapshot_a) = run();
        let (ticks_b, snapshot_b) = run();
        assert_eq!(ticks_a, ticks_b);
        assert_eq!(snapshot_a, snapshot_b);
    }

    #[test]
    fn per_tick_state_hash_sequence_reproduces_and_evolves() {
        // With max_steps = 1 each `advance(STEP)` runs exactly one tick, so the
        // snapshot after each advance is that tick's state. The whole hash
        // sequence reproduces on a second run (determinism)...
        let hashes = || -> Vec<u64> {
            let mut rt = runtime(1);
            (0..30u32)
                .map(|_step| {
                    rt.advance(STEP);
                    fnv1a(&rt.snapshot_sim())
                })
                .collect()
        };
        let first = hashes();
        assert_eq!(first, hashes());
        // ...and the state genuinely evolves (the demo cube spins), so the
        // fingerprint is not constant — proving the ticks did real work.
        assert!(first.iter().any(|&hash| hash != first[0]));
    }
}
