//! End-to-end physics-backed penalty-kick slice, driving only the public
//! [`PhysicalAnimationApi`] + `AnimationAuthoringApi` facades.
//!
//! Proves the authored penalty motion drives the real `axiom-physics` engine
//! deterministically: the ball is impulse-driven toward the net, the plant foot
//! is held, drive ordering is honored, and the pose path still works alongside.
//!
//! `PhysicalAnimationFrame` is intentionally not nameable from outside the crate
//! (the module publishes exactly one facade), so every frame here is held by
//! inference from `advance`.

use axiom_animation_authoring::{AnimationAuthoringApi, PlanId};
use axiom_kernel::{Ratio, Tick};
use axiom_math::Vec3;
use axiom_physical_animation::PhysicalAnimationApi;

fn ratio(v: f32) -> Ratio {
    Ratio::new(v).unwrap()
}

/// Author + compile the built-in penalty kick at `power`, and return a bound,
/// ball-attached controller ready to advance.
fn ready(power: f32) -> (AnimationAuthoringApi, PlanId, PhysicalAnimationApi) {
    let mut authoring = AnimationAuthoringApi::new();
    let motion = authoring.soccer_penalty_kick_v0(ratio(power));
    let plan = authoring.compile(motion).unwrap();
    let mut sim = PhysicalAnimationApi::new();
    sim.bind_standard_humanoid(&authoring, plan).unwrap();
    sim.attach_ball(&authoring, plan).unwrap();
    (authoring, plan, sim)
}

#[test]
fn the_pose_path_and_the_physics_path_coexist() {
    let (authoring, plan, mut sim) = ready(0.7);
    // The pose (kinematic) path still works untouched.
    assert!(authoring.sample(plan, Tick::new(10)).is_ok());
    // The physics path advances and reports a ball.
    let frame = sim.advance(&authoring, plan, Tick::new(0)).unwrap();
    assert_eq!(sim.frame_tick(&frame), Tick::new(0));
    assert!(sim.frame_ball_transform(&frame).is_some());
}

#[test]
fn two_identical_penalty_simulations_yield_the_same_ball_velocity_after_strike() {
    let (a_auth, a_plan, mut a) = ready(0.7);
    let (b_auth, b_plan, mut b) = ready(0.7);
    let af = (0..40)
        .map(|t| a.advance(&a_auth, a_plan, Tick::new(t)).unwrap())
        .last()
        .unwrap();
    let bf = (0..40)
        .map(|t| b.advance(&b_auth, b_plan, Tick::new(t)).unwrap())
        .last()
        .unwrap();
    assert_eq!(
        a.frame_ball_velocity(&af).unwrap(),
        b.frame_ball_velocity(&bf).unwrap(),
        "identical inputs -> identical ball velocity"
    );
}

#[test]
fn the_strike_drives_the_ball_toward_the_net_with_a_real_impulse() {
    let (authoring, plan, mut sim) = ready(0.7);
    let strike = (0..39)
        .map(|t| sim.advance(&authoring, plan, Tick::new(t)).unwrap())
        .last()
        .unwrap();
    let dir_to_net = Vec3::new(0.0, 0.8, 8.0).subtract(Vec3::new(0.0, 0.0, 0.0));
    let vel = sim.frame_ball_velocity(&strike).unwrap();
    assert!(
        sim.frame_ball_impulse(&strike).is_some(),
        "a real impulse was applied"
    );
    assert!(
        vel.dot(dir_to_net) > 0.0,
        "ball velocity points toward the net"
    );
    assert!(
        vel.length() > 1.0,
        "the strike imparted real speed (not a teleport)"
    );
}

#[test]
fn stronger_power_yields_a_faster_ball() {
    let (la, lp, mut low) = ready(0.2);
    let (ha, hp, mut high) = ready(0.9);
    let lf = (0..39)
        .map(|t| low.advance(&la, lp, Tick::new(t)).unwrap())
        .last()
        .unwrap();
    let hf = (0..39)
        .map(|t| high.advance(&ha, hp, Tick::new(t)).unwrap())
        .last()
        .unwrap();
    let slow = low.frame_ball_velocity(&lf).unwrap().length();
    let fast = high.frame_ball_velocity(&hf).unwrap().length();
    assert!(fast > slow, "more power -> faster ball");
}

#[test]
fn the_plant_phase_holds_the_left_foot_body_at_the_plant_spot() {
    let (authoring, plan, mut sim) = ready(0.7);
    let frame = (0..17)
        .map(|t| sim.advance(&authoring, plan, Tick::new(t)).unwrap())
        .last()
        .unwrap();
    let foot = sim
        .frame_body_transform(&frame, "left_foot")
        .unwrap()
        .translation;
    assert!(foot.distance(Vec3::new(0.25, 0.0, -0.1)) < 1.0e-4);
    assert!(sim.frame_foot_plant(&frame).is_some());
    assert_eq!(sim.frame_phase_name(&frame).as_deref(), Some("plant"));
}

#[test]
fn the_follow_through_drives_the_right_foot_past_the_ball() {
    let (authoring, plan, mut sim) = ready(0.7);
    let frame = (0..51)
        .map(|t| sim.advance(&authoring, plan, Tick::new(t)).unwrap())
        .last()
        .unwrap();
    let foot = sim
        .frame_body_transform(&frame, "right_foot")
        .unwrap()
        .translation;
    assert!(
        foot.z > 0.0,
        "the right foot has swung past the ball toward the net"
    );
    assert_eq!(
        sim.frame_phase_name(&frame).as_deref(),
        Some("follow_through")
    );
}

#[test]
fn strike_drive_exceeds_backswing_and_recover_drive() {
    let (authoring, plan, mut sim) = ready(0.7);
    let mut backswing = 0.0;
    let mut strike = 0.0;
    let mut recover = 0.0;
    (0..57).for_each(|t| {
        let f = sim.advance(&authoring, plan, Tick::new(t)).unwrap();
        (t == 26).then(|| backswing = sim.frame_motor_drive(&f).get());
        (t == 38).then(|| strike = sim.frame_motor_drive(&f).get());
        (t == 56).then(|| recover = sim.frame_motor_drive(&f).get());
    });
    assert!(strike > backswing, "strike drives harder than the wind-up");
    assert!(strike > recover, "strike drives harder than the settle");
}
