//! Behavioral proofs for the **dynamic vs dynamic** contact solver, driven only
//! through the public [`PhysicsApi`] facade and a real `step()`.
//!
//! These exercise the sequential-impulse solver on two *moving* bodies (so both
//! sides carry a non-zero inverse mass) — momentum exchange, inverse-mass impulse
//! splitting, restitution behaviour, penetration separation, determinism, and a
//! two-sphere resting stack on a static plane. Velocity is injected through
//! `apply_impulse` (the only public way to set motion); the collision then
//! redistributes it. Tests are exempt from the Branchless Law, so ordinary
//! control flow is used.

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

fn small_step() -> RuntimeStep {
    step_of(8_333_333)
}

fn body_vel(api: &PhysicsApi, h: PhysicsBodyHandle) -> Vec3 {
    api.snapshot()
        .bodies()
        .iter()
        .find(|b| b.handle() == h)
        .expect("body present")
        .linear_velocity()
}

fn body_pos(api: &PhysicsApi, h: PhysicsBodyHandle) -> Vec3 {
    api.snapshot()
        .bodies()
        .iter()
        .find(|b| b.handle() == h)
        .expect("body present")
        .transform()
        .translation
}

/// A gravity-free world holding two overlapping unit-diameter dynamic spheres on
/// the X axis (A at x=0, B at x=0.8 — penetrating, since the radii sum to 1.0).
/// `mass_a`/`mass_b` set the two masses and `restitution` the contact bounciness.
/// A's collider is attached first, so A is contact body A and the normal points
/// +X (A→B).
fn two_spheres(
    mass_a: f32,
    mass_b: f32,
    restitution: f32,
) -> (PhysicsApi, PhysicsBodyHandle, PhysicsBodyHandle) {
    let mut api = PhysicsApi::with_config(Vec3::ZERO, 8, 16, 16, 1, true, ratio(0.0), ratio(0.0)).unwrap();
    let material = PhysicsApi::material(ratio(0.0), ratio(restitution), ratio(1.0)).unwrap();
    let a = api
        .create_dynamic_body(Transform::from_translation(Vec3::new(0.0, 0.0, 0.0)), ratio(mass_a))
        .unwrap();
    let b = api
        .create_dynamic_body(Transform::from_translation(Vec3::new(0.8, 0.0, 0.0)), ratio(mass_b))
        .unwrap();
    api.attach_sphere_collider(a, meters(0.5), material, false)
        .unwrap();
    api.attach_sphere_collider(b, meters(0.5), material, false)
        .unwrap();
    (api, a, b)
}

#[test]
fn dynamic_dynamic_momentum_exchange_through_step() {
    // A is pushed toward the resting B with a +3 impulse; the inelastic collision
    // redistributes it so both bodies move and total momentum is preserved.
    let (mut api, a, b) = two_spheres(1.0, 1.0, 0.0);
    api.apply_impulse(a, Vec3::new(3.0, 0.0, 0.0)).unwrap();
    api.step(tenth_second()).unwrap();

    let va = body_vel(&api, a).x;
    let vb = body_vel(&api, b).x;
    // Equal masses, e=0 -> both converge on the common velocity 1.5.
    assert!((va - 1.5).abs() < 1.0e-3, "A settled at {va}");
    assert!((vb - 1.5).abs() < 1.0e-3, "B settled at {vb}");
    assert!(vb > 0.0, "the struck body must start moving");
    assert!(va < 3.0, "the striking body must be slowed by the collision");
    // Total linear momentum (mass 1 each) equals the 3.0 that was injected.
    assert!((va + vb - 3.0).abs() < 1.0e-3, "momentum {} != 3", va + vb);
}

#[test]
fn unequal_mass_dynamic_pair_splits_impulse_by_inverse_mass() {
    // A (mass 1, light) drives into the heavier resting B (mass 3); the lighter
    // body's velocity changes more, and momentum is conserved.
    let (mut api, a, b) = two_spheres(1.0, 3.0, 0.0);
    api.apply_impulse(a, Vec3::new(4.0, 0.0, 0.0)).unwrap();
    api.step(tenth_second()).unwrap();

    let va = body_vel(&api, a).x;
    let vb = body_vel(&api, b).x;
    // e=0 inelastic: common velocity = (1*4)/(1+3) = 1. So A changes by 3, B by 1.
    let delta_a = (va - 4.0).abs();
    let delta_b = (vb - 0.0).abs();
    assert!(delta_a > delta_b, "lighter body changes more: dA={delta_a}, dB={delta_b}");
    assert!((va - 1.0).abs() < 1.0e-3, "A settled at {va}");
    assert!((vb - 1.0).abs() < 1.0e-3, "B settled at {vb}");
    // Momentum: 1*4 == 1*va + 3*vb.
    assert!((va + 3.0 * vb - 4.0).abs() < 1.0e-3, "momentum not conserved");
}

#[test]
fn dynamic_dynamic_collision_conserves_linear_momentum_when_restitution_is_one() {
    // Perfectly elastic equal-mass head-on: the velocities swap, momentum kept.
    let (mut api, a, b) = two_spheres(1.0, 1.0, 1.0);
    api.apply_impulse(a, Vec3::new(3.0, 0.0, 0.0)).unwrap();
    api.step(tenth_second()).unwrap();

    let va = body_vel(&api, a).x;
    let vb = body_vel(&api, b).x;
    // e=1, equal mass: A stops, B leaves at the full incoming speed.
    assert!(va.abs() < 1.0e-2, "A should stop after an elastic equal-mass hit, got {va}");
    assert!((vb - 3.0).abs() < 1.0e-2, "B should carry the speed, got {vb}");
    assert!((va + vb - 3.0).abs() < 1.0e-3, "momentum {} != 3", va + vb);
}

