//! End-to-end integration tests driving `axiom-physics` only through its single
//! public facade, [`PhysicsApi`], plus its identity vocabulary
//! ([`PhysicsBodyHandle`], [`PhysicsColliderHandle`]).
//!
//! These are the module's behavioral proofs (spec §21): determinism, stable
//! handle ordering, gravity behavior per body kind, FIFO command application,
//! deterministic records/snapshots, typed validation failures, capacity limits,
//! the "no collision events in Phase 1" invariant, and the empty/no-hit query
//! scaffolds. They also fully exercise `physics_world.rs`, the one source file
//! with no inline unit tests — every world path is reached from here.
//!
//! Tests are exempt from the Branchless Law, so this file uses ordinary control
//! flow. Scalars cross the facade as kernel/math value types (`Ratio`, `Meters`,
//! `Vec3`, `Transform`) — never naked floats — exactly as production callers
//! must. The rich return types (snapshot, step record, material) are *sealed*
//! (returned by-value, never re-exported), so the tests never name them: type
//! inference carries them, and behavior is read through their public accessors.

use axiom_kernel::{FrameIndex, Meters, Ratio, Tick};
use axiom_math::{Transform, Vec3};
use axiom_physics::{PhysicsApi, PhysicsBodyHandle, PhysicsColliderHandle};
use axiom_runtime::RuntimeStep;

/// A deterministic fixed step of `nanos` nanoseconds (frame/tick/sequence are
/// irrelevant to physics, which reads only the fixed delta).
fn step_of(nanos: u64) -> RuntimeStep {
    RuntimeStep::new(FrameIndex::new(0), Tick::new(0), nanos, 0)
}

/// A 0.1-second step — a clean `dt` for the linear integrator.
fn tenth_second() -> RuntimeStep {
    step_of(100_000_000)
}

fn ratio(v: f32) -> Ratio {
    Ratio::new(v).unwrap()
}

fn meters(v: f32) -> Meters {
    Meters::new(v).unwrap()
}

// ----------------------------------------------------------------------------
// determinism + stable handle ordering
// ----------------------------------------------------------------------------

#[test]
fn identical_inputs_produce_byte_identical_snapshots_and_records() {
    // A fixed scenario built as a closure so its sealed snapshot/record return
    // types stay inferred (never named). Running it twice must agree exactly.
    let run = || {
        let mut api = PhysicsApi::new();
        let ground = api.create_static_body(Transform::IDENTITY).unwrap();
        let ball = api
            .create_dynamic_body(
                Transform::from_translation(Vec3::new(0.0, 5.0, 0.0)),
                ratio(2.0),
            )
            .unwrap();
        let _platform = api.create_kinematic_body(Transform::IDENTITY).unwrap();

        let material = PhysicsApi::material(ratio(0.4), ratio(0.2), ratio(1.0)).unwrap();
        api.attach_sphere_collider(ball, meters(0.5), material, false)
            .unwrap();
        api.attach_box_collider(ground, Vec3::new(10.0, 0.5, 10.0), material, false)
            .unwrap();

        api.apply_force(ball, Vec3::new(1.0, 0.0, 0.0)).unwrap();
        api.apply_impulse(ball, Vec3::new(0.0, 0.0, 0.5)).unwrap();
        api.step(tenth_second()).unwrap();
        (api.snapshot(), api.latest_step_record())
    };

    let (snap_a, rec_a) = run();
    let (snap_b, rec_b) = run();
    assert_eq!(snap_a, snap_b, "identical inputs must replay byte-equal");
    assert_eq!(rec_a, rec_b, "step records must be deterministic");
}

#[test]
fn handles_are_allocated_in_stable_increasing_order() {
    let mut api = PhysicsApi::new();
    let a = api.create_static_body(Transform::IDENTITY).unwrap();
    let b = api
        .create_dynamic_body(Transform::IDENTITY, ratio(1.0))
        .unwrap();
    let c = api.create_kinematic_body(Transform::IDENTITY).unwrap();
    assert_eq!(a.raw(), 1);
    assert_eq!(b.raw(), 2);
    assert_eq!(c.raw(), 3);
    assert!(a < b && b < c, "handles must be monotonically increasing");
    assert_ne!(a, PhysicsBodyHandle::NULL);
}

