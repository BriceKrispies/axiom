//! Golden proofs for the SPEC-10 rigid-body completions — damping, friction, and
//! angular dynamics — driven only through the public [`PhysicsApi`] facade.
//!
//! Each proof asserts on whole observable values (snapshot velocities, transforms,
//! step-record counts), never on internal state, and every world is fed an
//! explicit, deterministic `RuntimeStep` exactly as a production caller would.

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

/// The linear velocity of `body` in `api`'s current snapshot.
fn linear_velocity(api: &PhysicsApi, body: PhysicsBodyHandle) -> Vec3 {
    api.snapshot()
        .bodies()
        .iter()
        .find(|b| b.handle() == body)
        .expect("body present")
        .linear_velocity()
}

/// The angular velocity of `body` in `api`'s current snapshot.
fn angular_velocity(api: &PhysicsApi, body: PhysicsBodyHandle) -> Vec3 {
    api.snapshot()
        .bodies()
        .iter()
        .find(|b| b.handle() == body)
        .expect("body present")
        .angular_velocity()
}

// ---------------------------------------------------------------------------
// Damping — coast-to-rest, and damping == 0 reproduces today exactly
// ---------------------------------------------------------------------------

#[test]
fn linear_damping_decays_a_coasting_body_monotonically_to_rest() {
    // No gravity, 50% linear damping per step: a shoved body coasts and its speed
    // halves every step, approaching rest.
    let mut api = PhysicsApi::with_config(Vec3::ZERO, 8, 16, 16, 1, true, ratio(0.5), ratio(0.0))
        .unwrap();
    let body = api
        .create_dynamic_body(Transform::IDENTITY, ratio(1.0))
        .unwrap();
    api.apply_impulse(body, Vec3::new(8.0, 0.0, 0.0)).unwrap();

    let mut last = f32::INFINITY;
    let mut speeds = Vec::new();
    for _ in 0..6 {
        api.step(tenth_second()).unwrap();
        let vx = linear_velocity(&api, body).x;
        assert!(vx < last, "speed must strictly decay each step: {vx} !< {last}");
        last = vx;
        speeds.push(vx);
    }
    // First post-step speed: impulse 8 then *0.5 damping = 4; then 2, 1, ...
    assert_eq!(speeds[0], 4.0);
    assert_eq!(speeds[1], 2.0);
    assert!(speeds.last().copied().unwrap() < 0.2, "coasts toward rest");
}

#[test]
fn zero_damping_reproduces_prior_behaviour_exactly() {
    // A world built with explicit zero damping must produce byte-identical results
    // to the default-config world (which has always had no damping). This is the
    // regression guard: damping = 0 changes nothing.
    let scenario = |damped: bool| {
        let mut api = if damped {
            PhysicsApi::with_config(
                Vec3::new(0.0, -9.8, 0.0), 8, 16, 16, 1, true, ratio(0.0), ratio(0.0),
            )
            .unwrap()
        } else {
            PhysicsApi::new()
        };
        let body = api
            .create_dynamic_body(Transform::from_translation(Vec3::new(0.0, 5.0, 0.0)), ratio(2.0))
            .unwrap();
        api.apply_force(body, Vec3::new(3.0, 0.0, 1.0)).unwrap();
        api.apply_impulse(body, Vec3::new(0.0, 0.0, 0.5)).unwrap();
        for _ in 0..20 {
            api.step(tenth_second()).unwrap();
        }
        (api.snapshot(), api.latest_step_record())
    };
    let (snap_zero, rec_zero) = scenario(true);
    let (snap_default, rec_default) = scenario(false);
    assert_eq!(snap_zero, snap_default, "explicit zero damping == default behaviour");
    assert_eq!(rec_zero, rec_default);
}

// ---------------------------------------------------------------------------
// Angular — spin-up under torque, orientation advances, damped decay
// ---------------------------------------------------------------------------

