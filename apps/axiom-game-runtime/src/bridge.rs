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
//! ## Input (SPEC-05)
//! The `input_*` methods host the TS `NativeBridge` input surface over the
//! engine's `axiom-input` intent-snapshot facade ([`crate::input`]): the browser
//! arm feeds raw key/pointer events through the injection path, [`advance`] folds
//! the live device frame into the per-tick snapshot, and the reads project the
//! resolved intent. Action names cross as strings; edges/holds as booleans;
//! optional reads (pointer / press-start / pressed-at-tick) as a `Vec<f64>` that
//! is empty when absent; a swipe as its direction string.
//!
//! ## Time, state machines, and tweens (SPEC-07 / SPEC-09)
//! The `timer_*` / `machine_*` / `tween_*` methods host the TS `NativeBridge`
//! timer, state-machine, and tween surface over the deterministic `axiom-tick`
//! timer wheel + state machines and the `axiom-tween` eased-curve table
//! ([`crate::time`]). Both schedules are **mutating** per-tick reads, so [`advance`]
//! pumps them once per fixed tick and records what fired; the read methods report
//! that recorded frame. Ids and ticks cross as numbers, a fired-id / active-id /
//! completed-id list as a `Vec<u64>`, a tween value as `f64`.
//!
//! ## Grid, math, and audio (SPEC-06 / SPEC-03 / SPEC-11 / SPEC-08)
//! The `grid` / `mathbridge` / `audio` modules host the TS `HostBridge` query and
//! presentation surface: deterministic `axiom-grid` pathfinding ([`crate::grid`]),
//! `v3`/`mat4`/`quat`/scalar ops forwarding to `axiom-math` ([`crate::mathbridge`]),
//! and the neutral `axiom-audio` mixer core whose live Web Audio output is the
//! `wasm32` arm ([`crate::audio`]). 3D scene authoring (`createMesh`/`createMaterial`/
//! `setCamera3D`/`addLight`) and `mat4Invert` stay deferred behind documented engine
//! gaps (no runtime scene authoring on `RunningApp`; no `Mat4::inverse` in the math
//! layer) — a follow-up engine phase adds those primitives, not this app.
//!
//! ## Net (SPEC-13)
//! Net is **not** bridged through this wasm core. The browser already has a
//! complete, independent client — `@axiom/client` (`AxiomClient` + its own wire
//! codec + transports) — so `@axiom/game`'s `NetTransport` seam binds over it at the
//! TS edge (`packages/axiom-game/src/axiom-net.ts`), and the Rust
//! `axiom-net-protocol`/`axiom-client-core` crates stay the server/native substrate.
//! Re-implementing a net codec + socket here would duplicate that single source of
//! truth, so this core deliberately owns no net state.
//!
//! [`advance`]: GameBridge::advance

use axiom::prelude::{Entity, FrameOutcome, HostApi, HostOutcome, RunningApp, Score, StepBudget};
use axiom_draw2d::Draw2dApi;
use axiom_grid::GridApi;

use crate::assets::AssetRegistry;
use crate::audio::AudioState;
use crate::embed::OutcomeLatch;
use crate::input::InputBridge;
use crate::physics::PhysicsState;
use crate::rng::RngHub;
use crate::runtime::GameRuntime;
use crate::time::TimeBridge;
use crate::ui::UiState;
use crate::world;

