//! The authoring facade's test suite (moved out of `authoring_api.rs` to keep
//! that file inside the engine's 1000-line budget; same pattern as
//! `software_rasterizer/tests.rs`).

use super::*;
use crate::authoring_error_code::AuthoringErrorCode;
use axiom_math::Quat;

fn ratio(v: f32) -> Ratio {
    Ratio::new(v).unwrap()
}

#[test]
fn new_and_default_are_equivalent_empty_registries() {
    let a = AnimationAuthoringApi::new();
    let b = AnimationAuthoringApi::default();
    assert_eq!(format!("{a:?}"), format!("{b:?}"));
}

#[test]
fn a_rig_reports_its_joint_and_effector_names_and_validity() {
    let mut api = AnimationAuthoringApi::new();
    let rig = api.standard_humanoid();
    assert_eq!(rig, RigId::from_raw(0));
    assert_eq!(api.joint_names(rig).unwrap().len(), 25);
    assert_eq!(api.effector_names(rig).unwrap().len(), 8);
    assert!(api.rig_is_valid(rig).unwrap());
    assert_eq!(
        api.joint_id(rig, "root").unwrap(),
        Some(JointId::from_raw(0))
    );
    assert_eq!(api.joint_id(rig, "nope").unwrap(), None);
    assert_eq!(
        api.effector_id(rig, "left_foot_sole").unwrap(),
        Some(EffectorId::from_raw(0))
    );
    assert_eq!(api.effector_id(rig, "nope").unwrap(), None);
    assert_eq!(
        api.joint_id(RigId::from_raw(9), "root").unwrap_err().code(),
        AuthoringErrorCode::RigNotFound
    );
    assert_eq!(
        api.effector_id(RigId::from_raw(9), "x").unwrap_err().code(),
        AuthoringErrorCode::RigNotFound
    );
}

#[test]
fn a_full_motion_authors_compiles_and_samples() {
    let mut api = AnimationAuthoringApi::new();
    let rig = api.standard_humanoid();
    let m = api.create_motion("kick", Tick::new(20), rig).unwrap();
    api.add_target(m, "approach_start", Vec3::new(0.0, 0.0, -3.0))
        .unwrap();
    api.add_target(m, "ball", Vec3::new(0.0, 0.0, 0.0)).unwrap();
    api.add_target(m, "net_center", Vec3::new(0.0, 0.8, 8.0))
        .unwrap();
    api.add_target(m, "left_plant_spot", Vec3::new(0.25, 0.0, -0.1))
        .unwrap();
    api.set_style(m, "power", ratio(0.7)).unwrap();

    let approach = api
        .add_phase(m, "approach", Tick::new(0), Tick::new(10))
        .unwrap();
    api.set_phase_root_motion_move_toward(approach, "approach_start", "ball")
        .unwrap();
    api.set_phase_ease_smoothstep(approach).unwrap();
    api.set_phase_layer_weight(approach, ratio(1.0)).unwrap();

    let strike = api
        .add_phase(m, "strike", Tick::new(10), Tick::new(20))
        .unwrap();
    api.set_phase_root_motion_hold(strike).unwrap();
    api.set_phase_ease_linear(strike).unwrap();
    api.set_phase_ease_in(strike).unwrap();
    api.set_phase_ease_out(strike).unwrap();
    api.set_phase_root_motion_settle(strike).unwrap();
    api.add_set_joint_rotation(strike, "chest", Vec3::new(0.0, 0.2, 0.0))
        .unwrap();
    api.add_aim_effector_at_target(strike, "right_foot_instep", "ball")
        .unwrap();
    api.add_move_effector_toward_target(strike, "left_hand", "ball", ratio(0.5))
        .unwrap();
    api.add_raise_arm_for_balance(strike, true).unwrap();
    api.add_torso_twist_toward_target(strike, "net_center", ratio(0.5))
        .unwrap();
    api.add_leg_backswing(strike, true, ratio(0.8)).unwrap();
    api.add_leg_strike(strike, true, "ball").unwrap();
    api.add_follow_through(strike, true, "net_center").unwrap();
    api.add_pin_effector_to_target(strike, "left_foot_sole", "left_plant_spot")
        .unwrap();
    api.add_keep_gaze_on_target(strike, "ball").unwrap();
    api.add_keep_center_of_mass_over_support(strike, "left_foot_sole")
        .unwrap();
    api.add_orient_surface_toward_target(strike, "right_foot_instep", "net_center")
        .unwrap();
    api.add_preserve_foot_contact(strike, "left_foot_sole", "left_plant_spot")
        .unwrap();
    api.add_contact(strike, "right_foot_sole", "ball").unwrap();
    api.add_named_event(m, Tick::new(3), "whistle").unwrap();
    api.add_ball_contact(
        m,
        Tick::new(15),
        "right_foot_instep",
        "ball",
        "net_center",
        ratio(0.7),
    )
    .unwrap();

    let plan = api.compile(m).unwrap();
    assert_eq!(plan, PlanId::from_raw(0));

    let frame = api.sample(plan, Tick::new(15)).unwrap();
    assert_eq!(api.frame_root(&frame).translation, Vec3::new(0.0, 0.0, 0.0)); // held at ball
    assert!(api
        .frame_joint_local(&frame, JointId::from_raw(0))
        .is_some());
    assert!(api
        .frame_joint_local(&frame, JointId::from_raw(99))
        .is_none());
    assert!(api
        .frame_joint_world(&frame, JointId::from_raw(0))
        .is_some());
    assert!(api
        .frame_joint_world(&frame, JointId::from_raw(99))
        .is_none());
    assert!(api
        .frame_effector_world(&frame, EffectorId::from_raw(0))
        .is_some());
    assert!(api
        .frame_effector_world(&frame, EffectorId::from_raw(99))
        .is_none());
    assert_eq!(api.frame_event_names(&frame), vec!["ball_contact"]);
    assert_eq!(api.frame_active_constraint_count(&frame), 5);
    assert_eq!(api.frame_active_contact_count(&frame), 1);
    let (surface, target, direction, power) = api.frame_ball_contact(&frame).unwrap();
    assert_eq!(surface, EffectorId::from_raw(2)); // right_foot_instep is effector 2
    assert_eq!(target, TargetId::from_raw(1)); // ball
    assert_eq!(direction, TargetId::from_raw(2)); // net_center
    assert!((power.get() - 0.7).abs() < 1.0e-6);
}

