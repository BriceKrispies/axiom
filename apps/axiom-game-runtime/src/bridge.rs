//! `GameBridge`: the deterministic native core the `wasm32` boundary marshals to.
//!
//! This is the rlib heart of the bridge — everything the TS `NativeBridge` /
//! `HostBridge` seams drive, expressed in plain Rust over the real engine facades
//! so it is fully native-testable. The `wasm32` [`crate::wasm`] layer is a thin
//! JS-marshalling shell over this struct: it converts JS args to these method
//! calls and the results back, and owns only the browser channel (the inbound
//! query string, the outbound `postMessage`). Nothing here touches a browser
//! symbol or a wall clock.
//!
//! It composes three deterministic pieces, each over a real facade:
//! - the fixed-step [`GameRuntime`] (the `frame` accumulator → tick loop), which
//!   banks real elapsed nanoseconds into whole deterministic ticks;
//! - the [`RngHub`] (SPEC-01), the session-seeded entropy streams the `Rng`
//!   projection draws from, over `axiom-entropy`;
//! - the [`OutcomeLatch`] (SPEC-12), the emit-exactly-once terminal outcome.
//!
//! ## Landed vs deferred surface
//! This core lands the deterministic spine the rest of the bridge hangs off:
//! the fixed-step loop, the RNG seam, and the host outcome channel. The remaining
//! `NativeBridge` / `HostBridge` methods are deferred behind explicit structural
//! blockers, documented in [`crate::wasm`] and the slice report — chiefly the
//! retained world (the seam's opaque, `kind`-keyed dynamic component store has no
//! home in the engine's statically-typed ECS, and inventing one inside this app
//! would be a primitive built at the wrong layer), with physics depending on it
//! and input/tween/tick/grid/3D/audio/net following as later increments.

use axiom::prelude::{HostApi, HostOutcome, RunningApp, Score, StepBudget};

use crate::embed::OutcomeLatch;
use crate::rng::RngHub;
use crate::runtime::GameRuntime;

/// The deterministic native core: the fixed-step loop, the seeded RNG hub, the
/// session seed (fixed before tick 0), and the terminal-outcome latch. Construct
/// it with a built [`RunningApp`], the session seed, the fixed step (nanoseconds),
/// and the per-frame step ceiling.
#[derive(Debug)]
pub struct GameBridge {
    runtime: GameRuntime,
    rng: RngHub,
    seed: u64,
    outcome: OutcomeLatch,
}

impl GameBridge {
    /// Wrap a built app in the deterministic bridge core. The `seed` is the
    /// session determinism input (SPEC-12 §6): it keys the [`RngHub`] and is fixed
    /// for the whole session. `fixed_step_nanos` / `max_steps` configure the
    /// fixed-step loop exactly as [`GameRuntime::new`] documents.
    pub fn new(app: RunningApp, seed: u64, fixed_step_nanos: u64, max_steps: u32) -> Self {
        GameBridge {
            runtime: GameRuntime::new(app, fixed_step_nanos, max_steps),
            rng: RngHub::new(seed),
            seed,
            outcome: OutcomeLatch::new(),
        }
    }

    /// The host-supplied session seed, fixed before tick 0.
    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// Bank `elapsed_nanos` of real host time and run the resulting whole fixed
    /// ticks; returns the integer [`StepBudget`] for the presentation layer to
    /// interpolate with. Delegates to [`GameRuntime::advance`].
    pub fn advance(&mut self, elapsed_nanos: u64) -> StepBudget {
        self.runtime.advance(elapsed_nanos)
    }

    /// The monotonic count of fixed ticks driven so far.
    pub fn tick(&self) -> u64 {
        self.runtime.tick()
    }

    /// The durable simulation state as opaque bytes (checkpoint / determinism).
    pub fn snapshot_sim(&self) -> Vec<u8> {
        self.runtime.snapshot_sim()
    }

    /// A uniform draw in `[0, 1)` from `stream` (`Rng::unit`, SPEC-01).
    pub fn rng_unit(&mut self, stream: u32) -> f64 {
        self.rng.unit(stream)
    }

    /// A uniform integer in `[0, max_exclusive)` from `stream` (`Rng::int`).
    pub fn rng_below(&mut self, stream: u32, max_exclusive: u64) -> u64 {
        self.rng.below(stream, max_exclusive)
    }

    /// The weighted index `stream` selects over integer `weights` (`Rng::weighted`).
    pub fn rng_weighted(&mut self, stream: u32, weights: &[u64]) -> u32 {
        self.rng.weighted(stream, weights)
    }

