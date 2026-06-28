//! Proofs that the step record reports *real* per-step work, not configured
//! metadata dressed up as work.
//!
//! `broad_phase_pair_count`, `contact_pair_count`, `solved_contact_count`, and
//! `substep_count` are genuine per-step totals; `solver_iteration_count` is the
//! configured budget and is reported even when zero contacts are solved. Driven
//! only through the public [`PhysicsApi`] facade.

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

/// A static ground plane plus a dynamic sphere penetrating it at `y`, under
/// gravity. The plane collider is attached first.
fn ball_on_plane(
    y: f32,
    substeps: u32,
    iterations: u32,
) -> (PhysicsApi, PhysicsBodyHandle) {
    let mut api =
        PhysicsApi::with_config(Vec3::new(0.0, -9.8, 0.0), iterations, 16, 16, substeps, true).unwrap();
    let material = PhysicsApi::material(ratio(0.0), ratio(0.0), ratio(1.0)).unwrap();
    let ground = api.create_static_body(Transform::IDENTITY).unwrap();
    api.attach_plane_collider(ground, Vec3::UNIT_Y, meters(0.0), material, false)
        .unwrap();
    let ball = api
        .create_dynamic_body(Transform::from_translation(Vec3::new(0.0, y, 0.0)), ratio(1.0))
        .unwrap();
    api.attach_sphere_collider(ball, meters(0.5), material, false)
        .unwrap();
    (api, ball)
}

#[test]
fn zero_contact_step_reports_zero_solved_contacts() {
    // A lone dynamic body — no second collider, so no contact is solved.
    let mut api = PhysicsApi::new();
    api.create_dynamic_body(Transform::IDENTITY, ratio(1.0)).unwrap();
    api.step(tenth_second()).unwrap();
    let record = api.latest_step_record();
    assert_eq!(record.contact_pair_count(), 0);
    assert_eq!(record.solved_contact_count(), 0, "no contact -> zero solved");
}

#[test]
fn contact_step_reports_nonzero_solved_contacts() {
    // A sphere penetrating the plane and pulled into it by gravity is an
    // approaching contact -> at least one solved contact.
    let (mut api, _ball) = ball_on_plane(0.4, 1, 8);
    api.step(tenth_second()).unwrap();
    let record = api.latest_step_record();
    assert_eq!(record.contact_pair_count(), 1, "the sphere/plane pair is in contact");
    assert!(record.solved_contact_count() >= 1, "an approaching contact must be solved");
}

#[test]
fn solver_config_iteration_count_is_metadata_not_work_proof() {
    // A zero-contact step still reports the configured iteration budget (5) while
    // honestly reporting zero solved contacts.
    let mut api = PhysicsApi::with_config(Vec3::new(0.0, -9.8, 0.0), 5, 16, 16, 1, true).unwrap();
    api.create_dynamic_body(Transform::IDENTITY, ratio(1.0)).unwrap();
    api.step(tenth_second()).unwrap();
    let record = api.latest_step_record();
    assert_eq!(record.solver_iteration_count(), 5, "the configured budget is reported as metadata");
    assert_eq!(record.solved_contact_count(), 0, "but no contact was actually solved");
}

#[test]
fn broad_phase_pair_count_reports_actual_pairs() {
    // The infinite plane always pairs with the finite sphere: exactly one pair,
    // even while the sphere is far above the surface.
    let (mut api, _ball) = ball_on_plane(50.0, 1, 8);
    api.step(tenth_second()).unwrap();
    assert_eq!(api.latest_step_record().broad_phase_pair_count(), 1, "plane pairs with the sphere");

    // A lone body forms no pair at all.
    let mut solo = PhysicsApi::new();
    solo.create_dynamic_body(Transform::IDENTITY, ratio(1.0)).unwrap();
    solo.step(tenth_second()).unwrap();
    assert_eq!(solo.latest_step_record().broad_phase_pair_count(), 0, "a lone body forms no pair");
}

#[test]
fn contact_pair_count_reports_actual_contacts() {
    // Penetrating -> one contact; far apart (a broad pair but no overlap) -> zero.
    let (mut touching, _b1) = ball_on_plane(0.3, 1, 8);
    touching.step(tenth_second()).unwrap();
    assert_eq!(touching.latest_step_record().contact_pair_count(), 1, "penetrating -> one contact");

    let (mut apart, _b2) = ball_on_plane(50.0, 1, 8);
    apart.step(tenth_second()).unwrap();
    let record = apart.latest_step_record();
    assert_eq!(record.broad_phase_pair_count(), 1, "still a broad pair (infinite plane)");
    assert_eq!(record.contact_pair_count(), 0, "but no narrow-phase contact");
}

#[test]
fn substep_count_reports_actual_substeps() {
    let (mut api, _ball) = ball_on_plane(0.4, 4, 8);
    api.step(tenth_second()).unwrap();
    assert_eq!(api.latest_step_record().substep_count(), 4, "the configured substeps are reported");
}
