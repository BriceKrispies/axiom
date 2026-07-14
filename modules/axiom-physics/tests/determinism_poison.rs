//! Proofs that finite-but-extreme inputs can never poison stored world state.
//!
//! Validation screens *finiteness* of inputs, but a finite-but-extreme force,
//! impulse, gravity, or large step `dt` can still drive computed velocity or
//! translation to `±inf`. The world must then reject the step
//! (`NonFiniteStepResult`) and atomically roll back — bodies, events, and the
//! command queue all untouched — so a committed snapshot can never carry a
//! non-finite value. Driven only through the public [`PhysicsApi`] facade; the
//! sealed snapshot/error return types are used by inference, never named.

use axiom_kernel::{FrameIndex, Meters, Ratio, Tick};
use axiom_math::{Transform, Vec3};
use axiom_physics::{PhysicsApi, PhysicsBodyHandle};
use axiom_runtime::RuntimeStep;

fn ratio(v: f32) -> Ratio {
    Ratio::new(v).unwrap()
}

fn step_of(nanos: u64) -> RuntimeStep {
    RuntimeStep::new(FrameIndex::new(0), Tick::new(0), nanos, 0)
}

fn tenth_second() -> RuntimeStep {
    step_of(100_000_000)
}

/// `true` iff every body's transform and velocities are finite in the snapshot.
fn snapshot_is_finite(api: &PhysicsApi) -> bool {
    api.snapshot().bodies().iter().all(|b| {
        let t = b.transform().translation;
        let s = b.transform().scale;
        let v = b.linear_velocity();
        let w = b.angular_velocity();
        [t.x, t.y, t.z, s.x, s.y, s.z, v.x, v.y, v.z, w.x, w.y, w.z]
            .iter()
            .all(|f| f.is_finite())
    })
}

/// A gravity-free world with a single unit-mass dynamic body at the origin.
fn lone_dynamic() -> (PhysicsApi, PhysicsBodyHandle) {
    let mut api =
        PhysicsApi::with_config(Vec3::ZERO, 8, 16, 16, 1, true, ratio(0.0), ratio(0.0)).unwrap();
    let body = api
        .create_dynamic_body(Transform::IDENTITY, ratio(1.0))
        .unwrap();
    (api, body)
}

#[test]
fn extreme_finite_impulse_does_not_poison_state() {
    // A single finite impulse (3e38 < f32::MAX) keeps velocity finite, but over a
    // 2-second step the resulting translation (velocity * dt) overflows to +inf.
    let (mut api, body) = lone_dynamic();
    api.apply_impulse(body, Vec3::new(3.0e38, 0.0, 0.0))
        .unwrap();
    let before = api.snapshot();
    let err = api
        .step(step_of(2_000_000_000))
        .expect_err("overflow must be rejected");
    assert!(err.is_non_finite_step_result(), "raw={}", err.raw_code());
    assert_eq!(
        api.snapshot(),
        before,
        "rejected step must not mutate state"
    );
    assert!(snapshot_is_finite(&api));
}

#[test]
fn summed_impulses_cannot_overflow_to_non_finite() {
    // Two impulses each finite (3e38) but whose sum (6e38) overflows the f32
    // accumulator to +inf, driving velocity non-finite on the next step.
    let (mut api, body) = lone_dynamic();
    api.apply_impulse(body, Vec3::new(3.0e38, 0.0, 0.0))
        .unwrap();
    api.apply_impulse(body, Vec3::new(3.0e38, 0.0, 0.0))
        .unwrap();
    let before = api.snapshot();
    let err = api
        .step(tenth_second())
        .expect_err("summed overflow must be rejected");
    assert!(err.is_non_finite_step_result(), "raw={}", err.raw_code());
    assert_eq!(api.snapshot(), before);
    assert!(snapshot_is_finite(&api));
}

