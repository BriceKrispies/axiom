//! Proofs for the semantics of a disabled body across the public facade.
//! A disabled body deterministically rejects force/impulse operations (rather than
//! silently dropping them), does not integrate, and is skipped by the broad phase;
//! re-enabling it restores all of that. A disable/enable becomes effective only
//! once the queued command is drained by a `step()`. Driven only through the public
//! [`PhysicsApi`] facade.

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

fn tenth_second() -> RuntimeStep {
    RuntimeStep::new(FrameIndex::new(0), Tick::new(0), 100_000_000, 0)
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

fn enabled(api: &PhysicsApi, h: PhysicsBodyHandle) -> bool {
    api.snapshot()
        .bodies()
        .iter()
        .find(|b| b.handle() == h)
        .expect("body present")
        .enabled()
}

/// A gravity-free world with a single dynamic body that has been disabled and the
/// disable committed by a step (so the body's `enabled` flag is genuinely false).
fn disabled_world() -> (PhysicsApi, PhysicsBodyHandle) {
    let mut api = PhysicsApi::with_config(Vec3::ZERO, 8, 16, 16, 1, true, ratio(0.0), ratio(0.0)).unwrap();
    let body = api.create_dynamic_body(Transform::IDENTITY, ratio(1.0)).unwrap();
    api.disable_body(body).unwrap();
    api.step(tenth_second()).unwrap();
    assert!(!enabled(&api, body), "the disable must have committed");
    (api, body)
}

#[test]
fn apply_force_to_disabled_body_returns_error() {
    let (mut api, body) = disabled_world();
    let err = api
        .apply_force(body, Vec3::new(1.0, 0.0, 0.0))
        .expect_err("a force on a disabled body must be rejected");
    assert!(err.is_operation_on_disabled_body(), "raw={}", err.raw_code());
}

#[test]
fn apply_impulse_to_disabled_body_returns_error() {
    let (mut api, body) = disabled_world();
    let err = api
        .apply_impulse(body, Vec3::new(1.0, 0.0, 0.0))
        .expect_err("an impulse on a disabled body must be rejected");
    assert!(err.is_operation_on_disabled_body(), "raw={}", err.raw_code());
}

#[test]
fn disabled_dynamic_body_does_not_integrate() {
    // Under gravity, a disabled dynamic body stays exactly put and is not counted
    // as integrated.
    let mut api = PhysicsApi::with_config(Vec3::new(0.0, -9.8, 0.0), 8, 16, 16, 1, true, ratio(0.0), ratio(0.0)).unwrap();
    let body = api.create_dynamic_body(Transform::IDENTITY, ratio(1.0)).unwrap();
    api.disable_body(body).unwrap();
    for _ in 0..10 {
        api.step(tenth_second()).unwrap();
    }
    assert_eq!(pos(&api, body), Vec3::ZERO, "a disabled body must not fall");
    assert_eq!(api.latest_step_record().integration_count(), 0, "nothing integrates");
}

#[test]
fn disabled_body_is_skipped_by_broad_phase() {
    // Two overlapping spheres on different bodies form one broad pair while both are
    // enabled; disabling one removes the pair entirely.
    let mut api = PhysicsApi::with_config(Vec3::ZERO, 8, 16, 16, 1, true, ratio(0.0), ratio(0.0)).unwrap();
    let material = PhysicsApi::material(ratio(0.0), ratio(0.0), ratio(1.0)).unwrap();
    let a = api
        .create_dynamic_body(Transform::from_translation(Vec3::new(0.0, 0.0, 0.0)), ratio(1.0))
        .unwrap();
    let b = api
        .create_static_body(Transform::from_translation(Vec3::new(0.8, 0.0, 0.0)))
        .unwrap();
    api.attach_sphere_collider(a, meters(0.5), material, false).unwrap();
    api.attach_sphere_collider(b, meters(0.5), material, false).unwrap();

    api.step(tenth_second()).unwrap();
    assert_eq!(api.latest_step_record().broad_phase_pair_count(), 1, "both enabled -> one pair");

    api.disable_body(a).unwrap();
    api.step(tenth_second()).unwrap();
    let record = api.latest_step_record();
    assert_eq!(record.broad_phase_pair_count(), 0, "disabled body is skipped by broad phase");
    assert_eq!(record.contact_pair_count(), 0, "and generates no contact");
}

#[test]
fn reenabled_body_accepts_force_again() {
    let (mut api, body) = disabled_world();
    assert!(api.apply_force(body, Vec3::new(1.0, 0.0, 0.0)).is_err());

    api.enable_body(body).unwrap();
    api.step(tenth_second()).unwrap();
    assert!(enabled(&api, body), "the enable must have committed");

    api.apply_force(body, Vec3::new(5.0, 0.0, 0.0)).unwrap();
    api.step(tenth_second()).unwrap();
    assert!(pos(&api, body).x > 0.0, "a re-enabled body responds to force again");
}