#[test]
fn collider_handles_are_allocated_in_stable_increasing_order() {
    let mut api = PhysicsApi::new();
    let body = api.create_static_body(Transform::IDENTITY).unwrap();
    let material = PhysicsApi::material(ratio(0.0), ratio(0.0), ratio(1.0)).unwrap();
    let first = api
        .attach_sphere_collider(body, meters(1.0), material, false)
        .unwrap();
    let second = api
        .attach_box_collider(body, Vec3::ONE, material, true)
        .unwrap();
    assert_eq!(first.raw(), 1);
    assert_eq!(second.raw(), 2);
    assert!(first < second);
    assert_ne!(first, PhysicsColliderHandle::NULL);
}

// ----------------------------------------------------------------------------
// gravity behavior per body kind (linear integrator)
// ----------------------------------------------------------------------------

#[test]
fn dynamic_body_falls_under_gravity() {
    let mut api = PhysicsApi::new();
    let ball = api
        .create_dynamic_body(Transform::IDENTITY, ratio(1.0))
        .unwrap();
    api.step(tenth_second()).unwrap();
    let snap = api.snapshot();
    let body = snap
        .bodies()
        .iter()
        .find(|b| b.handle() == ball)
        .expect("ball present in snapshot");
    assert!(
        body.linear_velocity().y < 0.0,
        "gravity must accelerate the body downward"
    );
    assert!(
        body.transform().translation.y < 0.0,
        "the body must move downward"
    );
}

#[test]
fn static_body_does_not_move_under_gravity() {
    let mut api = PhysicsApi::new();
    let ground = api.create_static_body(Transform::IDENTITY).unwrap();
    api.step(tenth_second()).unwrap();
    let snap = api.snapshot();
    let body = snap.bodies().iter().find(|b| b.handle() == ground).unwrap();
    assert_eq!(body.transform().translation, Vec3::ZERO);
    assert_eq!(body.linear_velocity(), Vec3::ZERO);
}

#[test]
fn kinematic_body_does_not_move_under_gravity() {
    let mut api = PhysicsApi::new();
    let platform = api.create_kinematic_body(Transform::IDENTITY).unwrap();
    api.step(tenth_second()).unwrap();
    let snap = api.snapshot();
    let body = snap.bodies().iter().find(|b| b.handle() == platform).unwrap();
    assert_eq!(body.transform().translation, Vec3::ZERO);
    assert_eq!(body.linear_velocity(), Vec3::ZERO);
}

// ----------------------------------------------------------------------------
// forces, impulses, and FIFO command application
// ----------------------------------------------------------------------------

#[test]
fn applying_force_changes_dynamic_velocity_deterministically() {
    let velocity_after_force = || {
        let mut api = PhysicsApi::new();
        let ball = api
            .create_dynamic_body(Transform::IDENTITY, ratio(1.0))
            .unwrap();
        api.apply_force(ball, Vec3::new(10.0, 0.0, 0.0)).unwrap();
        api.step(tenth_second()).unwrap();
        api.snapshot()
            .bodies()
            .iter()
            .find(|b| b.handle() == ball)
            .unwrap()
            .linear_velocity()
    };
    let v = velocity_after_force();
    assert!(v.x > 0.0, "a positive x force must produce positive x velocity");
    assert_eq!(v, velocity_after_force(), "force response must be deterministic");
}

#[test]
fn applying_impulse_changes_dynamic_velocity_deterministically() {
    let velocity_after_impulse = || {
        let mut api = PhysicsApi::new();
        let ball = api
            .create_dynamic_body(Transform::IDENTITY, ratio(1.0))
            .unwrap();
        api.apply_impulse(ball, Vec3::new(0.0, 0.0, 5.0)).unwrap();
        api.step(tenth_second()).unwrap();
        api.snapshot()
            .bodies()
            .iter()
            .find(|b| b.handle() == ball)
            .unwrap()
            .linear_velocity()
    };
    let v = velocity_after_impulse();
    assert!(v.z > 0.0, "a positive z impulse must produce positive z velocity");
    assert_eq!(v, velocity_after_impulse(), "impulse response must be deterministic");
}