/// A lone dynamic sphere (which gives the body a finite moment of inertia) in a
/// gravity-free world with the given angular damping.
fn lone_spinner(angular_damping: f32) -> (PhysicsApi, PhysicsBodyHandle) {
    let mut api =
        PhysicsApi::with_config(Vec3::ZERO, 8, 16, 16, 1, true, ratio(0.0), ratio(angular_damping))
            .unwrap();
    let body = api
        .create_dynamic_body(Transform::IDENTITY, ratio(1.0))
        .unwrap();
    let material = PhysicsApi::material(ratio(0.0), ratio(0.0), ratio(1.0)).unwrap();
    api.attach_sphere_collider(body, meters(0.5), material, false)
        .unwrap();
    (api, body)
}

#[test]
fn torque_spins_a_body_up_and_advances_its_orientation() {
    let (mut api, body) = lone_spinner(0.0);
    // Rotation starts at identity, angular velocity at rest.
    assert_eq!(angular_velocity(&api, body), Vec3::ZERO);

    api.apply_torque(body, Vec3::new(0.0, 2.0, 0.0)).unwrap();
    api.step(tenth_second()).unwrap();
    let w1 = angular_velocity(&api, body).y;
    assert!(w1 > 0.0, "torque must produce +Y angular velocity, got {w1}");

    let r1 = api.snapshot().bodies()[0].transform().rotation;
    assert!(r1.y > 0.0, "orientation must advance about +Y, got {r1:?}");
    assert!((r1.length() - 1.0).abs() < 1.0e-5, "orientation stays unit");

    // With no further torque and zero damping, angular velocity is conserved and
    // the orientation keeps advancing.
    api.step(tenth_second()).unwrap();
    assert!((angular_velocity(&api, body).y - w1).abs() < 1.0e-6, "spin conserved");
    let r2 = api.snapshot().bodies()[0].transform().rotation;
    assert!(r2.y > r1.y, "orientation continues to advance");
}

#[test]
fn angular_damping_decays_a_spin_monotonically_toward_rest() {
    let (mut api, body) = lone_spinner(0.5);
    // Spin it up with a one-step torque, then let it coast under damping.
    api.apply_torque(body, Vec3::new(0.0, 4.0, 0.0)).unwrap();
    api.step(tenth_second()).unwrap();

    let mut last = angular_velocity(&api, body).y;
    assert!(last > 0.0, "spun up");
    for _ in 0..5 {
        api.step(tenth_second()).unwrap();
        let w = angular_velocity(&api, body).y;
        assert!(w < last, "angular speed must strictly decay: {w} !< {last}");
        assert!(w >= 0.0, "decays toward rest, never reverses");
        last = w;
    }
}

// ---------------------------------------------------------------------------
// Friction — grip vs slide, and the honest frictioned-contact count
// ---------------------------------------------------------------------------

/// A box sliding sideways across a flat floor under gravity, with the given
/// surface friction on both bodies. Returns the world, the box handle, and runs
/// `steps` fixed steps after launching the box with a sideways impulse.
fn sliding_box(friction: f32, steps: u32) -> (PhysicsApi, PhysicsBodyHandle) {
    let mut api = PhysicsApi::new();
    let material = PhysicsApi::material(ratio(friction), ratio(0.0), ratio(1.0)).unwrap();
    let ground = api.create_static_body(Transform::IDENTITY).unwrap();
    api.attach_plane_collider(ground, Vec3::UNIT_Y, meters(0.0), material, false)
        .unwrap();
    let boxed = api
        .create_dynamic_body(Transform::from_translation(Vec3::new(0.0, 0.45, 0.0)), ratio(1.0))
        .unwrap();
    api.attach_box_collider(boxed, Vec3::new(0.5, 0.5, 0.5), material, false)
        .unwrap();
    // Launch it sliding along +X.
    api.apply_impulse(boxed, Vec3::new(6.0, 0.0, 0.0)).unwrap();
    (0..steps).for_each(|_| api.step(tenth_second()).unwrap());
    (api, boxed)
}

