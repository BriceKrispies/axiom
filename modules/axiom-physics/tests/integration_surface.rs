//! Proofs for the app-facing *integration surface*: neutral collider geometry on
//! a `ColliderSnapshot`, neutral contact reports from the most recent step, and
//! machine-stable error inspection — all without naming any sealed internal type.
//!
//! An app reading the physics world must be able to learn a collider's shape and
//! dimensions, read the latest contacts (normal, depth, point), and branch on an
//! error's stable code, using only the public predicate/accessor surface. Driven
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

fn tenth_second() -> RuntimeStep {
    RuntimeStep::new(FrameIndex::new(0), Tick::new(0), 100_000_000, 0)
}

/// Attach one collider of each shape and return the world for snapshot inspection.
fn world_with_one(shape: &str) -> PhysicsApi {
    let mut api = PhysicsApi::new();
    let body = api.create_static_body(Transform::IDENTITY).unwrap();
    let mat = PhysicsApi::material(ratio(0.0), ratio(0.0), ratio(1.0)).unwrap();
    match shape {
        "sphere" => {
            api.attach_sphere_collider(body, meters(2.0), mat, false).unwrap();
        }
        "box" => {
            api.attach_box_collider(body, Vec3::new(1.0, 2.0, 3.0), mat, false).unwrap();
        }
        "capsule" => {
            api.attach_capsule_collider(body, meters(0.5), meters(1.5), mat, false).unwrap();
        }
        _ => {
            api.attach_plane_collider(body, Vec3::UNIT_Y, meters(5.0), mat, false).unwrap();
        }
    }
    api
}

#[test]
fn collider_snapshot_exposes_sphere_radius() {
    let api = world_with_one("sphere");
    let snap = api.snapshot();
    let shape = snap.colliders()[0].shape();
    assert!(shape.is_sphere());
    assert_eq!(shape.sphere_radius().unwrap().get(), 2.0);
    assert!(shape.box_half_extents().is_none(), "a sphere has no box extents");
}

#[test]
fn collider_snapshot_exposes_box_half_extents() {
    let api = world_with_one("box");
    let snap = api.snapshot();
    let shape = snap.colliders()[0].shape();
    assert!(shape.is_box());
    assert_eq!(shape.box_half_extents().unwrap(), Vec3::new(1.0, 2.0, 3.0));
    assert!(shape.sphere_radius().is_none(), "a box has no sphere radius");
}

#[test]
fn collider_snapshot_exposes_capsule_dimensions() {
    let api = world_with_one("capsule");
    let snap = api.snapshot();
    let shape = snap.colliders()[0].shape();
    assert!(shape.is_capsule());
    assert_eq!(shape.capsule_radius().unwrap().get(), 0.5);
    assert_eq!(shape.capsule_half_height().unwrap().get(), 1.5);
}

#[test]
fn collider_snapshot_exposes_plane_data() {
    let api = world_with_one("plane");
    let snap = api.snapshot();
    let shape = snap.colliders()[0].shape();
    assert!(shape.is_plane());
    assert_eq!(shape.plane_normal().unwrap(), Vec3::UNIT_Y);
    assert_eq!(shape.plane_distance().unwrap().get(), 5.0);
}

#[test]
fn shape_kind_is_observable_without_naming_internal_enum() {
    let sphere = world_with_one("sphere");
    let boxed = world_with_one("box");
    let s = sphere.snapshot().colliders()[0].shape();
    let b = boxed.snapshot().colliders()[0].shape();
    // Each shape answers true to its own kind and false to the others.
    assert!(s.is_sphere() && !s.is_box() && !s.is_capsule() && !s.is_plane());
    assert!(b.is_box() && !b.is_sphere() && !b.is_capsule() && !b.is_plane());
}

#[test]
fn latest_contact_report_exposes_contact_point_and_normal() {
    // A sphere penetrating a static plane; after a step the contact is reported.
    let mut api = PhysicsApi::with_config(Vec3::new(0.0, -9.8, 0.0), 8, 16, 16, 1, true, ratio(0.0), ratio(0.0)).unwrap();
    let mat = PhysicsApi::material(ratio(0.0), ratio(0.0), ratio(1.0)).unwrap();
    let ground = api.create_static_body(Transform::IDENTITY).unwrap();
    api.attach_plane_collider(ground, Vec3::UNIT_Y, meters(0.0), mat, false).unwrap();
    let ball = api
        .create_dynamic_body(Transform::from_translation(Vec3::new(0.0, 0.4, 0.0)), ratio(1.0))
        .unwrap();
    api.attach_sphere_collider(ball, meters(0.5), mat, false).unwrap();

    api.step(tenth_second()).unwrap();
    let contacts = api.latest_contacts();
    assert_eq!(contacts.len(), 1, "the sphere/plane contact must be reported");
    let report = contacts[0];
    // The plane collider was attached first (body A); the normal points A->B, i.e.
    // up out of the ground toward the sphere.
    assert!(report.normal().y > 0.9, "normal points up, got {:?}", report.normal());
    assert!(report.depth().get() > 0.0, "penetration depth is positive");
    let p = report.point();
    assert!(p.x.is_finite() && p.y.is_finite() && p.z.is_finite(), "finite contact point");
    // The contact names the two real bodies.
    assert!(report.body_a() == ground || report.body_b() == ground);
    assert!(report.body_a() == ball || report.body_b() == ball);
}