#[test]
fn commands_are_applied_in_fifo_order() {
    // Enqueue disable-then-enable: the later enable wins -> body ends enabled.
    let mut api = PhysicsApi::new();
    let body = api
        .create_dynamic_body(Transform::IDENTITY, ratio(1.0))
        .unwrap();
    api.disable_body(body).unwrap();
    api.enable_body(body).unwrap();
    api.step(tenth_second()).unwrap();
    assert!(
        api.snapshot().bodies()[0].enabled(),
        "the last command (enable) must win under FIFO"
    );

    // Reverse order: enable-then-disable -> body ends disabled.
    let mut api2 = PhysicsApi::new();
    let body2 = api2
        .create_dynamic_body(Transform::IDENTITY, ratio(1.0))
        .unwrap();
    api2.enable_body(body2).unwrap();
    api2.disable_body(body2).unwrap();
    api2.step(tenth_second()).unwrap();
    assert!(
        !api2.snapshot().bodies()[0].enabled(),
        "the last command (disable) must win under FIFO"
    );
}

#[test]
fn disabled_dynamic_body_does_not_integrate() {
    let mut api = PhysicsApi::new();
    let ball = api
        .create_dynamic_body(Transform::IDENTITY, ratio(1.0))
        .unwrap();
    api.disable_body(ball).unwrap();
    api.step(tenth_second()).unwrap();
    let snap = api.snapshot();
    let body = snap.bodies()[0];
    assert!(!body.enabled());
    assert_eq!(body.transform().translation, Vec3::ZERO);
    // A disabled body is not counted as integrated.
    assert_eq!(api.latest_step_record().integration_count(), 0);
}

// ----------------------------------------------------------------------------
// deterministic step records + snapshots
// ----------------------------------------------------------------------------

#[test]
fn step_record_reports_real_counts_for_a_single_body() {
    // A lone dynamic body with one collider and a queued force: there is no second
    // collider to pair with, so the broad/narrow/solver counts are genuinely zero
    // while the integration/command counts are genuinely one.
    let mut api = PhysicsApi::new();
    let a = api
        .create_dynamic_body(Transform::IDENTITY, ratio(1.0))
        .unwrap();
    let material = PhysicsApi::material(ratio(0.0), ratio(0.0), ratio(1.0)).unwrap();
    api.attach_sphere_collider(a, meters(1.0), material, false)
        .unwrap();
    api.apply_force(a, Vec3::new(1.0, 0.0, 0.0)).unwrap();

    let before = api.latest_step_record();
    assert_eq!(before.step_index(), 0, "no step has run yet");

    api.step(tenth_second()).unwrap();
    let rec = api.latest_step_record();
    assert_eq!(rec.step_index(), 1);
    assert_eq!(rec.body_count(), 1);
    assert_eq!(rec.collider_count(), 1);
    assert_eq!(rec.dynamic_body_count(), 1);
    assert_eq!(rec.command_count(), 1, "one force command was drained");
    assert_eq!(rec.integration_count(), 1, "one enabled dynamic body integrated");
    // A single collider can form no pair, so the collision pipeline does no work
    // and — crucially — no contact is *solved*.
    assert_eq!(rec.broad_phase_pair_count(), 0);
    assert_eq!(rec.contact_pair_count(), 0);
    assert_eq!(rec.solved_contact_count(), 0, "a lone body solves no contact");
    assert_eq!(rec.substep_count(), 1, "the default world runs a single substep");
    // `solver_iteration_count` is the *configured* budget (8), reported every step
    // as metadata — never as proof that any contact was solved.
    assert_eq!(rec.solver_iteration_count(), 8);
    assert!(rec.event_count() >= 1, "at least StepCompleted was emitted");

    // The index advances by exactly one per step.
    api.step(tenth_second()).unwrap();
    assert_eq!(api.latest_step_record().step_index(), 2);
}