#[test]
fn extreme_finite_force_does_not_poison_state() {
    // A finite force (3e38) over a 2-second step produces an acceleration*dt that
    // overflows velocity to +inf.
    let (mut api, body) = lone_dynamic();
    api.apply_force(body, Vec3::new(3.0e38, 0.0, 0.0)).unwrap();
    let before = api.snapshot();
    let err = api
        .step(step_of(2_000_000_000))
        .expect_err("overflow must be rejected");
    assert!(err.is_non_finite_step_result(), "raw={}", err.raw_code());
    assert_eq!(api.snapshot(), before);
    assert!(snapshot_is_finite(&api));
}

#[test]
fn extreme_finite_gravity_does_not_poison_state() {
    // Gravity of -f32::MAX is finite, so the first 1-second step commits (velocity
    // and position both reach -f32::MAX, still finite). The second step adds
    // another -f32::MAX of velocity, overflowing to -inf, and must be rejected.
    let mut api = PhysicsApi::with_config(
        Vec3::new(0.0, -f32::MAX, 0.0),
        8,
        16,
        16,
        1,
        true,
        ratio(0.0),
        ratio(0.0),
    )
    .unwrap();
    let _body = api
        .create_dynamic_body(Transform::IDENTITY, ratio(1.0))
        .unwrap();
    api.step(step_of(1_000_000_000)).unwrap();
    let before = api.snapshot();
    assert!(
        snapshot_is_finite(&api),
        "the first extreme step still commits a finite state"
    );
    let err = api
        .step(step_of(1_000_000_000))
        .expect_err("second step overflows");
    assert!(err.is_non_finite_step_result(), "raw={}", err.raw_code());
    assert_eq!(
        api.snapshot(),
        before,
        "rejected step must not mutate state"
    );
    assert!(snapshot_is_finite(&api));
}

#[test]
fn solver_impulse_overflow_is_rejected_or_kept_finite() {
    // Two penetrating dynamic spheres driven at near-MAX speeds head-on. The solver
    // impulse may overflow; whichever way it resolves, the world must remain honest:
    // either it commits a fully finite state, or it rejects with a non-finite-step
    // error. It must never silently store a poisoned body.
    let mut api =
        PhysicsApi::with_config(Vec3::ZERO, 8, 16, 16, 1, true, ratio(0.0), ratio(0.0)).unwrap();
    let material = PhysicsApi::material(ratio(0.0), ratio(0.0), ratio(1.0)).unwrap();
    let a = api
        .create_dynamic_body(
            Transform::from_translation(Vec3::new(0.0, 0.0, 0.0)),
            ratio(1.0),
        )
        .unwrap();
    let b = api
        .create_dynamic_body(
            Transform::from_translation(Vec3::new(0.8, 0.0, 0.0)),
            ratio(1.0),
        )
        .unwrap();
    api.attach_sphere_collider(a, Meters::new(0.5).unwrap(), material, false)
        .unwrap();
    api.attach_sphere_collider(b, Meters::new(0.5).unwrap(), material, false)
        .unwrap();
    api.apply_impulse(a, Vec3::new(3.0e38, 0.0, 0.0)).unwrap();
    api.apply_impulse(b, Vec3::new(-3.0e38, 0.0, 0.0)).unwrap();

    match api.step(tenth_second()) {
        Ok(()) => assert!(
            snapshot_is_finite(&api),
            "a committed step must be fully finite"
        ),
        Err(e) => assert!(
            e.is_non_finite_step_result(),
            "rejection must be a non-finite-step error"
        ),
    }
}

#[test]
fn failed_extreme_step_does_not_mutate_snapshot() {
    // After a valid committing step, an overflowing step must leave the snapshot
    // exactly as the valid step left it.
    let (mut api, body) = lone_dynamic();
    api.apply_force(body, Vec3::new(2.0, 0.0, 0.0)).unwrap();
    api.step(tenth_second()).unwrap();
    let committed = api.snapshot();

    api.apply_impulse(body, Vec3::new(3.0e38, 0.0, 0.0))
        .unwrap();
    let err = api
        .step(step_of(2_000_000_000))
        .expect_err("overflow must be rejected");
    assert!(err.is_non_finite_step_result(), "raw={}", err.raw_code());
    assert_eq!(
        api.snapshot(),
        committed,
        "the rolled-back state equals the last commit"
    );
    assert!(snapshot_is_finite(&api));
}

