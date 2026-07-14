//! End-to-end vertical-slice proofs, driving only the public
//! [`AnimationAuthoringApi`] facade + id vocabulary.
//!
//! These exercise the whole authoring → compile → sample pipeline through the
//! public surface: the standard rig, validation rejections, deterministic
//! sampling, and every authored property of the built-in `soccer_penalty_kick_v0`
//! motion (approach, plant, backswing, strike, follow-through).

use axiom_animation_authoring::{AnimationAuthoringApi, EffectorId, MotionId};
use axiom_kernel::{Ratio, Tick};
use axiom_math::Vec3;

fn ratio(v: f32) -> Ratio {
    Ratio::new(v).unwrap()
}

/// The effector index of `name` in the standard rig (resolved via a scratch rig,
/// whose indices match the one the penalty motion builds internally).
fn effector(api: &mut AnimationAuthoringApi, name: &str) -> EffectorId {
    let rig = api.standard_humanoid();
    api.effector_id(rig, name).unwrap().unwrap()
}

/// Author and compile the built-in penalty kick at `power`, returning the plan.
fn penalty_plan(api: &mut AnimationAuthoringApi, power: f32) -> MotionId {
    api.soccer_penalty_kick_v0(ratio(power))
}

#[test]
fn the_standard_humanoid_hierarchy_is_valid() {
    let mut api = AnimationAuthoringApi::new();
    let rig = api.standard_humanoid();
    assert!(api.rig_is_valid(rig).unwrap());
    assert_eq!(api.joint_names(rig).unwrap().len(), 25);
    assert_eq!(api.effector_names(rig).unwrap().len(), 8);
}

#[test]
fn an_unknown_joint_effector_or_target_is_rejected_at_compile() {
    // Unknown joint.
    let mut a = AnimationAuthoringApi::new();
    let rig = a.standard_humanoid();
    let m = a.create_motion("k", Tick::new(10), rig).unwrap();
    let ph = a.add_phase(m, "p", Tick::new(0), Tick::new(10)).unwrap();
    a.add_set_joint_rotation(ph, "no_such_joint", Vec3::ZERO)
        .unwrap();
    assert!(a.compile(m).is_err());

    // Unknown effector.
    let mut b = AnimationAuthoringApi::new();
    let rig = b.standard_humanoid();
    let m = b.create_motion("k", Tick::new(10), rig).unwrap();
    let ph = b.add_phase(m, "p", Tick::new(0), Tick::new(10)).unwrap();
    b.add_aim_effector_at_target(ph, "no_such_effector", "ball")
        .unwrap();
    b.add_target(m, "ball", Vec3::ZERO).unwrap();
    assert!(b.compile(m).is_err());

    // Unknown target.
    let mut c = AnimationAuthoringApi::new();
    let rig = c.standard_humanoid();
    let m = c.create_motion("k", Tick::new(10), rig).unwrap();
    let ph = c.add_phase(m, "p", Tick::new(0), Tick::new(10)).unwrap();
    c.add_leg_strike(ph, true, "no_such_target").unwrap();
    assert!(c.compile(m).is_err());
}

#[test]
fn an_invalid_phase_range_is_rejected() {
    let mut api = AnimationAuthoringApi::new();
    let rig = api.standard_humanoid();
    let m = api.create_motion("k", Tick::new(10), rig).unwrap();
    // Empty span (start == end).
    api.add_phase(m, "bad", Tick::new(5), Tick::new(5)).unwrap();
    assert!(api.compile(m).is_err());
}

#[test]
fn overlapping_phases_are_rejected() {
    let mut api = AnimationAuthoringApi::new();
    let rig = api.standard_humanoid();
    let m = api.create_motion("k", Tick::new(30), rig).unwrap();
    api.add_phase(m, "a", Tick::new(0), Tick::new(15)).unwrap();
    api.add_phase(m, "b", Tick::new(10), Tick::new(20)).unwrap();
    assert!(api.compile(m).is_err());
}

#[test]
fn a_non_finite_authored_value_is_rejected() {
    let mut api = AnimationAuthoringApi::new();
    let rig = api.standard_humanoid();
    let m = api.create_motion("k", Tick::new(10), rig).unwrap();
    api.add_target(m, "bad", Vec3::new(f32::NAN, 0.0, 0.0))
        .unwrap();
    assert!(api.compile(m).is_err());
}