#[test]
fn snapshot_lists_bodies_and_colliders_in_insertion_order() {
    let mut api = PhysicsApi::new();
    let first = api.create_static_body(Transform::IDENTITY).unwrap();
    let second = api
        .create_dynamic_body(Transform::IDENTITY, ratio(1.0))
        .unwrap();
    let material = PhysicsApi::material(ratio(0.0), ratio(0.0), ratio(1.0)).unwrap();
    let col = api
        .attach_box_collider(first, Vec3::ONE, material, false)
        .unwrap();
    api.step(tenth_second()).unwrap();

    let snap = api.snapshot();
    assert_eq!(snap.step_index(), 1);
    let handles: Vec<PhysicsBodyHandle> = snap.bodies().iter().map(|b| b.handle()).collect();
    assert_eq!(handles, vec![first, second], "bodies stay in insertion order");
    assert_eq!(snap.colliders().len(), 1);
    let collider = snap.colliders()[0];
    assert_eq!(collider.handle(), col);
    assert_eq!(collider.body(), first);
    assert!(!collider.is_trigger());
    assert!(collider.enabled());
}

// ----------------------------------------------------------------------------
// typed validation failures (no panics for invalid input)
// ----------------------------------------------------------------------------

#[test]
fn non_finite_body_transform_is_rejected() {
    let mut api = PhysicsApi::new();
    let nan = Transform::from_translation(Vec3::new(f32::NAN, 0.0, 0.0));
    assert!(api.create_static_body(nan).is_err());
    assert!(api.create_dynamic_body(nan, ratio(1.0)).is_err());
    assert!(api.create_kinematic_body(nan).is_err());
}

#[test]
fn invalid_mass_is_rejected() {
    let mut api = PhysicsApi::new();
    assert!(
        api.create_dynamic_body(Transform::IDENTITY, ratio(0.0)).is_err(),
        "zero mass must be rejected"
    );
}

#[test]
fn invalid_material_is_rejected() {
    // restitution out of [0, 1]
    assert!(PhysicsApi::material(ratio(0.0), ratio(2.0), ratio(1.0)).is_err());
    // density must be > 0
    assert!(PhysicsApi::material(ratio(0.0), ratio(0.0), ratio(0.0)).is_err());
}

#[test]
fn invalid_collider_shapes_are_rejected() {
    let mut api = PhysicsApi::new();
    let body = api.create_static_body(Transform::IDENTITY).unwrap();
    let material = PhysicsApi::material(ratio(0.0), ratio(0.0), ratio(1.0)).unwrap();
    assert!(api
        .attach_sphere_collider(body, meters(0.0), material, false)
        .is_err());
    assert!(api
        .attach_box_collider(body, Vec3::new(0.0, 1.0, 1.0), material, false)
        .is_err());
    assert!(api
        .attach_capsule_collider(body, meters(0.0), meters(1.0), material, false)
        .is_err());
    assert!(api
        .attach_plane_collider(body, Vec3::ZERO, meters(0.0), material, false)
        .is_err());
}

#[test]
fn valid_capsule_and_plane_colliders_attach() {
    let mut api = PhysicsApi::new();
    let body = api.create_static_body(Transform::IDENTITY).unwrap();
    let material = PhysicsApi::material(ratio(0.0), ratio(0.0), ratio(1.0)).unwrap();
    assert!(api
        .attach_capsule_collider(body, meters(0.5), meters(1.0), material, false)
        .is_ok());
    assert!(api
        .attach_plane_collider(body, Vec3::UNIT_Y, meters(0.0), material, true)
        .is_ok());
    assert_eq!(api.snapshot().colliders().len(), 2);
}

#[test]
fn invalid_body_handle_is_rejected() {
    let mut api = PhysicsApi::new();
    let bogus = PhysicsBodyHandle::from_raw(999);
    assert!(api.apply_force(bogus, Vec3::ZERO).is_err());
    assert!(api.apply_impulse(bogus, Vec3::ZERO).is_err());
    assert!(api.enable_body(bogus).is_err());
    assert!(api.disable_body(bogus).is_err());
}