/// The deterministic native core: the fixed-step loop, the seeded RNG hub, the
/// session seed (fixed before tick 0), and the terminal-outcome latch. Construct
/// it with a built [`RunningApp`], the session seed, the fixed step (nanoseconds),
/// and the per-frame step ceiling.
#[derive(Debug)]
pub struct GameBridge {
    pub(crate) runtime: GameRuntime,
    rng: RngHub,
    seed: u64,
    outcome: OutcomeLatch,
    pub(crate) physics: PhysicsState,
    pub(crate) input: InputBridge,
    pub(crate) time: TimeBridge,
    /// The deterministic grid / pathfinding core (SPEC-06), forwarded to by the
    /// `grid_*` methods in [`crate::grid`]. Stateless, so the queries are pure.
    pub(crate) grid: GridApi,
    /// The neutral audio mixer core + live output arm (SPEC-08), driven by the
    /// `*_sound` / `*_tone` / `*_music` / mix methods in [`crate::audio`].
    pub(crate) audio: AudioState,
    /// The immediate-mode UI surface + encoded draw log (SPEC-09), driven by the
    /// `ui_*` methods in [`crate::ui`].
    pub(crate) ui: UiState,
    /// The 2D draw builder (SPEC-10: particles / render targets / shapes), driven
    /// by the `draw2d_*` methods in [`crate::draw2d`].
    pub(crate) draw2d: Draw2dApi,
    /// The presentation texture/font handle registry (SPEC-04 §10), driven by the
    /// `load_texture` / `texture_url` / `load_font` methods in [`crate::assets`].
    pub(crate) assets: AssetRegistry,
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
            physics: PhysicsState::new(),
            input: InputBridge::new(),
            time: TimeBridge::new(fixed_step_nanos),
            grid: GridApi::new(),
            audio: AudioState::new(),
            ui: UiState::new(),
            draw2d: Draw2dApi::new(),
            assets: AssetRegistry::new(),
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
        let start = self.runtime.tick();
        let budget = self.runtime.advance(elapsed_nanos);
        self.input.sample(start, budget.steps());
        // Snapshot the frame's accumulated relative look into this tick's read and
        // zero the accumulator (the fold-then-reset the original mouse-look does).
        self.input.commit_look();
        self.physics.step_and_writeback(
            self.runtime.app_mut(),
            budget.steps(),
            budget.fixed_step_nanos(),
        );
        self.time
            .pump(start, budget.steps(), budget.fixed_step_nanos());
        // Drain the audio batch accumulated this frame into the live output (the
        // wasm Web Audio arm; a no-op on native). Presentation-only — it reads no
        // sim state and feeds none back, so it never perturbs determinism.
        self.realize_audio();
        budget
    }

    /// The monotonic count of fixed ticks driven so far.
    pub fn tick(&self) -> u64 {
        self.runtime.tick()
    }

    /// Render the current 3D scene state at the current tick and return the
    /// summarised [`FrameOutcome`] — the present half of a frame (SPEC-11). The
    /// wasm boundary calls this once per host frame, *after* the author's per-frame
    /// scene mutations (camera / node transforms), so the presented pixels reflect
    /// the very latest authored state; it steps no simulation (see
    /// [`RunningApp::render`](axiom::prelude::RunningApp)). The native slice tests
    /// drive the same path a browser presents.
    pub fn render_frame(&mut self) -> FrameOutcome {
        let tick = self.runtime.tick();
        self.runtime.app_mut().render(tick)
    }

    /// The live-backend mesh upload set for the current scene (`(mesh_id,
    /// interleaved vertices, indices)`) — what the windowing presenter uploads once
    /// when the surface is bound.
    pub fn mesh_set(&self) -> Vec<(u64, Vec<f32>, Vec<u32>)> {
        self.runtime.app().mesh_set()
    }

    /// The live-backend material upload set for the current scene (`(material_id,
    /// width, height, RGBA8 albedo)`) — uploaded once when the surface is bound.
    pub fn material_set(&self) -> Vec<(u64, u32, u32, Vec<u8>)> {
        self.runtime.app().material_textures()
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

    // --- Hierarchy + liveness reads (SPEC-02) ---
    //
    // The remaining `NativeBridge` world surface over the engine's Entity-addressed
    // scene seam: liveness, kind-keyed presence/removal, parent linking, and the
    // authoritative world transform. Entity handles cross as raw ids; an optional
    // read is the empty-is-absent `Vec<f64>` the rest of the boundary uses.

    /// Whether `entity` names a live node (`worldAlive`); a stale handle is `false`.
    pub fn world_alive(&self, entity: u64) -> bool {
        self.runtime.app().is_alive(Entity::from_raw(entity))
    }

    /// Whether `entity` carries a component of `kind` (`worldHas`); an unknown
    /// kind / dead entity / absent component is a clean `false`.
    pub fn world_has(&self, entity: u64, kind: &str) -> bool {
        world::world_has(self.runtime.app(), Entity::from_raw(entity), kind)
    }

    /// Remove `entity`'s component of `kind` (`worldRemove`), returning whether it
    /// existed.
    pub fn world_remove(&mut self, entity: u64, kind: &str) -> bool {
        world::world_remove(self.runtime.app_mut(), Entity::from_raw(entity), kind)
    }

    /// Re-parent `child` under `parent` (`worldSetParent`); self-parenting, a
    /// cycle, or a stale handle is a clean `false`. World transforms refresh so a
    /// later `world_world_transform` read reflects the new chain.
    pub fn world_set_parent(&mut self, child: u64, parent: u64) -> bool {
        self.runtime
            .app_mut()
            .set_parent(Entity::from_raw(child), Entity::from_raw(parent))
    }

    /// `entity`'s parent as `[]` (root / absent) or `[parent]` (`worldParentOf`).
    pub fn world_parent_of(&self, entity: u64) -> Vec<f64> {
        self.runtime
            .app()
            .parent_of(Entity::from_raw(entity))
            .map(|parent| vec![parent.raw() as f64])
            .unwrap_or_default()
    }

    /// `entity`'s authoritative world transform (`worldWorldTransform`) as `[]`
    /// (absent) or the flat 10-tuple `[tx, ty, tz, qx, qy, qz, qw, sx, sy, sz]`.
    pub fn world_world_transform(&self, entity: u64) -> Vec<f64> {
        self.runtime
            .app()
            .world_transform(Entity::from_raw(entity))
            .map(|t| {
                vec![
                    f64::from(t.translation.x),
                    f64::from(t.translation.y),
                    f64::from(t.translation.z),
                    f64::from(t.rotation.x),
                    f64::from(t.rotation.y),
                    f64::from(t.rotation.z),
                    f64::from(t.rotation.w),
                    f64::from(t.scale.x),
                    f64::from(t.scale.y),
                    f64::from(t.scale.z),
                ]
            })
            .unwrap_or_default()
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
    fn render_frame_summarises_the_authored_scene_without_stepping() {
        // The present half the wasm boundary drives: the demo scene (one cube,
        // a camera, a light) renders one draw and one light, and rendering twice at
        // the same tick perturbs no simulation state (it is pure present).
        let mut b = bridge(3);
        let before = b.snapshot_sim();
        let outcome = b.render_frame();
        assert_eq!(outcome.draws().len(), 1, "the demo cube renders one draw");
        assert_eq!(outcome.lights().len(), 1, "the demo directional light resolves");
        // The upload sets the presenter binds are non-empty for the demo scene.
        assert_eq!(b.mesh_set().len(), 1);
        assert_eq!(b.material_set().len(), 1);
        assert_eq!(b.render_frame(), outcome, "render is idempotent at a fixed tick");
        assert_eq!(before, b.snapshot_sim(), "render steps no simulation");
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

    #[test]
    fn world_hierarchy_liveness_presence_and_transform_reads() {
        let mut b = bridge(1);
        let parent = b.world_spawn();
        let child = b.world_spawn();
        // Liveness: live spawns are alive; a stale handle is not.
        assert!(b.world_alive(parent));
        assert!(!b.world_alive(9999));
        // Kind-keyed presence + removal over the closed vocabulary.
        assert!(b.world_set(child, "Transform", &transform_bytes(2.0, 3.0)));
        assert!(b.world_has(child, "Transform"));
        assert!(!b.world_has(child, "Velocity"));
        assert!(!b.world_has(child, "ghost")); // unknown kind ⇒ clean false
        assert!(b.world_remove(child, "Transform"));
        assert!(!b.world_has(child, "Transform"));
        assert!(!b.world_remove(child, "Transform")); // already gone ⇒ false
        assert!(!b.world_remove(child, "ghost")); // unknown kind ⇒ false
        // Parent linking: none initially, then `child` under `parent`.
        assert!(b.world_parent_of(child).is_empty());
        assert!(b.world_set_parent(child, parent));
        assert_eq!(b.world_parent_of(child), vec![parent as f64]);
        // Self-parenting is rejected as a clean false.
        assert!(!b.world_set_parent(parent, parent));
        // World transform is the flat 10-tuple for a live node, empty for a stale one.
        assert_eq!(b.world_world_transform(parent).len(), 10);
        assert!(b.world_world_transform(9999).is_empty());
    }
}