    /// A Fisher-Yates permutation of `[0, length)` from `stream` (`Rng::permutation`).
    pub fn rng_permutation(&mut self, stream: u32, length: u32) -> Vec<u32> {
        self.rng.permutation(stream, length)
    }

    /// Resolve the id of the named sub-stream of `parent` (`Rng::stream`).
    pub fn rng_stream(&mut self, parent: u32, name: &str) -> u32 {
        self.rng.stream(parent, name)
    }

    /// Report the terminal outcome (`reportOutcome`, SPEC-12 §4.2): the first call
    /// latches exactly one [`HostOutcome`] derived from `won`/`score` and returns
    /// `true`; every later call is a no-op returning `false`.
    pub fn report_outcome(&mut self, won: bool, score: f64) -> bool {
        let outcome = HostApi::new().outcome(won, Score::new(score));
        self.outcome.report(outcome)
    }

    /// The latched terminal outcome, if one has been reported — the value the
    /// browser arm forwards to the parent frame.
    pub fn reported_outcome(&self) -> Option<&HostOutcome> {
        self.outcome.reported()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo_app;
    use axiom::prelude::HostOutcome;

    /// 1 ms fixed step — small enough that the test elapsed sequences cross step
    /// boundaries on every advance.
    const STEP: u64 = 1_000_000;

    fn bridge(seed: u64) -> GameBridge {
        GameBridge::new(demo_app().build(), seed, STEP, 1)
    }

    /// Deterministic FNV-1a over a byte buffer — the per-tick state fingerprint
    /// the determinism test compares across two independent runs.
    fn fnv1a(bytes: &[u8]) -> u64 {
        bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
        })
    }

    /// Drive the whole bridge (loop + RNG) for `seed`, producing the per-tick
    /// state-hash sequence: each tick advances one fixed step, draws from a named
    /// RNG sub-stream, and folds the sim snapshot AND the draw into one hash. This
    /// fingerprints the *entire* deterministic boundary, not just the scene.
    fn per_tick_hashes(seed: u64) -> Vec<u64> {
        let mut b = bridge(seed);
        let loot = b.rng_stream(0, "loot");
        (0..30u32)
            .map(|_| {
                b.advance(STEP);
                let draw = b.rng_below(loot, u64::from(u32::MAX));
                let mut buf = b.snapshot_sim();
                buf.extend_from_slice(&draw.to_le_bytes());
                fnv1a(&buf)
            })
            .collect()
    }

    #[test]
    fn same_seed_and_inputs_reproduce_the_per_tick_state_hash_sequence() {
        // The determinism invariant: same seed + same call sequence ⇒ a
        // byte-identical per-tick state-hash sequence across two independent runs.
        let first = per_tick_hashes(7);
        assert_eq!(first, per_tick_hashes(7));
        // The state genuinely evolves over the run (the demo cube spins and the
        // RNG stream advances), so the fingerprint is not constant — the ticks did
        // real work, this is not a degenerate all-equal sequence.
        assert!(first.iter().any(|&hash| hash != first[0]));
    }

    #[test]
    fn the_seed_changes_the_run() {
        // A different session seed re-keys the RNG, so the folded-in draw — and
        // thus the whole per-tick hash sequence — diverges from seed 7's run.
        assert_ne!(per_tick_hashes(7), per_tick_hashes(8));
    }

    #[test]
    fn seed_is_fixed_and_advance_drives_the_loop() {
        let mut b = bridge(42);
        assert_eq!(b.seed(), 42);
        assert_eq!(b.tick(), 0);
        let budget = b.advance(STEP);
        assert_eq!(budget.steps(), 1);
        assert_eq!(b.tick(), 1);
    }

    #[test]
    fn the_outcome_is_latched_exactly_once() {
        let mut b = bridge(0);
        assert!(b.reported_outcome().is_none());
        // The first report latches and is accepted; a second is a no-op.
        assert!(b.report_outcome(true, 9.0));
        assert!(!b.report_outcome(false, 0.0));
        assert_eq!(
            b.reported_outcome().map(HostOutcome::score),
            Some(Score::new(9.0))
        );
    }

    #[test]
    fn the_rng_seam_forwards_to_the_hub() {
        // The bridge's rng_* methods are the seam the projection drives; confirm
        // they route to the seeded hub (bounded draw, idempotent stream id).
        let mut b = bridge(5);
        let s = b.rng_stream(0, "spawn");
        assert_eq!(b.rng_stream(0, "spawn"), s);
        assert!(b.rng_below(s, 10) < 10);
        assert!((0.0..1.0).contains(&b.rng_unit(0)));
        assert_eq!(b.rng_weighted(0, &[0, 1, 0]), 1);
        let perm = b.rng_permutation(0, 8);
        assert_eq!(perm.len(), 8);
    }
}