#[test]
fn force_and_impulse_require_a_dynamic_body() {
    let mut api = PhysicsApi::new();
    let ground = api.create_static_body(Transform::IDENTITY).unwrap();
    let platform = api.create_kinematic_body(Transform::IDENTITY).unwrap();
    assert!(api.apply_force(ground, Vec3::ZERO).is_err());
    assert!(api.apply_impulse(ground, Vec3::ZERO).is_err());
    assert!(api.apply_force(platform, Vec3::ZERO).is_err());
}

#[test]
fn non_finite_force_or_impulse_is_rejected() {
    let mut api = PhysicsApi::new();
    let ball = api
        .create_dynamic_body(Transform::IDENTITY, ratio(1.0))
        .unwrap();
    assert!(api
        .apply_force(ball, Vec3::new(f32::NAN, 0.0, 0.0))
        .is_err());
    assert!(api
        .apply_impulse(ball, Vec3::new(0.0, f32::INFINITY, 0.0))
        .is_err());
}

#[test]
fn zero_length_step_is_rejected() {
    let mut api = PhysicsApi::new();
    api.create_dynamic_body(Transform::IDENTITY, ratio(1.0))
        .unwrap();
    assert!(api.step(step_of(0)).is_err(), "a zero-nanosecond step is invalid");
    // A rejected step must not advance the world.
    assert_eq!(api.latest_step_record().step_index(), 0);
}

// ----------------------------------------------------------------------------
// capacity limits fail deterministically
// ----------------------------------------------------------------------------

#[test]
fn body_capacity_is_enforced() {
    let mut api =
        PhysicsApi::with_config(Vec3::new(0.0, -9.8, 0.0), 8, 1, 1, 1, true).unwrap();
    assert!(api.create_static_body(Transform::IDENTITY).is_ok());
    assert!(
        api.create_static_body(Transform::IDENTITY).is_err(),
        "a second body must exceed max_bodies = 1"
    );
}

#[test]
fn collider_capacity_is_enforced() {
    let mut api =
        PhysicsApi::with_config(Vec3::new(0.0, -9.8, 0.0), 8, 4, 1, 1, true).unwrap();
    let body = api.create_static_body(Transform::IDENTITY).unwrap();
    let material = PhysicsApi::material(ratio(0.0), ratio(0.0), ratio(1.0)).unwrap();
    assert!(api
        .attach_sphere_collider(body, meters(1.0), material, false)
        .is_ok());
    assert!(
        api.attach_box_collider(body, Vec3::ONE, material, false).is_err(),
        "a second collider must exceed max_colliders = 1"
    );
}

#[test]
fn invalid_configuration_is_rejected() {
    // Zero capacities / iterations are invalid.
    assert!(PhysicsApi::with_config(Vec3::ZERO, 0, 1, 1, 1, true).is_err());
    // Non-finite gravity is invalid.
    assert!(PhysicsApi::with_config(Vec3::new(f32::NAN, 0.0, 0.0), 1, 1, 1, 1, true).is_err());
}

#[test]
fn attaching_to_a_missing_body_is_rejected() {
    let mut api = PhysicsApi::new();
    let bogus = PhysicsBodyHandle::from_raw(42);
    let material = PhysicsApi::material(ratio(0.0), ratio(0.0), ratio(1.0)).unwrap();
    assert!(api
        .attach_sphere_collider(bogus, meters(1.0), material, false)
        .is_err());
}

// ----------------------------------------------------------------------------
// Phase-1 invariants: lifecycle-only events, empty queries
// ----------------------------------------------------------------------------