#[test]
fn friction_grips_a_sliding_box_but_frictionless_keeps_sliding() {
    // High friction: gravity presses the box into the floor each step, and the
    // per-step Coulomb friction at the below-centre contact both bleeds off the
    // slide *and* — now that a friction impulse carries its contact-lever torque —
    // rolls the box forward about -Z. So the slide is reduced and the box spins up.
    let (gripped, body) = sliding_box(1.0, 12);
    let vx_gripped = linear_velocity(&gripped, body).x;
    let spin_gripped = angular_velocity(&gripped, body).z;
    assert!(vx_gripped < 4.0, "high friction bleeds off the slide, got {vx_gripped}");
    assert!(spin_gripped < 0.0, "friction torque rolls the box forward (-Z), got {spin_gripped}");

    // Frictionless: the slide is conserved (gravity only changes the vertical) and,
    // with no tangential impulse, the box never picks up spin.
    let (frictionless, body) = sliding_box(0.0, 12);
    let vx_free = linear_velocity(&frictionless, body).x;
    assert!(vx_free > 5.5, "a frictionless box keeps sliding, got {vx_free}");
    assert_eq!(angular_velocity(&frictionless, body).z, 0.0, "frictionless never rolls");
    assert!(vx_gripped < vx_free, "friction slows the box more than no friction does");
}

#[test]
fn step_record_reports_an_honest_frictioned_contact_count() {
    // A single sliding box on a frictional floor: while it is still sliding, the
    // record reports one frictioned contact; a frictionless run reports none.
    let mut api = PhysicsApi::new();
    let material = PhysicsApi::material(ratio(0.8), ratio(0.0), ratio(1.0)).unwrap();
    let ground = api.create_static_body(Transform::IDENTITY).unwrap();
    api.attach_plane_collider(ground, Vec3::UNIT_Y, meters(0.0), material, false)
        .unwrap();
    let boxed = api
        .create_dynamic_body(Transform::from_translation(Vec3::new(0.0, 0.45, 0.0)), ratio(1.0))
        .unwrap();
    api.attach_box_collider(boxed, Vec3::new(0.5, 0.5, 0.5), material, false)
        .unwrap();
    api.apply_impulse(boxed, Vec3::new(6.0, 0.0, 0.0)).unwrap();
    // Two steps to settle into contact while still sliding.
    api.step(tenth_second()).unwrap();
    api.step(tenth_second()).unwrap();
    assert_eq!(
        api.latest_step_record().frictioned_contact_count(),
        1,
        "a sliding frictional contact is counted"
    );
    assert!(api.latest_step_record().solved_contact_count() >= 1);

    // The same scene with a frictionless floor reports zero frictioned contacts.
    let mut frictionless = PhysicsApi::new();
    let slick = PhysicsApi::material(ratio(0.0), ratio(0.0), ratio(1.0)).unwrap();
    let g2 = frictionless.create_static_body(Transform::IDENTITY).unwrap();
    frictionless
        .attach_plane_collider(g2, Vec3::UNIT_Y, meters(0.0), slick, false)
        .unwrap();
    let b2 = frictionless
        .create_dynamic_body(Transform::from_translation(Vec3::new(0.0, 0.45, 0.0)), ratio(1.0))
        .unwrap();
    frictionless
        .attach_box_collider(b2, Vec3::new(0.5, 0.5, 0.5), slick, false)
        .unwrap();
    frictionless.apply_impulse(b2, Vec3::new(6.0, 0.0, 0.0)).unwrap();
    frictionless.step(tenth_second()).unwrap();
    frictionless.step(tenth_second()).unwrap();
    assert_eq!(frictionless.latest_step_record().frictioned_contact_count(), 0);
}

// ---------------------------------------------------------------------------
// Same-binary replay determinism for the new paths
// ---------------------------------------------------------------------------

#[test]
fn torque_and_friction_replay_byte_equal_within_one_binary() {
    let scenario = || {
        let (mut api, spinner) = lone_spinner(0.25);
        api.apply_torque(spinner, Vec3::new(0.3, 1.5, -0.2)).unwrap();
        let (mut floor_world, slider) = sliding_box(0.6, 0);
        api.step(tenth_second()).unwrap();
        floor_world.step(tenth_second()).unwrap();
        (
            api.snapshot(),
            api.latest_step_record(),
            floor_world.snapshot(),
            floor_world.latest_step_record(),
            angular_velocity(&api, spinner),
            linear_velocity(&floor_world, slider),
        )
    };
    assert_eq!(scenario(), scenario(), "the angular + friction paths replay byte-equal");
}
