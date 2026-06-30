//! SPEC-13 §16.5 / §6 — the **physics net-prediction reconciliation proof**.
//!
//! `ClientCoreApi` exposes opt-in local-player prediction
//! ([`ClientCoreApi::set_predict_local_player`]) and a generic resimulation driver
//! ([`ClientCoreApi::resimulate`]) that folds the still-unacked local intents over
//! the caller's deterministic fixed step. This test is the load-bearing proof that
//! the driver reconciles **drift-free**: a predicting client that snaps to the
//! authoritative snapshot and replays its unacked intents through a *real physics
//! step* reaches **byte-identical** physics state to the authority that applied the
//! same intents directly.
//!
//! ## Scope of the determinism claim
//! This is **same-binary** byte-identity (one build, two worlds) — which is what
//! `axiom-physics` actually guarantees today (`LEGITIMACY_AUDIT.md`): identical
//! ordered inputs ⇒ identical snapshots. Cross-target (native ↔ wasm32) f32
//! bit-determinism is the separate, still-unresolved SPEC-10 §17.6 obligation, and
//! it is precisely why prediction is shipped **default-OFF** and gated behind an
//! explicit opt-in. When that obligation is met, the same driver predicts across
//! the wire unchanged; until then the opt-in carries the caveat.
//!
//! The test imports `axiom-physics` / `axiom-math` / `axiom-runtime` only as
//! **dev-dependencies** (it lives in `tests/`), so the module's runtime surface
//! stays kernel-only and `allowed_modules = []`.

use axiom_client_core::ClientCoreApi;
use axiom_kernel::{BinaryWriter, FrameIndex, Ratio, Tick};
use axiom_math::{Transform, Vec3};
use axiom_physics::{PhysicsApi, PhysicsBodyHandle};
use axiom_runtime::RuntimeStep;

/// The authoritative fixed step (~60 Hz), constant across the authority and the
/// predicting client so the same intents integrate identically.
const FIXED_STEP_NS: u64 = 16_666_667;

/// A live physics world plus the dynamic body the intents drive — the state `S`
/// the resimulation driver threads.
type World = (PhysicsApi, PhysicsBodyHandle);

/// One client intent: a planar `(dx, dy)` impulse, the same two-`f32` payload the
/// netplay authority decodes (`tools/axiom-netplay-server`).
fn move_payload(dx: f32, dy: f32) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&dx.to_le_bytes());
    bytes.extend_from_slice(&dy.to_le_bytes());
    bytes
}

/// Decode a `(dx, dy)` intent payload (the inverse of [`move_payload`]).
fn decode(payload: &[u8]) -> (f32, f32) {
    let dx = f32::from_le_bytes(payload[0..4].try_into().unwrap());
    let dy = f32::from_le_bytes(payload[4..8].try_into().unwrap());
    (dx, dy)
}

/// A fresh world holding one unit-mass dynamic body at the origin.
fn fresh_world() -> World {
    let mut api = PhysicsApi::new();
    let body = api
        .create_dynamic_body(Transform::IDENTITY, Ratio::new(1.0).unwrap())
        .unwrap();
    (api, body)
}

/// The fixed step for the `tick`-th update (deterministic from the tick index).
fn rt_step(tick: u64) -> RuntimeStep {
    RuntimeStep::new(FrameIndex::new(tick), Tick::new(tick), FIXED_STEP_NS, tick)
}

/// Apply one intent (impulse) to `world`'s body, then advance one fixed step. This
/// is the single deterministic per-intent fixed update shared by the authority and
/// the predicting client.
fn apply_and_step(world: &mut World, payload: &[u8], tick: u64) {
    let (dx, dy) = decode(payload);
    world.0.apply_impulse(world.1, Vec3::new(dx, dy, 0.0)).unwrap();
    world.0.step(rt_step(tick)).unwrap();
}

/// Serialize the world's physics state to bytes: the step index plus, per body in
/// snapshot order, its transform and linear/angular velocity. Byte-equality of two
/// such blobs is bit-identical physics state.
fn serialize(world: &World) -> Vec<u8> {
    let snapshot = world.0.snapshot();
    let mut writer = BinaryWriter::new();
    writer.write_u64(snapshot.step_index());
    snapshot.bodies().iter().for_each(|body| {
        body.transform().write_to(&mut writer);
        body.linear_velocity().write_to(&mut writer);
        body.angular_velocity().write_to(&mut writer);
    });
    writer.into_bytes()
}