#[test]
fn no_collision_or_trigger_events_are_emitted_yet() {
    let mut api = PhysicsApi::new();
    let a = api
        .create_dynamic_body(Transform::IDENTITY, ratio(1.0))
        .unwrap();
    let b = api.create_static_body(Transform::IDENTITY).unwrap();
    let material = PhysicsApi::material(ratio(0.0), ratio(0.0), ratio(1.0)).unwrap();
    api.attach_sphere_collider(a, meters(1.0), material, false)
        .unwrap();
    api.enable_body(a).unwrap();
    api.disable_body(b).unwrap();
    api.step(tenth_second()).unwrap();

    let events = api.events();
    assert!(!events.is_empty(), "lifecycle events must be recorded");
    // The only event variants that exist are the five lifecycle kinds; assert no
    // contact/collision/trigger/overlap event ever appears (the Phase-1 invariant).
    for event in events {
        let rendered = format!("{event:?}");
        for forbidden in ["Contact", "Collision", "Trigger", "Overlap", "Penetration"] {
            assert!(
                !rendered.contains(forbidden),
                "Phase 1 must emit no `{forbidden}` events, got {rendered}"
            );
        }
    }
    // Exactly the lifecycle kinds we caused: 2 BodyCreated + 1 ColliderAttached
    // + 1 BodyEnabled + 1 BodyDisabled + 1 StepCompleted = 6.
    assert_eq!(events.len(), 6);
}

#[test]
fn bodies_without_colliders_are_not_hit_by_queries() {
    // Queries operate on colliders; a body with no collider attached is invisible
    // to raycasts and overlap tests.
    let mut api = PhysicsApi::new();
    api.create_dynamic_body(Transform::IDENTITY, ratio(1.0))
        .unwrap();
    api.create_static_body(Transform::from_translation(Vec3::new(0.0, 1.0, 0.0)))
        .unwrap();
    api.step(tenth_second()).unwrap();

    assert!(api.raycast(Vec3::ZERO, Vec3::UNIT_X, meters(100.0)).is_none());
    assert!(api.overlap_sphere(Vec3::ZERO, meters(100.0)).is_empty());
}

#[test]
fn raycast_and_overlap_find_a_collidered_body_through_the_facade() {
    let mut api = PhysicsApi::new();
    let target = api
        .create_static_body(Transform::from_translation(Vec3::new(5.0, 0.0, 0.0)))
        .unwrap();
    let material = PhysicsApi::material(ratio(0.0), ratio(0.0), ratio(1.0)).unwrap();
    api.attach_sphere_collider(target, meters(1.0), material, false)
        .unwrap();

    // A ray from the origin down +X strikes the sphere at x = 5.
    assert_eq!(
        api.raycast(Vec3::ZERO, Vec3::UNIT_X, meters(100.0)),
        Some(target)
    );
    // A query sphere around the body overlaps it.
    assert_eq!(
        api.overlap_sphere(Vec3::new(5.0, 0.0, 0.0), meters(0.5)),
        vec![target]
    );
    // Queries never mutate the world.
    let before = api.snapshot();
    let _ = api.raycast(Vec3::ZERO, Vec3::UNIT_X, meters(100.0));
    let _ = api.overlap_sphere(Vec3::ZERO, meters(100.0));
    assert_eq!(before, api.snapshot(), "queries must not mutate world state");
}

#[test]
fn default_world_is_empty_and_unstepped() {
    let api = PhysicsApi::default();
    assert!(api.events().is_empty());
    assert!(api.snapshot().bodies().is_empty());
    assert!(api.snapshot().colliders().is_empty());
    assert_eq!(api.latest_step_record().step_index(), 0);
}

// ----------------------------------------------------------------------------
// contact response: broad phase -> narrow phase -> solver -> correction
// ----------------------------------------------------------------------------

/// A small `1/120 s` step — fine enough for stable resting contact.
fn small_step() -> RuntimeStep {
    step_of(8_333_333)
}

