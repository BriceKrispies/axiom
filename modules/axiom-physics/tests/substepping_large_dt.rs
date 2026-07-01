//! Proofs for deterministic substepping of a single fixed step.
//! A step is split into `max_substeps` deterministic substeps so a fast body
//! cannot tunnel through thin geometry in one large jump. Queued commands are
//! applied **once** before substepping; each substep runs the full collision
//! pipeline over a fraction of the step; and exactly one `StepCompleted` is
//! emitted per outer step. Driven only through the public [`PhysicsApi`] facade.

use axiom_kernel::{FrameIndex, Meters, Ratio, Tick};
use axiom_math::{Transform, Vec3};
use axiom_physics::{PhysicsApi, PhysicsBodyHandle};
use axiom_runtime::RuntimeStep;

fn ratio(v: f32) -> Ratio {
    Ratio::new(v).unwrap()
}

fn meters(v: f32) -> Meters {
    Meters::new(v).unwrap()
}

fn step_of(nanos: u64) -> RuntimeStep {
    RuntimeStep::new(FrameIndex::new(0), Tick::new(0), nanos, 0)
}

fn tenth_second() -> RuntimeStep {
    step_of(100_000_000)
}

fn pos(api: &PhysicsApi, h: PhysicsBodyHandle) -> Vec3 {
    api.snapshot()
        .bodies()
        .iter()
        .find(|b| b.handle() == h)
        .expect("body present")
        .transform()
        .translation
}

fn vel(api: &PhysicsApi, h: PhysicsBodyHandle) -> Vec3 {
    api.snapshot()
        .bodies()
        .iter()
        .find(|b| b.handle() == h)
        .expect("body present")
        .linear_velocity()
}

#[test]
fn max_substeps_is_consumed_by_step() {
    let mut api = PhysicsApi::with_config(Vec3::new(0.0, -9.8, 0.0), 8, 16, 16, 5, true, ratio(0.0), ratio(0.0)).unwrap();
    api.create_dynamic_body(Transform::IDENTITY, ratio(1.0)).unwrap();
    api.step(tenth_second()).unwrap();
    assert_eq!(api.latest_step_record().substep_count(), 5, "the configured substeps are run");
}

#[test]
fn max_substeps_one_preserves_existing_behavior() {
    // With one substep, a lone falling body is pure semi-implicit Euler:
    // v = g*dt, then position += v*dt = g*dt^2. Compute the expected value with the
    // identical f32 arithmetic the integrator uses.
    let gravity_y = -10.0_f32;
    let mut api = PhysicsApi::with_config(Vec3::new(0.0, gravity_y, 0.0), 8, 16, 16, 1, true, ratio(0.0), ratio(0.0)).unwrap();
    let body = api.create_dynamic_body(Transform::IDENTITY, ratio(1.0)).unwrap();
    api.step(tenth_second()).unwrap();

    let dt = 100_000_000.0_f32 / 1_000_000_000.0;
    let expected_v = gravity_y * dt;
    let expected_y = expected_v * dt;
    assert!((vel(&api, body).y - expected_v).abs() < 1.0e-6, "vy {}", vel(&api, body).y);
    assert!((pos(&api, body).y - expected_y).abs() < 1.0e-6, "y {}", pos(&api, body).y);
}

#[test]
fn large_step_is_substepped_and_does_not_tunnel_through_plane() {
    // A sphere given a strong downward velocity above a static plane. Over a single
    // huge step it would shoot far below the plane; substepping catches the contact.
    let build = |substeps: u32| {
        let mut api = PhysicsApi::with_config(Vec3::ZERO, 8, 16, 16, substeps, true, ratio(0.0), ratio(0.0)).unwrap();
        let material = PhysicsApi::material(ratio(0.0), ratio(0.0), ratio(1.0)).unwrap();
        let ground = api.create_static_body(Transform::IDENTITY).unwrap();
        api.attach_plane_collider(ground, Vec3::UNIT_Y, meters(0.0), material, false)
            .unwrap();
        let ball = api
            .create_dynamic_body(Transform::from_translation(Vec3::new(0.0, 5.0, 0.0)), ratio(1.0))
            .unwrap();
        api.attach_sphere_collider(ball, meters(0.5), material, false)
            .unwrap();
        api.apply_impulse(ball, Vec3::new(0.0, -10.0, 0.0)).unwrap();
        (api, ball)
    };

    // One substep over a 1-second step: the sphere moves -10 and tunnels through.
    let (mut coarse, ball_c) = build(1);
    coarse.step(step_of(1_000_000_000)).unwrap();
    assert!(pos(&coarse, ball_c).y < 0.0, "coarse run must tunnel below the plane, y={}", pos(&coarse, ball_c).y);

    // 200 substeps over the same step: each sub-move is < the radius, so the sphere
    // is caught at the surface and never passes through.
    let (mut fine, ball_f) = build(200);
    fine.step(step_of(1_000_000_000)).unwrap();
    let y = pos(&fine, ball_f).y;
    assert!(y > 0.0, "substepped run must stay above the plane, y={y}");
    assert!(y < 1.0, "and the sphere must have descended onto it, y={y}");
}