#[test]
fn a_run_cycle_authors_a_stepping_gait_that_oscillates_the_legs() {
    let mut api = AnimationAuthoringApi::new();
    let rig = api.standard_humanoid();
    let m = api.create_motion("run", Tick::new(12), rig).unwrap();
    api.add_target(m, "start", Vec3::new(0.0, 0.0, -3.0))
        .unwrap();
    api.add_target(m, "ball", Vec3::new(0.0, 0.0, 0.0)).unwrap();
    let run = api
        .add_phase(m, "run", Tick::new(0), Tick::new(12))
        .unwrap();
    api.set_phase_root_motion_move_toward(run, "start", "ball")
        .unwrap();
    api.add_run_cycle(run, 3, ratio(0.6), ratio(0.5), ratio(0.4))
        .unwrap();
    let plan = api.compile(m).unwrap();

    let left = api.plan_joint_id(plan, "left_thigh").unwrap().unwrap();
    let right = api.plan_joint_id(plan, "right_thigh").unwrap().unwrap();
    let rot = |joint, t| {
        api.sample(plan, Tick::new(t))
            .unwrap()
            .joint_local(joint)
            .unwrap()
            .rotation
    };
    // The gait cycles the thigh over raw progress: tick 1 (sin≈+1) and tick 3
    // (sin≈-1) read distinct, non-identity rotations — the leg steps, not slides.
    assert_ne!(rot(left, 1), rot(left, 3));
    assert_ne!(rot(left, 1), Quat::IDENTITY);
    // The two legs run in antiphase (offset π), so they differ at the same tick.
    assert_ne!(rot(left, 1), rot(right, 1));
}