/// A static ground plane (`y = 0`, normal up) plus a unit-radius dynamic sphere
/// dropped from `drop_height`, both using the given restitution.
fn ground_and_ball(
    restitution: f32,
    drop_height: f32,
) -> (PhysicsApi, PhysicsBodyHandle, PhysicsBodyHandle) {
    let mut api = PhysicsApi::new();
    let ground = api.create_static_body(Transform::IDENTITY).unwrap();
    let ground_material = PhysicsApi::material(ratio(0.0), ratio(restitution), ratio(1.0)).unwrap();
    api.attach_plane_collider(ground, Vec3::UNIT_Y, meters(0.0), ground_material, false)
        .unwrap();
    let ball = api
        .create_dynamic_body(
            Transform::from_translation(Vec3::new(0.0, drop_height, 0.0)),
            ratio(1.0),
        )
        .unwrap();
    let ball_material = PhysicsApi::material(ratio(0.0), ratio(restitution), ratio(1.0)).unwrap();
    api.attach_sphere_collider(ball, meters(0.5), ball_material, false)
        .unwrap();
    (api, ground, ball)
}

fn body_y(api: &PhysicsApi, handle: PhysicsBodyHandle) -> f32 {
    api.snapshot()
        .bodies()
        .iter()
        .find(|b| b.handle() == handle)
        .expect("body present")
        .transform()
        .translation
        .y
}

fn body_vy(api: &PhysicsApi, handle: PhysicsBodyHandle) -> f32 {
    api.snapshot()
        .bodies()
        .iter()
        .find(|b| b.handle() == handle)
        .expect("body present")
        .linear_velocity()
        .y
}

#[test]
fn dynamic_sphere_settles_on_static_plane_without_tunnelling() {
    let (mut api, _ground, ball) = ground_and_ball(0.0, 2.0);
    let mut min_y = f32::MAX;
    for _ in 0..800 {
        api.step(small_step()).unwrap();
        min_y = min_y.min(body_y(&api, ball));
    }
    let y = body_y(&api, ball);
    // A 0.5-radius sphere on the plane at y = 0 rests with its centre near y = 0.5.
    assert!((0.4..=0.6).contains(&y), "settled centre y = {y}");
    // It never sinks through the surface.
    assert!(min_y > 0.2, "minimum centre y = {min_y} indicates tunnelling");
    // And it is at rest (no residual vertical velocity).
    assert!(body_vy(&api, ball).abs() < 0.2, "should be at rest");
}

#[test]
fn settling_is_deterministic() {
    let drop = || {
        let (mut api, _g, _ball) = ground_and_ball(0.0, 2.0);
        for _ in 0..800 {
            api.step(small_step()).unwrap();
        }
        api.snapshot()
    };
    assert_eq!(drop(), drop(), "a settling drop must replay byte-identically");
}

#[test]
fn step_record_reports_real_broad_and_contact_counts_while_resting() {
    let (mut api, _ground, _ball) = ground_and_ball(0.0, 0.55);
    for _ in 0..400 {
        api.step(small_step()).unwrap();
    }
    let record = api.latest_step_record();
    // The plane is infinite, so its collider always pairs with the sphere.
    assert_eq!(record.broad_phase_pair_count(), 1);
    // While resting, that pair is genuinely in contact.
    assert_eq!(record.contact_pair_count(), 1);
    // The solver runs the configured iteration count.
    assert_eq!(record.solver_iteration_count(), 8);
}

#[test]
fn a_perfectly_elastic_ball_rebounds_upward() {
    let (mut api, _ground, ball) = ground_and_ball(1.0, 1.0);
    let mut touched_low = false;
    let mut rebounded = false;
    for _ in 0..600 {
        api.step(small_step()).unwrap();
        touched_low |= body_y(&api, ball) < 0.7;
        rebounded |= touched_low && body_vy(&api, ball) > 0.5;
    }
    assert!(rebounded, "an e = 1 ball must rebound upward after contact");
}

#[test]
fn two_separated_bodies_register_no_contact() {
    // A sphere far above the plane: a broad-phase pair (plane is infinite) but no
    // narrow-phase contact.
    let (mut api, _ground, _ball) = ground_and_ball(0.0, 50.0);
    api.step(small_step()).unwrap();
    let record = api.latest_step_record();
    assert_eq!(record.broad_phase_pair_count(), 1);
    assert_eq!(record.contact_pair_count(), 0);
}