#[test]
fn commands_apply_once_across_substeps() {
    // A single queued impulse must be applied once before substepping, not once per
    // substep. With 4 substeps and mass 1, the velocity must be exactly the impulse.
    let mut api = PhysicsApi::with_config(Vec3::ZERO, 8, 16, 16, 4, true, ratio(0.0), ratio(0.0)).unwrap();
    let body = api.create_dynamic_body(Transform::IDENTITY, ratio(1.0)).unwrap();
    api.apply_impulse(body, Vec3::new(10.0, 0.0, 0.0)).unwrap();
    api.step(tenth_second()).unwrap();
    // Applied 4x it would be 40; applied once it is 10.
    assert!((vel(&api, body).x - 10.0).abs() < 1.0e-4, "vx {} (impulse applied once)", vel(&api, body).x);
}

#[test]
fn step_completed_event_emits_once_for_outer_step() {
    let mut api = PhysicsApi::with_config(Vec3::new(0.0, -9.8, 0.0), 8, 16, 16, 3, true, ratio(0.0), ratio(0.0)).unwrap();
    api.create_dynamic_body(Transform::IDENTITY, ratio(1.0)).unwrap();
    api.drain_events();
    api.step(tenth_second()).unwrap();
    let completed = api
        .drain_events()
        .iter()
        .filter(|e| format!("{e:?}").contains("StepCompleted"))
        .count();
    assert_eq!(completed, 1, "exactly one StepCompleted per outer step, regardless of substeps");
}

#[test]
fn substep_remainder_distribution_is_deterministic() {
    // 3 substeps over 1e9 ns (not divisible by 3): the remainder is distributed
    // deterministically across substeps, so two fresh worlds must agree exactly.
    let run = || {
        let mut api = PhysicsApi::with_config(Vec3::new(0.0, -9.8, 0.0), 8, 16, 16, 3, true, ratio(0.0), ratio(0.0)).unwrap();
        let body = api.create_dynamic_body(Transform::IDENTITY, ratio(1.0)).unwrap();
        api.apply_force(body, Vec3::new(1.0, 0.0, 0.0)).unwrap();
        api.step(step_of(1_000_000_000)).unwrap();
        (api.snapshot(), api.latest_step_record())
    };
    assert_eq!(run(), run(), "remainder distribution must be deterministic");
}

#[test]
fn substepped_world_replays_identically() {
    let run = || {
        let mut api = PhysicsApi::with_config(Vec3::new(0.0, -9.8, 0.0), 8, 16, 16, 6, true, ratio(0.0), ratio(0.0)).unwrap();
        let material = PhysicsApi::material(ratio(0.0), ratio(0.3), ratio(1.0)).unwrap();
        let ground = api.create_static_body(Transform::IDENTITY).unwrap();
        api.attach_plane_collider(ground, Vec3::UNIT_Y, meters(0.0), material, false)
            .unwrap();
        let ball = api
            .create_dynamic_body(Transform::from_translation(Vec3::new(0.0, 1.0, 0.0)), ratio(1.0))
            .unwrap();
        api.attach_sphere_collider(ball, meters(0.5), material, false)
            .unwrap();
        for _ in 0..50 {
            api.step(tenth_second()).unwrap();
        }
        (api.snapshot(), api.latest_step_record())
    };
    assert_eq!(run(), run(), "a substepped collision run must replay byte-equal");
}

#[test]
fn zero_or_invalid_max_substeps_still_fails_validation() {
    assert!(
        PhysicsApi::with_config(Vec3::new(0.0, -9.8, 0.0), 8, 16, 16, 0, true, ratio(0.0), ratio(0.0)).is_err(),
        "max_substeps = 0 must be rejected"
    );
}