#[test]
fn inspection_readers_expose_names_style_and_plan_stats() {
    let mut api = AnimationAuthoringApi::new();
    let rig = api.standard_humanoid();
    let m = api.create_motion("kick", Tick::new(10), rig).unwrap();
    api.set_style(m, "power", ratio(0.4)).unwrap();
    api.add_phase(m, "approach", Tick::new(0), Tick::new(5))
        .unwrap();
    api.add_phase(m, "strike", Tick::new(5), Tick::new(10))
        .unwrap();
    api.add_named_event(m, Tick::new(3), "cue").unwrap();

    assert_eq!(api.motion_name(m).unwrap(), "kick");
    assert_eq!(
        api.motion_phase_names(m).unwrap(),
        vec!["approach", "strike"]
    );
    assert!((api.motion_style(m, "power").unwrap().unwrap().get() - 0.4).abs() < 1.0e-6);
    assert_eq!(api.motion_style(m, "missing").unwrap(), None);

    let plan = api.compile(m).unwrap();
    assert_eq!(api.plan_duration(plan).unwrap(), Tick::new(10));
    assert_eq!(api.plan_event_count(plan).unwrap(), 1);

    // Missing ids fail with their codes.
    let ghost_motion = MotionId::from_raw(9);
    let ghost_plan = PlanId::from_raw(9);
    assert_eq!(
        api.motion_name(ghost_motion).unwrap_err().code(),
        AuthoringErrorCode::MotionNotFound
    );
    assert_eq!(
        api.motion_phase_names(ghost_motion).unwrap_err().code(),
        AuthoringErrorCode::MotionNotFound
    );
    assert_eq!(
        api.motion_style(ghost_motion, "power").unwrap_err().code(),
        AuthoringErrorCode::MotionNotFound
    );
    assert_eq!(
        api.plan_duration(ghost_plan).unwrap_err().code(),
        AuthoringErrorCode::PlanNotFound
    );
    assert_eq!(
        api.plan_event_count(ghost_plan).unwrap_err().code(),
        AuthoringErrorCode::PlanNotFound
    );
}

#[test]
fn a_frame_without_a_ball_contact_reports_no_ball_contact() {
    let mut api = AnimationAuthoringApi::new();
    let rig = api.standard_humanoid();
    let m = api.create_motion("k", Tick::new(10), rig).unwrap();
    api.add_phase(m, "p", Tick::new(0), Tick::new(10)).unwrap();
    let plan = api.compile(m).unwrap();
    let frame = api.sample(plan, Tick::new(5)).unwrap();
    assert_eq!(api.frame_ball_contact(&frame), None);
    assert!(api.frame_event_names(&frame).is_empty());
}