#[test]
fn the_same_plan_sampled_twice_at_the_same_tick_is_identical() {
    let mut api = AnimationAuthoringApi::new();
    let m = penalty_plan(&mut api, 0.7);
    let plan = api.compile(m).unwrap();
    let a = api.sample(plan, Tick::new(30)).unwrap();
    let b = api.sample(plan, Tick::new(30)).unwrap();
    // Debug-identical (the replay/debug contract) and structurally equal.
    assert_eq!(format!("{a:?}"), format!("{b:?}"));
    assert_eq!(api.frame_root(&a), api.frame_root(&b));
}

#[test]
fn the_approach_phase_moves_the_root_toward_the_ball() {
    let mut api = AnimationAuthoringApi::new();
    let m = penalty_plan(&mut api, 0.7);
    let plan = api.compile(m).unwrap();
    let ball = Vec3::new(0.0, 0.0, 0.0);
    let early = api
        .frame_root(&api.sample(plan, Tick::new(2)).unwrap())
        .translation;
    let late = api
        .frame_root(&api.sample(plan, Tick::new(10)).unwrap())
        .translation;
    assert!(late.distance(ball) < early.distance(ball));
}

#[test]
fn the_plant_phase_pins_the_left_foot_sole_at_the_plant_spot() {
    let mut api = AnimationAuthoringApi::new();
    let sole = effector(&mut api, "left_foot_sole");
    let m = penalty_plan(&mut api, 0.7);
    let plan = api.compile(m).unwrap();
    let frame = api.sample(plan, Tick::new(16)).unwrap(); // inside the plant phase
    let world = api.frame_effector_world(&frame, sole).unwrap().translation;
    assert!(world.distance(Vec3::new(0.25, 0.0, -0.1)) < 1.0e-5);
}

#[test]
fn the_backswing_places_the_right_foot_behind_the_body() {
    let mut api = AnimationAuthoringApi::new();
    let sole = effector(&mut api, "right_foot_sole");
    let m = penalty_plan(&mut api, 0.7);
    let plan = api.compile(m).unwrap();
    let frame = api.sample(plan, Tick::new(28)).unwrap(); // inside the backswing
    let root = api.frame_root(&frame).translation;
    let foot = api.frame_effector_world(&frame, sole).unwrap().translation;
    // Strike direction is +Z (ball -> net); the drawn-back foot is behind the body.
    assert!(
        foot.z - root.z < 0.0,
        "foot {foot:?} not behind root {root:?}"
    );
}

#[test]
fn the_strike_emits_exactly_one_ball_contact() {
    let mut api = AnimationAuthoringApi::new();
    let m = penalty_plan(&mut api, 0.7);
    let plan = api.compile(m).unwrap();
    let ticks_with_contact: Vec<u64> = (0..60)
        .filter(|&t| {
            api.frame_ball_contact(&api.sample(plan, Tick::new(t)).unwrap())
                .is_some()
        })
        .collect();
    assert_eq!(
        ticks_with_contact.len(),
        1,
        "exactly one ball_contact expected"
    );
    // It fires during the strike phase span [32, 44).
    let at = ticks_with_contact[0];
    assert!(
        (32..44).contains(&at),
        "ball_contact at {at} not in the strike phase"
    );
}

#[test]
fn the_follow_through_moves_the_right_foot_past_the_ball_toward_the_net() {
    let mut api = AnimationAuthoringApi::new();
    let sole = effector(&mut api, "right_foot_sole");
    let m = penalty_plan(&mut api, 0.7);
    let plan = api.compile(m).unwrap();
    let frame = api.sample(plan, Tick::new(50)).unwrap(); // inside the follow-through
    let foot = api.frame_effector_world(&frame, sole).unwrap().translation;
    // The ball sits at z = 0; the net is forward at +Z, so a follow-through foot
    // has swung past the ball toward the net.
    assert!(
        foot.z > 0.0,
        "foot {foot:?} did not pass the ball toward the net"
    );
}

#[test]
fn style_power_changes_the_ball_contact_power_deterministically() {
    let power_at = |p: f32| {
        let mut api = AnimationAuthoringApi::new();
        let m = penalty_plan(&mut api, p);
        let plan = api.compile(m).unwrap();
        // Find the strike frame and read its power.
        (0..60)
            .find_map(|t| {
                let frame = api.sample(plan, Tick::new(t)).unwrap();
                api.frame_ball_contact(&frame)
                    .map(|(_, _, _, power)| power.get())
            })
            .unwrap()
    };
    assert!((power_at(0.2) - 0.2).abs() < 1.0e-6);
    assert!((power_at(0.9) - 0.9).abs() < 1.0e-6);
    assert!(power_at(0.2) < power_at(0.9));
}
