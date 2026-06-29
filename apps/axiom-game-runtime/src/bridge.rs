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
//! It composes the deterministic pieces, each over a real facade:
//! - the fixed-step [`GameRuntime`] (the `frame` accumulator → tick loop), which
//!   banks real elapsed nanoseconds into whole deterministic ticks **and** owns
//!   the [`RunningApp`] the retained world lives in;
//! - the [`RngHub`] (SPEC-01), the session-seeded entropy streams the `Rng`
//!   projection draws from, over `axiom-entropy`;
//! - the [`OutcomeLatch`] (SPEC-12), the emit-exactly-once terminal outcome.
//!
//! ## Retained world (SPEC-02)
//! The `world*` methods host the TS `NativeBridge` retained world over the app's
//! dynamic, schema-name-keyed component store (`RunningApp::set_dynamic` /
//! `query_dynamic` / `despawn_subtree`, landed in the `axiom` umbrella). The
//! opaque component vocabulary the seam needs is **not** a new ECS primitive
//! invented in this app — it is the engine's existing dynamic arm, which this
//! app merely *populates* with its closed game-component `Reflect` types
//! ([`crate::world`]). A component crosses the boundary as a `(kind, bytes)`
//! pair; the convention is documented in [`crate::world`]. Entity handles cross
//! as their raw `u64` id (the `wasm32` shell narrows them to JS numbers).
//! `worldSpawn` is composed at the TS edge from `world_spawn` + per-component
//! `world_set`, so this core exposes only scalar / byte / string methods.
//!
//! Input / tween / tick / grid / 3D / audio / net remain later increments.

use axiom::prelude::{Entity, HostApi, HostOutcome, RunningApp, Score, StepBudget};