#[test]
fn contact_report_order_is_deterministic() {
    // Two spheres resting on a plane produce two sphere/plane contacts; their
    // report order (by ascending collider handle) must be identical across worlds.
    let run = || {
        let mut api = PhysicsApi::with_config(Vec3::new(0.0, -9.8, 0.0), 8, 16, 16, 1, true, ratio(0.0), ratio(0.0)).unwrap();
        let mat = PhysicsApi::material(ratio(0.0), ratio(0.0), ratio(1.0)).unwrap();
        let ground = api.create_static_body(Transform::IDENTITY).unwrap();
        api.attach_plane_collider(ground, Vec3::UNIT_Y, meters(0.0), mat, false).unwrap();
        let left = api
            .create_dynamic_body(Transform::from_translation(Vec3::new(-3.0, 0.4, 0.0)), ratio(1.0))
            .unwrap();
        api.attach_sphere_collider(left, meters(0.5), mat, false).unwrap();
        let right = api
            .create_dynamic_body(Transform::from_translation(Vec3::new(3.0, 0.4, 0.0)), ratio(1.0))
            .unwrap();
        api.attach_sphere_collider(right, meters(0.5), mat, false).unwrap();
        api.step(tenth_second()).unwrap();
        api.latest_contacts()
            .iter()
            .map(|r| (r.collider_a().raw(), r.collider_b().raw()))
            .collect::<Vec<_>>()
    };
    let a = run();
    let b = run();
    assert_eq!(a.len(), 2, "two contacts expected");
    assert_eq!(a, b, "contact report ordering must be deterministic");
    assert!(a[0] < a[1], "reports are ordered by ascending handle pair");
}

#[test]
fn integration_surface_does_not_mutate_world() {
    let mut api = PhysicsApi::with_config(Vec3::new(0.0, -9.8, 0.0), 8, 16, 16, 1, true, ratio(0.0), ratio(0.0)).unwrap();
    let mat = PhysicsApi::material(ratio(0.0), ratio(0.0), ratio(1.0)).unwrap();
    let ground = api.create_static_body(Transform::IDENTITY).unwrap();
    api.attach_plane_collider(ground, Vec3::UNIT_Y, meters(0.0), mat, false).unwrap();
    let ball = api
        .create_dynamic_body(Transform::from_translation(Vec3::new(0.0, 0.4, 0.0)), ratio(1.0))
        .unwrap();
    api.attach_sphere_collider(ball, meters(0.5), mat, false).unwrap();
    api.step(tenth_second()).unwrap();

    let snap1 = api.snapshot();
    let _ = api.latest_contacts();
    let _ = api.snapshot();
    let snap2 = api.snapshot();
    assert_eq!(snap1, snap2, "read-only inspection must not mutate the world");
}

#[test]
fn public_callers_can_inspect_invalid_mass_error_code() {
    let mut api = PhysicsApi::new();
    let err = api
        .create_dynamic_body(Transform::IDENTITY, ratio(0.0))
        .expect_err("zero mass must be rejected");
    assert!(err.is_invalid_mass());
    assert_eq!(err.raw_code(), 2, "InvalidMass has the documented stable discriminant 2");
}

#[test]
fn public_callers_can_inspect_missing_body_error_code() {
    let mut api = PhysicsApi::new();
    let bogus = PhysicsBodyHandle::from_raw(999);
    let err = api
        .apply_impulse(bogus, Vec3::ZERO)
        .expect_err("an unknown body must be rejected");
    assert!(err.is_body_not_found());
    assert_eq!(err.raw_code(), 5, "BodyNotFound has the documented stable discriminant 5");
}

#[test]
fn public_callers_can_inspect_non_dynamic_force_error_code() {
    let mut api = PhysicsApi::new();
    let ground = api.create_static_body(Transform::IDENTITY).unwrap();
    let err = api
        .apply_force(ground, Vec3::ZERO)
        .expect_err("a force on a static body must be rejected");
    assert!(err.is_force_on_non_dynamic_body());
    assert_eq!(err.raw_code(), 9, "ForceOnNonDynamicBody has the documented stable discriminant 9");
}

#[test]
fn error_equality_is_deterministic() {
    let mut api = PhysicsApi::new();
    let first = api.create_dynamic_body(Transform::IDENTITY, ratio(0.0)).unwrap_err();
    let second = api.create_dynamic_body(Transform::IDENTITY, ratio(0.0)).unwrap_err();
    assert_eq!(first, second, "the same operation yields equal errors");
}

#[test]
fn error_debug_text_is_not_required_for_logic() {
    // A caller routes purely on the stable code / predicate, never on the human
    // Debug string. We verify the predicate path and that it disagrees with an
    // unrelated error, all without parsing any text.
    let mut api = PhysicsApi::new();
    let mass_err = api.create_dynamic_body(Transform::IDENTITY, ratio(0.0)).unwrap_err();
    let missing_err = api
        .apply_force(PhysicsBodyHandle::from_raw(123), Vec3::ZERO)
        .unwrap_err();
    assert!(mass_err.is_invalid_mass() && !mass_err.is_body_not_found());
    assert!(missing_err.is_body_not_found() && !missing_err.is_invalid_mass());
    assert_ne!(mass_err.raw_code(), missing_err.raw_code(), "distinct codes distinguish them");
}