#[test]
fn replay_after_extreme_rejected_input_remains_deterministic() {
    // After a *transient* overflow (a huge-dt step that overflows position but
    // queues no command), a following normal step must commit, and two independent
    // worlds must agree byte-for-byte through the whole sequence.
    let scenario = || {
        let mut api =
            PhysicsApi::with_config(Vec3::ZERO, 8, 16, 16, 1, true, ratio(0.0), ratio(0.0))
                .unwrap();
        let body = api
            .create_dynamic_body(Transform::IDENTITY, ratio(1.0))
            .unwrap();
        // Commit a huge but finite velocity with a tiny-dt step (position stays
        // finite because dt is ~1e-9 s).
        api.apply_impulse(body, Vec3::new(2.0e28, 0.0, 0.0))
            .unwrap();
        api.step(step_of(1)).unwrap();
        // A maximal-dt step overflows translation (velocity * dt) -> rejected. No
        // command is queued, so the rollback leaves an empty queue.
        let rejected = api.step(step_of(u64::MAX)).is_err();
        // A normal step now proceeds and commits.
        let recovered = api.step(tenth_second()).is_ok();
        (
            rejected,
            recovered,
            api.snapshot(),
            api.latest_step_record(),
        )
    };
    let a = scenario();
    let b = scenario();
    assert!(a.0, "the huge-dt step must be rejected");
    assert!(a.1, "the following normal step must commit");
    assert_eq!(a.2, b.2, "post-recovery snapshots must match");
    assert_eq!(a.3, b.3, "post-recovery records must match");
}

#[test]
fn snapshot_never_contains_non_finite_values_after_public_operations() {
    // A mix of valid and rejected operations; the committed snapshot must remain
    // finite throughout.
    let (mut api, body) = lone_dynamic();
    api.apply_force(body, Vec3::new(5.0, -3.0, 1.0)).unwrap();
    api.step(tenth_second()).unwrap();
    assert!(snapshot_is_finite(&api));

    // A rejected overflow step in the middle (extreme impulse over a 2-second dt)
    // must not corrupt anything.
    api.apply_impulse(body, Vec3::new(3.0e38, 0.0, 0.0))
        .unwrap();
    assert!(api.step(step_of(2_000_000_000)).is_err());
    assert!(snapshot_is_finite(&api));

    // The impulse command persists across the rejection; over a normal dt it now
    // commits a finite (if extreme) velocity, and the snapshot stays finite.
    api.step(tenth_second()).unwrap();
    assert!(
        snapshot_is_finite(&api),
        "committing an extreme-but-finite value stays finite"
    );
}