use crate::embed::OutcomeLatch;
use crate::rng::RngHub;
use crate::runtime::GameRuntime;
use crate::world;

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

    // --- Retained world (SPEC-02) ---
    //
    // The TS `NativeBridge` world surface over the app's dynamic component store.
    // Entity handles cross as their raw `u64` id; components as `(kind, bytes)`.

    /// Spawn a bare entity carrying no components, returning its raw id
    /// (`worldSpawn`'s root, which the TS edge then dresses with `world_set`s).
    pub fn world_spawn(&mut self) -> u64 {
        self.runtime.app_mut().spawn_empty().raw()
    }

    /// Despawn one entity, returning whether it named a live node (`worldDespawn`;
    /// a stale handle is a clean `false`).
    pub fn world_despawn(&mut self, entity: u64) -> bool {
        self.runtime.app_mut().despawn(Entity::from_raw(entity))
    }

    /// Despawn an entity and its whole subtree (`worldDespawnSubtree`), returning
    /// whether `entity` named a live node.
    pub fn world_despawn_subtree(&mut self, entity: u64) -> bool {
        self.runtime
            .app_mut()
            .despawn_subtree(Entity::from_raw(entity))
    }

    /// Set (or replace) `entity`'s component of `kind` from its field `bytes`
    /// (`worldSet`). An unknown kind, a stale entity, or undecodable bytes are all
    /// a clean `false`.
    pub fn world_set(&mut self, entity: u64, kind: &str, bytes: &[u8]) -> bool {
        world::world_set(self.runtime.app_mut(), Entity::from_raw(entity), kind, bytes)
    }

    /// Read `entity`'s component of `kind` as field bytes (`worldGet`) — an empty
    /// buffer on a miss / dead entity / unknown kind (the TS edge maps it to the
    /// empty `Result`).
    pub fn world_get(&self, entity: u64, kind: &str) -> Vec<u8> {
        world::world_get(self.runtime.app(), Entity::from_raw(entity), kind)
    }

    /// Every entity carrying *all* of `kinds`, in ascending-id order
    /// (`worldQuery`). An unknown kind makes the result empty (nothing can carry a
    /// kind the engine was never given).
    pub fn world_query(&self, kinds: &[&str]) -> Vec<u64> {
        world::static_kinds(kinds)
            .map(|resolved| {
                self.runtime
                    .app()
                    .query_dynamic(&resolved)
                    .into_iter()
                    .map(Entity::raw)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The direct children of `entity`, in ascending-id order (`worldChildrenOf`).
    pub fn world_children_of(&self, entity: u64) -> Vec<u64> {
        self.runtime
            .app()
            .children_of(Entity::from_raw(entity))
            .into_iter()
            .map(Entity::raw)
            .collect()
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

    use axiom::prelude::{BinaryReader, Reflect};
    use crate::world::{Transform2D, Velocity2D};

    /// A `Transform` component's bytes with the given position (defaults for the
    /// rest), for driving `world_set` over the byte boundary.
    fn transform_bytes(x: f32, y: f32) -> Vec<u8> {
        world::encode(&Transform2D {
            x,
            y,
            rotation: 0.0,
            scale_x: 1.0,
            scale_y: 1.0,
        })
    }

    /// A deterministic fingerprint of the *observable* retained-world state: the
    /// `Transform`/`Velocity` query results (ascending-id) plus each entity's
    /// component reads, folded with FNV-1a. This is the keystone determinism probe
    /// — it sees exactly what an author sees through the world surface.
    fn world_state_hash(b: &GameBridge, entities: &[u64]) -> u64 {
        let mut buf: Vec<u8> = Vec::new();
        ["Transform", "Velocity"].iter().for_each(|kind| {
            b.world_query(&[kind])
                .iter()
                .for_each(|&e| buf.extend_from_slice(&e.to_le_bytes()));
            buf.push(0xFF); // a kind separator so two queries can't alias.
        });
        entities.iter().for_each(|&e| {
            buf.extend_from_slice(&b.world_get(e, "Transform"));
            buf.extend_from_slice(&b.world_get(e, "Velocity"));
        });
        fnv1a(&buf)
    }

    /// Drive a scripted retained-world session: spawn 5 entities, then over 8
    /// ticks set each entity's `Transform` (and the even ones' `Velocity`) to a
    /// tick/index-derived value, despawning one entity midway. Returns the per-tick
    /// observable-state hash sequence.
    fn world_session_hashes() -> Vec<u64> {
        let mut b = bridge(7);
        let entities: Vec<u64> = (0..5u32).map(|_| b.world_spawn()).collect();
        (0..8u32)
            .map(|tick| {
                b.advance(STEP);
                entities.iter().enumerate().for_each(|(i, &e)| {
                    let fx = tick as f32 + i as f32;
                    b.world_set(e, "Transform", &transform_bytes(fx, fx * 2.0));
                    (i % 2 == 0).then(|| {
                        b.world_set(e, "Velocity", &world::encode(&Velocity2D { x: fx, y: -fx }))
                    });
                });
                (tick == 3).then(|| b.world_despawn(entities[2]));
                world_state_hash(&b, &entities)
            })
            .collect()
    }

    #[test]
    fn the_retained_world_replays_to_a_byte_identical_state_hash_sequence() {
        // The keystone proof: the same scripted spawn/set/despawn sequence over the
        // retained world produces a byte-identical per-tick observable-state hash
        // sequence across two independent runs.
        let first = world_session_hashes();
        assert_eq!(first, world_session_hashes());
        // The world genuinely evolves (positions change every tick, an entity is
        // removed), so the fingerprint is not constant — this is real work.
        assert!(first.iter().any(|&hash| hash != first[0]));
    }

    #[test]
    fn world_reads_back_the_last_value_set_and_queries_track_lifecycle() {
        let mut b = bridge(1);
        let a = b.world_spawn();
        let c = b.world_spawn();
        // Distinct live entities; a Transform set on each reads back the last value.
        assert_ne!(a, c);
        assert!(b.world_set(a, "Transform", &transform_bytes(3.0, 4.0)));
        assert!(b.world_set(a, "Transform", &transform_bytes(7.0, 8.0))); // replace
        let got = Transform2D::reflect_read(&mut BinaryReader::new(&b.world_get(a, "Transform")))
            .unwrap();
        assert_eq!((got.x, got.y), (7.0, 8.0));
        // Both carry Transform ⇒ both appear in the query, ascending-id order.
        assert!(b.world_set(c, "Transform", &transform_bytes(1.0, 1.0)));
        assert_eq!(b.world_query(&["Transform"]), vec![a, c]);
        // Only `a` carries Velocity ⇒ the two-kind intersection is just `a`.
        assert!(b.world_set(a, "Velocity", &world::encode(&Velocity2D { x: 1.0, y: 2.0 })));
        assert_eq!(b.world_query(&["Transform", "Velocity"]), vec![a]);
        // An unknown kind in the query makes it empty (closed vocabulary).
        assert!(b.world_query(&["Transform", "ghost"]).is_empty());
        // Despawn removes `a` from the world: its reads go empty and it leaves the
        // query; `c` is untouched.
        assert!(b.world_despawn(a));
        assert!(b.world_get(a, "Transform").is_empty());
        assert_eq!(b.world_query(&["Transform"]), vec![c]);
        // A stale despawn is a clean false; a leaf has no children.
        assert!(!b.world_despawn(a));
        assert!(b.world_children_of(c).is_empty());
    }
}