#[test]
fn every_missing_id_fails_with_its_code() {
    let mut api = AnimationAuthoringApi::new();
    let ghost_rig = RigId::from_raw(9);
    let ghost_motion = MotionId::from_raw(9);
    let ghost_phase = PhaseId::new(MotionId::from_raw(9), 0);
    let ghost_plan = PlanId::from_raw(9);

    assert_eq!(
        api.joint_names(ghost_rig).unwrap_err().code(),
        AuthoringErrorCode::RigNotFound
    );
    assert_eq!(
        api.effector_names(ghost_rig).unwrap_err().code(),
        AuthoringErrorCode::RigNotFound
    );
    assert_eq!(
        api.rig_is_valid(ghost_rig).unwrap_err().code(),
        AuthoringErrorCode::RigNotFound
    );
    assert_eq!(
        api.create_motion("k", Tick::new(1), ghost_rig)
            .unwrap_err()
            .code(),
        AuthoringErrorCode::RigNotFound
    );

    assert_eq!(
        api.add_target(ghost_motion, "t", Vec3::ZERO)
            .unwrap_err()
            .code(),
        AuthoringErrorCode::MotionNotFound
    );
    assert_eq!(
        api.set_style(ghost_motion, "s", ratio(0.1))
            .unwrap_err()
            .code(),
        AuthoringErrorCode::MotionNotFound
    );
    assert_eq!(
        api.add_phase(ghost_motion, "p", Tick::new(0), Tick::new(1))
            .unwrap_err()
            .code(),
        AuthoringErrorCode::MotionNotFound
    );
    assert_eq!(
        api.add_named_event(ghost_motion, Tick::new(0), "n")
            .unwrap_err()
            .code(),
        AuthoringErrorCode::MotionNotFound
    );
    assert_eq!(
        api.add_ball_contact(ghost_motion, Tick::new(0), "e", "t", "d", ratio(0.1))
            .unwrap_err()
            .code(),
        AuthoringErrorCode::MotionNotFound
    );
    assert_eq!(
        api.compile(ghost_motion).unwrap_err().code(),
        AuthoringErrorCode::MotionNotFound
    );

    // Phase-scoped setters and every goal/constraint/contact adder.
    assert_eq!(
        api.set_phase_root_motion_move_toward(ghost_phase, "a", "b")
            .unwrap_err()
            .code(),
        AuthoringErrorCode::MotionNotFound
    );
    assert_eq!(
        api.set_phase_root_motion_hold(ghost_phase)
            .unwrap_err()
            .code(),
        AuthoringErrorCode::MotionNotFound
    );
    assert_eq!(
        api.set_phase_root_motion_settle(ghost_phase)
            .unwrap_err()
            .code(),
        AuthoringErrorCode::MotionNotFound
    );
    assert_eq!(
        api.set_phase_ease_linear(ghost_phase).unwrap_err().code(),
        AuthoringErrorCode::MotionNotFound
    );
    assert_eq!(
        api.set_phase_ease_smoothstep(ghost_phase)
            .unwrap_err()
            .code(),
        AuthoringErrorCode::MotionNotFound
    );
    assert_eq!(
        api.set_phase_ease_in(ghost_phase).unwrap_err().code(),
        AuthoringErrorCode::MotionNotFound
    );
    assert_eq!(
        api.set_phase_ease_out(ghost_phase).unwrap_err().code(),
        AuthoringErrorCode::MotionNotFound
    );
    assert_eq!(
        api.set_phase_layer_weight(ghost_phase, ratio(1.0))
            .unwrap_err()
            .code(),
        AuthoringErrorCode::MotionNotFound
    );
    assert_eq!(
        api.add_set_joint_rotation(ghost_phase, "chest", Vec3::ZERO)
            .unwrap_err()
            .code(),
        AuthoringErrorCode::MotionNotFound
    );
    assert_eq!(
        api.add_aim_effector_at_target(ghost_phase, "e", "t")
            .unwrap_err()
            .code(),
        AuthoringErrorCode::MotionNotFound
    );
    assert_eq!(
        api.add_move_effector_toward_target(ghost_phase, "e", "t", ratio(0.5))
            .unwrap_err()
            .code(),
        AuthoringErrorCode::MotionNotFound
    );
    assert_eq!(
        api.add_raise_arm_for_balance(ghost_phase, true)
            .unwrap_err()
            .code(),
        AuthoringErrorCode::MotionNotFound
    );
    assert_eq!(
        api.add_torso_twist_toward_target(ghost_phase, "t", ratio(0.5))
            .unwrap_err()
            .code(),
        AuthoringErrorCode::MotionNotFound
    );
    assert_eq!(
        api.add_leg_backswing(ghost_phase, true, ratio(0.5))
            .unwrap_err()
            .code(),
        AuthoringErrorCode::MotionNotFound
    );
    assert_eq!(
        api.add_leg_strike(ghost_phase, true, "t")
            .unwrap_err()
            .code(),
        AuthoringErrorCode::MotionNotFound
    );
    assert_eq!(
        api.add_follow_through(ghost_phase, true, "t")
            .unwrap_err()
            .code(),
        AuthoringErrorCode::MotionNotFound
    );
    assert_eq!(
        api.add_pin_effector_to_target(ghost_phase, "e", "t")
            .unwrap_err()
            .code(),
        AuthoringErrorCode::MotionNotFound
    );
    assert_eq!(
        api.add_keep_gaze_on_target(ghost_phase, "t")
            .unwrap_err()
            .code(),
        AuthoringErrorCode::MotionNotFound
    );
    assert_eq!(
        api.add_keep_center_of_mass_over_support(ghost_phase, "e")
            .unwrap_err()
            .code(),
        AuthoringErrorCode::MotionNotFound
    );
    assert_eq!(
        api.add_orient_surface_toward_target(ghost_phase, "e", "t")
            .unwrap_err()
            .code(),
        AuthoringErrorCode::MotionNotFound
    );
    assert_eq!(
        api.add_preserve_foot_contact(ghost_phase, "e", "t")
            .unwrap_err()
            .code(),
        AuthoringErrorCode::MotionNotFound
    );
    assert_eq!(
        api.add_contact(ghost_phase, "e", "t").unwrap_err().code(),
        AuthoringErrorCode::MotionNotFound
    );

    assert_eq!(
        api.sample(ghost_plan, Tick::new(0)).unwrap_err().code(),
        AuthoringErrorCode::PlanNotFound
    );
}

#[test]
fn a_missing_phase_index_fails_with_phase_not_found() {
    let mut api = AnimationAuthoringApi::new();
    let rig = api.standard_humanoid();
    let m = api.create_motion("k", Tick::new(10), rig).unwrap();
    let bad_phase = PhaseId::new(m, 7); // motion exists, phase index does not
    assert_eq!(
        api.set_phase_ease_linear(bad_phase).unwrap_err().code(),
        AuthoringErrorCode::PhaseNotFound
    );
}