#[test]
fn dynamic_dynamic_collision_loses_kinetic_energy_when_restitution_is_zero() {
    // The same 3.0 impulse (injected KE = 0.5*1*3^2 = 4.5) but e=0: the inelastic
    // collision must dissipate energy while still conserving momentum.
    let (mut api, a, b) = two_spheres(1.0, 1.0, 0.0);
    api.apply_impulse(a, Vec3::new(3.0, 0.0, 0.0)).unwrap();
    api.step(tenth_second()).unwrap();

    let va = body_vel(&api, a).x;
    let vb = body_vel(&api, b).x;
    let injected_ke = 0.5 * 3.0 * 3.0; // 4.5
    let post_ke = 0.5 * va * va + 0.5 * vb * vb;
    assert!(post_ke < injected_ke - 1.0e-3, "e=0 must lose KE: {post_ke} !< {injected_ke}");
    // Equal mass, e=0 halves the kinetic energy (4.5 -> 2.25).
    assert!((post_ke - 2.25).abs() < 0.05, "post KE {post_ke}");
    assert!((va + vb - 3.0).abs() < 1.0e-3, "momentum still conserved");
}

#[test]
fn two_dynamic_spheres_separate_after_penetrating_contact() {
    // Two interpenetrating spheres at rest (no gravity, no velocity): Baumgarte
    // position correction must push them apart each step until they barely touch.
    let (mut api, a, b) = two_spheres(1.0, 1.0, 0.0);
    let start = body_pos(&api, b).x - body_pos(&api, a).x;
    assert!((start - 0.8).abs() < 1.0e-6, "start separation {start}");
    for _ in 0..400 {
        api.step(tenth_second()).unwrap();
    }
    let end = body_pos(&api, b).x - body_pos(&api, a).x;
    assert!(end > start + 0.1, "spheres must separate: {start} -> {end}");
    // Correction asymptotes to the touching distance (radii sum 1.0, minus slop).
    assert!((0.95..=1.01).contains(&end), "final separation {end}");
    // The push is symmetric: A goes -X and B goes +X about the original midpoint.
    assert!(body_pos(&api, a).x < 0.0 && body_pos(&api, b).x > 0.8);
}

#[test]
fn dynamic_dynamic_collision_replays_deterministically() {
    let run = || {
        let (mut api, a, _b) = two_spheres(1.0, 3.0, 0.5);
        api.apply_impulse(a, Vec3::new(5.0, 0.0, 0.0)).unwrap();
        api.step(tenth_second()).unwrap();
        api.step(tenth_second()).unwrap();
        (api.snapshot(), api.latest_step_record())
    };
    let (snap_a, rec_a) = run();
    let (snap_b, rec_b) = run();
    assert_eq!(snap_a, snap_b, "dynamic/dynamic collision must replay byte-equal");
    assert_eq!(rec_a, rec_b, "step records must be deterministic");
}

#[test]
fn two_dynamic_spheres_settle_into_a_resting_stack() {
    // A static ground plane plus two vertically stacked dynamic spheres. Under
    // gravity they must come to rest in a stable stack (bottom ~0.5, top ~1.5),
    // never sinking through the plane and never going non-finite.
    let mut api = PhysicsApi::with_config(Vec3::new(0.0, -9.8, 0.0), 8, 16, 16, 1, true, ratio(0.0), ratio(0.0)).unwrap();
    let material = PhysicsApi::material(ratio(0.0), ratio(0.0), ratio(1.0)).unwrap();

    let ground = api.create_static_body(Transform::IDENTITY).unwrap();
    api.attach_plane_collider(ground, Vec3::UNIT_Y, meters(0.0), material, false)
        .unwrap();
    let bottom = api
        .create_dynamic_body(Transform::from_translation(Vec3::new(0.0, 0.55, 0.0)), ratio(1.0))
        .unwrap();
    api.attach_sphere_collider(bottom, meters(0.5), material, false)
        .unwrap();
    let top = api
        .create_dynamic_body(Transform::from_translation(Vec3::new(0.0, 1.55, 0.0)), ratio(1.0))
        .unwrap();
    api.attach_sphere_collider(top, meters(0.5), material, false)
        .unwrap();

    let mut min_bottom = f32::MAX;
    for _ in 0..3000 {
        api.step(small_step()).unwrap();
        min_bottom = min_bottom.min(body_pos(&api, bottom).y);
    }

    let by = body_pos(&api, bottom).y;
    let ty = body_pos(&api, top).y;
    assert!((0.4..=0.65).contains(&by), "bottom settled at {by}");
    assert!((1.3..=1.7).contains(&ty), "top settled at {ty}");
    assert!(ty > by, "the stack order must be preserved");
    assert!(min_bottom > 0.2, "the bottom sphere must never sink through the plane");
    assert!(body_vel(&api, bottom).y.abs() < 0.3, "bottom at rest");
    assert!(body_vel(&api, top).y.abs() < 0.3, "top at rest");
    assert!(by.is_finite() && ty.is_finite(), "stack stays finite");
}