/// The scripted per-player intent stream (5 ticks of planar impulses).
fn intent_stream() -> Vec<Vec<u8>> {
    vec![
        move_payload(0.50, 0.00),
        move_payload(0.00, 0.50),
        move_payload(-0.30, 0.20),
        move_payload(0.10, -0.40),
        move_payload(0.20, 0.20),
    ]
}

/// Run a world through `intents[..count]`, applying intent `i` at tick `i`.
fn run_through(intents: &[Vec<u8>], count: usize) -> World {
    let mut world = fresh_world();
    (0..count).for_each(|i| apply_and_step(&mut world, &intents[i], i as u64));
    world
}

/// A `ClientCoreApi` driven to hold exactly `intents` as pending, with the first
/// `acked` of them acknowledged by a snapshot and prediction enabled. After this,
/// `unacked_intents()` is exactly the intents after `acked`.
fn predicting_client(intents: &[Vec<u8>], acked: u64) -> ClientCoreApi {
    let mut core = ClientCoreApi::new();
    core.connect();
    core.accept_welcome(0);
    intents.iter().for_each(|payload| {
        core.next_intent(0, 0, payload).unwrap();
    });
    core.set_predict_local_player(true);
    // The authoritative snapshot acks `acked` intents (advancing to tick `acked`).
    assert!(core.accept_snapshot(acked, acked));
    core
}

#[test]
fn a_predicting_client_reconciles_to_byte_identical_physics_state() {
    let intents = intent_stream();
    let acked = 2u64; // intents 1,2 acknowledged; 3,4,5 still unacked (to replay).

    // The authority applies the WHOLE stream directly.
    let authority = run_through(&intents, intents.len());
    let authority_bytes = serialize(&authority);

    // The predicting client snaps to the authoritative snapshot at tick `acked`
    // (the world after the acked prefix) and resimulates its unacked intents.
    let core = predicting_client(&intents, acked);
    assert_eq!(core.unacked_intents().len(), intents.len() - acked as usize);

    let baseline = run_through(&intents, acked as usize);
    let baseline_bytes = serialize(&baseline);

    // Replay the unacked intents over the baseline through the SAME fixed step.
    let mut tick = acked;
    let predicted = core.resimulate(baseline, |mut world, payload| {
        apply_and_step(&mut world, payload, tick);
        tick += 1;
        world
    });
    let predicted_bytes = serialize(&predicted);

    // The reconciled prediction equals the authority's full state, byte-for-byte —
    // reconciliation drift is exactly zero.
    assert_eq!(
        predicted_bytes, authority_bytes,
        "predicted state must match the authority byte-for-byte"
    );
    // The proof is non-vacuous: replaying genuinely advanced past the snapshot.
    assert_ne!(
        predicted_bytes, baseline_bytes,
        "the unacked intents must actually move the state"
    );
}

#[test]
fn prediction_off_leaves_the_authoritative_baseline_untouched() {
    let intents = intent_stream();
    let acked = 2u64;

    // Same client, but prediction OFF (the default behaviour we must preserve).
    let mut core = ClientCoreApi::new();
    core.connect();
    core.accept_welcome(0);
    intents.iter().for_each(|payload| {
        core.next_intent(0, 0, payload).unwrap();
    });
    assert!(core.accept_snapshot(acked, acked));
    assert!(!core.predicts_local_player());

    let baseline = run_through(&intents, acked as usize);
    let baseline_bytes = serialize(&baseline);

    // With prediction off, resimulate is the identity: the closure is never invoked,
    // so the result equals the authoritative baseline exactly.
    let result = core.resimulate(baseline, |mut world, payload| {
        apply_and_step(&mut world, payload, acked);
        world
    });
    assert_eq!(serialize(&result), baseline_bytes);
}

#[test]
fn reconciliation_is_stable_across_runs() {
    // The whole reconciliation is deterministic: a second run reaches the identical
    // predicted bytes (drift = 0 across runs as well as across deployments).
    let run = || {
        let intents = intent_stream();
        let core = predicting_client(&intents, 2);
        let baseline = run_through(&intents, 2);
        let mut tick = 2u64;
        let predicted = core.resimulate(baseline, |mut world, payload| {
            apply_and_step(&mut world, payload, tick);
            tick += 1;
            world
        });
        serialize(&predicted)
    };
    assert_eq!(run(), run());
}
