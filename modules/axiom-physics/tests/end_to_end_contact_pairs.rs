//! End-to-end proofs that each implemented narrow-phase pairing — sphere/sphere,
//! sphere/plane, sphere/box, box/plane — flows through the full `step()` pipeline
//! (broad phase -> narrow phase -> integrate -> solve -> correct) and produces a
//! real, observable contact response on the dynamic body.
//!
//! For every pairing each test asserts: the broad-phase pair count, the step
//! record's contact pair count, a concrete body-state effect on the dynamic body,
//! a finite final state, and a byte-identical replay of the snapshot and record.
//! Driven only through the public [`PhysicsApi`] facade.

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

fn vel(api: &PhysicsApi, h: PhysicsBodyHandle) -> Vec3 {
    api.snapshot()
        .bodies()
        .iter()
        .find(|b| b.handle() == h)
        .expect("body present")
        .linear_velocity()
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

fn all_finite(api: &PhysicsApi) -> bool {
    api.snapshot().bodies().iter().all(|b| {
        let t = b.transform().translation;
        let v = b.linear_velocity();
        [t.x, t.y, t.z, v.x, v.y, v.z].iter().all(|f| f.is_finite())
    })
}

#[test]
fn sphere_sphere_contact_response_through_step() {
    // A static sphere at the origin and a dynamic sphere penetrating it (centres
    // 0.8 apart, radii sum 1.0). With no gravity the only effect is the position
    // correction pushing the dynamic sphere out along +X.
    let build = || {
        let mut api = PhysicsApi::with_config(Vec3::ZERO, 8, 16, 16, 1, true).unwrap();
        let material = PhysicsApi::material(ratio(0.0), ratio(0.0), ratio(1.0)).unwrap();
        let stat = api.create_static_body(Transform::IDENTITY).unwrap();
        let dynamic = api
            .create_dynamic_body(Transform::from_translation(Vec3::new(0.8, 0.0, 0.0)), ratio(1.0))
            .unwrap();
        api.attach_sphere_collider(stat, meters(0.5), material, false)
            .unwrap();
        api.attach_sphere_collider(dynamic, meters(0.5), material, false)
            .unwrap();
        (api, dynamic)
    };
    let (mut api, dynamic) = build();
    api.step(tenth_second()).unwrap();

    let record = api.latest_step_record();
    assert_eq!(record.broad_phase_pair_count(), 1, "the two spheres are one broad pair");
    assert_eq!(record.contact_pair_count(), 1, "and a genuine narrow-phase contact");
    assert!(pos(&api, dynamic).x > 0.8 + 1.0e-4, "the dynamic sphere is pushed out of penetration");
    assert!(all_finite(&api));

    let replay = || {
        let (mut a, _d) = build();
        a.step(tenth_second()).unwrap();
        (a.snapshot(), a.latest_step_record())
    };
    assert_eq!(replay(), replay(), "sphere/sphere step must replay identically");
}

#[test]
fn sphere_plane_contact_response_through_step() {
    // A dynamic sphere penetrating a static ground plane. Gravity pulls it down;
    // the solver must cancel the approaching velocity so it does not free-fall.
    let build = || {
        let mut api = PhysicsApi::with_config(Vec3::new(0.0, -9.8, 0.0), 8, 16, 16, 1, true).unwrap();
        let material = PhysicsApi::material(ratio(0.0), ratio(0.0), ratio(1.0)).unwrap();
        let ground = api.create_static_body(Transform::IDENTITY).unwrap();
        api.attach_plane_collider(ground, Vec3::UNIT_Y, meters(0.0), material, false)
            .unwrap();
        let ball = api
            .create_dynamic_body(Transform::from_translation(Vec3::new(0.0, 0.3, 0.0)), ratio(1.0))
            .unwrap();
        api.attach_sphere_collider(ball, meters(0.5), material, false)
            .unwrap();
        (api, ball)
    };
    let (mut api, ball) = build();
    api.step(tenth_second()).unwrap();

    let record = api.latest_step_record();
    assert_eq!(record.broad_phase_pair_count(), 1);
    assert_eq!(record.contact_pair_count(), 1);
    // Free fall would leave vy = -0.98; the contact cancels it to ~0.
    assert!(vel(&api, ball).y.abs() < 1.0e-3, "contact must cancel gravity, vy={}", vel(&api, ball).y);
    assert!(pos(&api, ball).y > 0.0, "the sphere stays above the plane");
    assert!(all_finite(&api));

    let replay = || {
        let (mut a, _b) = build();
        a.step(tenth_second()).unwrap();
        (a.snapshot(), a.latest_step_record())
    };
    assert_eq!(replay(), replay(), "sphere/plane step must replay identically");
}

#[test]
fn sphere_box_contact_response_through_step() {
    // A dynamic sphere resting on the top face of a static box. The box collider is
    // attached first, so the box is contact body A and the sphere body B.
    let build = || {
        let mut api = PhysicsApi::with_config(Vec3::new(0.0, -9.8, 0.0), 8, 16, 16, 1, true).unwrap();
        let material = PhysicsApi::material(ratio(0.0), ratio(0.0), ratio(1.0)).unwrap();
        let block = api.create_static_body(Transform::IDENTITY).unwrap();
        api.attach_box_collider(block, Vec3::new(1.0, 1.0, 1.0), material, false)
            .unwrap();
        let ball = api
            .create_dynamic_body(Transform::from_translation(Vec3::new(0.0, 1.3, 0.0)), ratio(1.0))
            .unwrap();
        api.attach_sphere_collider(ball, meters(0.5), material, false)
            .unwrap();
        (api, ball)
    };
    let (mut api, ball) = build();
    api.step(tenth_second()).unwrap();

    let record = api.latest_step_record();
    assert_eq!(record.broad_phase_pair_count(), 1);
    assert_eq!(record.contact_pair_count(), 1);
    assert!(vel(&api, ball).y.abs() < 1.0e-3, "the box top cancels gravity, vy={}", vel(&api, ball).y);
    assert!(pos(&api, ball).y > 1.0, "the sphere stays above the box top face");
    assert!(all_finite(&api));

    let replay = || {
        let (mut a, _b) = build();
        a.step(tenth_second()).unwrap();
        (a.snapshot(), a.latest_step_record())
    };
    assert_eq!(replay(), replay(), "sphere/box step must replay identically");
}

#[test]
fn box_plane_contact_response_through_step() {
    // A dynamic box penetrating a static ground plane. The plane collider is
    // attached first (body A); the box is the dynamic body B.
    let build = || {
        let mut api = PhysicsApi::with_config(Vec3::new(0.0, -9.8, 0.0), 8, 16, 16, 1, true).unwrap();
        let material = PhysicsApi::material(ratio(0.0), ratio(0.0), ratio(1.0)).unwrap();
        let ground = api.create_static_body(Transform::IDENTITY).unwrap();
        api.attach_plane_collider(ground, Vec3::UNIT_Y, meters(0.0), material, false)
            .unwrap();
        let crate_body = api
            .create_dynamic_body(Transform::from_translation(Vec3::new(0.0, 0.3, 0.0)), ratio(1.0))
            .unwrap();
        api.attach_box_collider(crate_body, Vec3::new(0.5, 0.5, 0.5), material, false)
            .unwrap();
        (api, crate_body)
    };
    let (mut api, crate_body) = build();
    api.step(tenth_second()).unwrap();

    let record = api.latest_step_record();
    assert_eq!(record.broad_phase_pair_count(), 1);
    assert_eq!(record.contact_pair_count(), 1);
    assert!(vel(&api, crate_body).y.abs() < 1.0e-3, "the plane cancels gravity, vy={}", vel(&api, crate_body).y);
    assert!(pos(&api, crate_body).y > 0.0, "the box stays above the plane");
    assert!(all_finite(&api));

    let replay = || {
        let (mut a, _b) = build();
        a.step(tenth_second()).unwrap();
        (a.snapshot(), a.latest_step_record())
    };
    assert_eq!(replay(), replay(), "box/plane step must replay identically");
}