#[test]
fn overlapping_kinematic_bodies_never_wedge_the_world() {
    // Two kinematic character bodies (zero inverse mass and inertia) whose
    // sphere colliders overlap while approaching — the immovable-pair contact
    // used to overflow the solver's floored effective mass to `inf`, whose
    // zero normal components became `NaN`, so EVERY step was rejected and the
    // whole world froze while the app kept mirroring the characters. The
    // solver's movable gate must keep such steps committing: the kinematic
    // pair is untouched and a dynamic body elsewhere keeps integrating.
    let mut api = PhysicsApi::with_config(
        Vec3::new(0.0, -9.8, 0.0),
        8,
        16,
        16,
        4,
        true,
        ratio(0.0),
        ratio(0.0),
    )
    .unwrap();
    let material = PhysicsApi::material(ratio(0.5), ratio(0.3), ratio(1.0)).unwrap();
    let a = api
        .create_kinematic_body(Transform::from_translation(Vec3::new(0.0, 1.0, 0.0)))
        .unwrap();
    let b = api
        .create_kinematic_body(Transform::from_translation(Vec3::new(0.6, 1.0, 0.0)))
        .unwrap();
    api.attach_sphere_collider(a, Meters::new(0.5).unwrap(), material, false)
        .unwrap();
    api.attach_sphere_collider(b, Meters::new(0.5).unwrap(), material, false)
        .unwrap();
    api.set_body_velocity(a, Vec3::new(2.0, 0.0, 0.0), Vec3::ZERO)
        .unwrap();
    api.set_body_velocity(b, Vec3::new(-2.0, 0.0, 0.0), Vec3::ZERO)
        .unwrap();
    let ball = api
        .create_dynamic_body(
            Transform::from_translation(Vec3::new(10.0, 8.0, 0.0)),
            ratio(1.0),
        )
        .unwrap();
    api.attach_sphere_collider(ball, Meters::new(0.2).unwrap(), material, false)
        .unwrap();

    for n in 0..30 {
        let step = RuntimeStep::new(FrameIndex::new(n), Tick::new(n), 16_666_667, n);
        api.step(step)
            .expect("an immovable-pair contact must not reject the step");
        // The app keeps re-pinning its characters, exactly like a real game loop.
        api.set_body_velocity(a, Vec3::new(2.0, 0.0, 0.0), Vec3::ZERO)
            .unwrap();
        api.set_body_velocity(b, Vec3::new(-2.0, 0.0, 0.0), Vec3::ZERO)
            .unwrap();
    }
    assert!(snapshot_is_finite(&api));
    let snap = api.snapshot();
    let ball_y = snap
        .bodies()
        .iter()
        .find(|body| body.handle() == ball)
        .map(|body| body.transform().translation.y)
        .unwrap();
    assert!(
        ball_y < 7.0,
        "the dynamic body must keep integrating (fell from 8.0, got {ball_y})"
    );
}

#[test]
fn a_perturbed_angular_or_friction_replay_is_detected() {
    // The angular + friction paths are a pure function of world state, so two
    // identical runs agree byte-for-byte — and any perturbation of the angular
    // drive (a different torque) is *detected* as a divergent snapshot/record.
    let run = |torque_y: f32, friction: f32| {
        let mut api = PhysicsApi::with_config(
            Vec3::new(0.0, -9.8, 0.0),
            8,
            16,
            16,
            1,
            true,
            ratio(0.0),
            ratio(0.1),
        )
        .unwrap();
        let material = PhysicsApi::material(ratio(friction), ratio(0.0), ratio(1.0)).unwrap();
        let ground = api.create_static_body(Transform::IDENTITY).unwrap();
        api.attach_plane_collider(
            ground,
            Vec3::new(0.0, 1.0, 0.0),
            Meters::new(0.0).unwrap(),
            material,
            false,
        )
        .unwrap();
        let ball = api
            .create_dynamic_body(
                Transform::from_translation(Vec3::new(0.0, 0.6, 0.0)),
                ratio(1.0),
            )
            .unwrap();
        api.attach_sphere_collider(ball, Meters::new(0.5).unwrap(), material, false)
            .unwrap();
        api.apply_impulse(ball, Vec3::new(2.0, 0.0, 0.0)).unwrap();
        api.apply_torque(ball, Vec3::new(0.0, torque_y, 0.0))
            .unwrap();
        for _ in 0..12 {
            api.step(tenth_second()).unwrap();
        }
        (api.snapshot(), api.latest_step_record())
    };
    // Same inputs replay byte-equal (same-binary determinism over the new paths).
    assert_eq!(
        run(3.0, 0.7),
        run(3.0, 0.7),
        "identical spin+friction runs agree"
    );
    // A perturbed torque is detected as a divergent replay.
    assert_ne!(run(3.0, 0.7), run(3.5, 0.7), "a perturbed torque diverges");
    // A perturbed friction is detected too.
    assert_ne!(
        run(3.0, 0.7),
        run(3.0, 0.1),
        "a perturbed friction diverges"
    );
    // And the snapshots stay finite throughout (no poison from the extra ops).
    let (snap, _) = run(3.0, 0.7);
    assert!(snap.bodies().iter().all(|b| {
        let r = b.transform().rotation;
        [r.x, r.y, r.z, r.w].iter().all(|f| f.is_finite())
    }));
}
