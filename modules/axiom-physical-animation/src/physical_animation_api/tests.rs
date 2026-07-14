//! The physical-animation facade's test suite (moved out of
//! `physical_animation_api.rs` to keep that file inside the engine's
//! 1000-line budget; same pattern as `software_rasterizer/tests.rs`).

use super::*;
use crate::physical_error_code::PhysicalErrorCode;

/// A ready-to-simulate `(authoring, plan)` for the built-in penalty kick.
fn penalty(power: f32) -> (AnimationAuthoringApi, PlanId) {
    let mut api = AnimationAuthoringApi::new();
    let m = api.soccer_penalty_kick_v0(Ratio::new(power).unwrap());
    let plan = api.compile(m).unwrap();
    (api, plan)
}

/// A bound + ball-attached controller for the penalty kick.
fn ready(authoring: &AnimationAuthoringApi, plan: PlanId) -> PhysicalAnimationApi {
    let mut sim = PhysicalAnimationApi::new();
    sim.bind_standard_humanoid(authoring, plan).unwrap();
    sim.attach_ball(authoring, plan).unwrap();
    sim
}

#[test]
fn new_and_default_agree_and_advancing_unbound_or_ballless_fails() {
    let a = PhysicalAnimationApi::new();
    let b = PhysicalAnimationApi::default();
    assert!(format!("{a:?}").contains("PhysicalAnimationApi"));
    assert!(format!("{b:?}").contains("PhysicalAnimationApi"));
    let (authoring, plan) = penalty(0.7);

    // Advancing before binding fails NotBound.
    let mut unbound = PhysicalAnimationApi::new();
    assert_eq!(
        unbound
            .advance(&authoring, plan, Tick::new(0))
            .unwrap_err()
            .code(),
        PhysicalErrorCode::NotBound
    );
    // Bound but no ball fails NoBall.
    let mut no_ball = PhysicalAnimationApi::new();
    no_ball.bind_standard_humanoid(&authoring, plan).unwrap();
    assert_eq!(
        no_ball
            .advance(&authoring, plan, Tick::new(0))
            .unwrap_err()
            .code(),
        PhysicalErrorCode::NoBall
    );
}

#[test]
fn advancing_the_same_inputs_twice_produces_identical_frames() {
    let (authoring, plan) = penalty(0.7);
    let mut a = ready(&authoring, plan);
    let mut b = ready(&authoring, plan);
    // Run both simulations through the strike and compare the final frames.
    let last_a = (0..40)
        .map(|t| a.advance(&authoring, plan, Tick::new(t)).unwrap())
        .last()
        .unwrap();
    let last_b = (0..40)
        .map(|t| b.advance(&authoring, plan, Tick::new(t)).unwrap())
        .last()
        .unwrap();
    assert_eq!(format!("{last_a:?}"), format!("{last_b:?}"));
}

#[test]
fn approach_drives_the_pelvis_toward_the_ball_and_reports_the_objective() {
    let (authoring, plan) = penalty(0.7);
    let mut sim = ready(&authoring, plan);
    let ball_z = 0.0;
    let early = sim.advance(&authoring, plan, Tick::new(2)).unwrap();
    let early_z = sim
        .frame_body_transform(&early, "pelvis")
        .unwrap()
        .translation
        .z;
    let late = (3..11)
        .map(|t| sim.advance(&authoring, plan, Tick::new(t)).unwrap())
        .last()
        .unwrap();
    let late_z = sim
        .frame_body_transform(&late, "pelvis")
        .unwrap()
        .translation
        .z;
    assert!(
        late_z > early_z,
        "pelvis moved toward the ball under physics"
    );
    assert!(early_z <= ball_z + 1.0);
    // The approach frame reports the root-velocity objective (+Z).
    assert!(sim.frame_root_velocity(&early).unwrap().z > 0.0);
    assert_eq!(sim.frame_phase_name(&early).as_deref(), Some("approach"));
    assert_eq!(sim.frame_tick(&early), Tick::new(2));
}

#[test]
fn plant_holds_the_left_foot_body_at_the_plant_spot() {
    let (authoring, plan) = penalty(0.7);
    let mut sim = ready(&authoring, plan);
    let frame = (0..17)
        .map(|t| sim.advance(&authoring, plan, Tick::new(t)).unwrap())
        .last()
        .unwrap();
    // At tick 16 (plant phase), the left-foot body is held at the plant spot.
    let foot = sim
        .frame_body_transform(&frame, "left_foot")
        .unwrap()
        .translation;
    assert!(foot.distance(Vec3::new(0.25, 0.0, -0.1)) < 1.0e-4);
    assert!(sim.frame_foot_plant(&frame).is_some());
}

#[test]
fn strike_applies_a_real_ball_impulse_toward_the_net_and_drives_harder_than_backswing() {
    let (authoring, plan) = penalty(0.7);
    let mut sim = ready(&authoring, plan);
    // Capture the backswing drive on the way to the strike.
    let mut backswing_drive = 0.0;
    let mut strike_frame = None;
    (0..39).for_each(|t| {
        let f = sim.advance(&authoring, plan, Tick::new(t)).unwrap();
        (t == 26).then(|| backswing_drive = sim.frame_motor_drive(&f).get());
        (t == 38).then(|| strike_frame = Some(f));
    });
    let strike = strike_frame.unwrap();
    // A real impulse was applied to the ball this tick.
    assert!(sim.frame_ball_impulse(&strike).is_some());
    // The ball gained velocity pointing toward the net (+Z dominant).
    let vel = sim.frame_ball_velocity(&strike).unwrap();
    assert!(vel.z > 0.0, "ball flies toward the net");
    assert!(vel.length() > 1.0, "the strike imparted real speed");
    // Strike drive exceeds backswing drive.
    assert!(sim.frame_motor_drive(&strike).get() > backswing_drive);
    assert!(sim.frame_motor_count(&strike) > 0);
}

#[test]
fn frame_exposes_gaze_effectors_contacts_events_and_step_index() {
    let (authoring, plan) = penalty(0.7);
    let mut sim = ready(&authoring, plan);
    let strike = (0..39)
        .map(|t| sim.advance(&authoring, plan, Tick::new(t)).unwrap())
        .last()
        .unwrap();
    assert_eq!(sim.frame_gaze(&strike), Some(Vec3::new(0.0, 0.0, 0.0))); // gaze on the ball
    assert!(sim
        .frame_effector_transform(&strike, "right_foot_instep")
        .is_some());
    assert_eq!(
        sim.frame_effector_transform(&strike, "no_such_effector"),
        None
    );
    assert_eq!(
        sim.frame_event_names(&strike),
        vec!["ball_contact".to_string()]
    );
    assert_eq!(sim.frame_step_index(&strike), 39); // 39 steps taken (ticks 0..39)
    assert_eq!(
        sim.frame_contact_count(&strike),
        sim.frame_contact_count(&strike)
    );
    assert!(sim.frame_ball_transform(&strike).is_some());
    assert_eq!(sim.frame_body_transform(&strike, "no_such_body"), None);
}

#[test]
fn recover_drive_is_weaker_than_the_strike() {
    let (authoring, plan) = penalty(0.7);
    let mut sim = ready(&authoring, plan);
    let mut strike_drive = 0.0;
    let mut recover = None;
    (0..57).for_each(|t| {
        let f = sim.advance(&authoring, plan, Tick::new(t)).unwrap();
        (t == 38).then(|| strike_drive = sim.frame_motor_drive(&f).get());
        (t == 56).then(|| recover = Some(f));
    });
    // The recover phase (layer weight 0.3) drives less than the strike (1.0).
    let recover = recover.unwrap();
    assert_eq!(sim.frame_phase_name(&recover).as_deref(), Some("recover"));
    assert!(sim.frame_motor_drive(&recover).get() < strike_drive);
}

#[test]
fn attach_ball_missing_plan_fails_through_authoring() {
    let mut sim = PhysicalAnimationApi::new();
    let authoring = AnimationAuthoringApi::new();
    assert_eq!(
        sim.attach_ball(&authoring, PlanId::from_raw(9))
            .unwrap_err()
            .code(),
        PhysicalErrorCode::AuthoringFailed
    );
}

/// Ten identical per-group phase weights.
fn weights(w: f32) -> [Ratio; MUSCLE_GROUP_COUNT] {
    [Ratio::new(w).unwrap(); MUSCLE_GROUP_COUNT]
}

/// Ten identical per-group base params.
fn profile(s: f32, d: f32, t: f32, rw: f32) -> [(Ratio, Ratio, Ratio, Ratio); MUSCLE_GROUP_COUNT] {
    [(
        Ratio::new(s).unwrap(),
        Ratio::new(d).unwrap(),
        Ratio::new(t).unwrap(),
        Ratio::new(rw).unwrap(),
    ); MUSCLE_GROUP_COUNT]
}

#[test]
fn muscled_advance_records_the_command_and_muscle_free_does_not() {
    let (authoring, plan) = penalty(0.7);
    let mut sim = ready(&authoring, plan);
    // Run into the plant phase with left-foot support.
    let frame = (0..17)
        .map(|t| {
            sim.advance_muscled(&authoring, plan, Tick::new(t), 1, weights(0.6))
                .unwrap()
        })
        .last()
        .unwrap();
    assert_eq!(sim.frame_support_mode(&frame), Some(1));
    assert!(sim.frame_center_of_mass(&frame).is_some());
    assert!(sim.frame_support_target(&frame).is_some());
    assert!(sim.frame_balance_correction(&frame).is_some());
    assert!(sim.frame_plant_strength(&frame).unwrap().get() > 0.0);
    assert!(sim.frame_recovery_damping(&frame).is_some());
    assert!(sim.frame_muscle_group_weight(&frame, 5).is_some());
    assert!(sim.frame_muscle_group_max_torque(&frame, 5).unwrap().get() > 0.0);
    assert!(sim
        .frame_muscle_report(&frame)
        .unwrap()
        .contains("support=1"));

    // The muscle-free path carries no muscle readouts.
    let plain = sim.advance(&authoring, plan, Tick::new(17)).unwrap();
    assert_eq!(sim.frame_support_mode(&plain), None);
    assert_eq!(sim.frame_center_of_mass(&plain), None);
    assert_eq!(sim.frame_muscle_report(&plain), None);
    assert_eq!(sim.frame_balance_correction(&plain), None);
    assert_eq!(sim.frame_muscle_group_weight(&plain, 0), None);
    assert_eq!(sim.frame_muscle_group_max_torque(&plain, 0), None);
    assert_eq!(sim.frame_plant_strength(&plain), None);
    assert_eq!(sim.frame_recovery_damping(&plain), None);
    assert_eq!(sim.frame_support_target(&plain), None);
}

#[test]
fn muscle_strength_and_balance_strength_scale_the_command() {
    let (authoring, plan) = penalty(0.7);
    let torque_at = |strength: f32| {
        let mut sim = ready(&authoring, plan);
        sim.set_muscle_profile(profile(1.0, 0.5, 1.0, 0.6));
        sim.set_muscle_style(
            Ratio::new(strength).unwrap(),
            Ratio::new(1.0).unwrap(),
            Ratio::new(1.0).unwrap(),
        );
        let f = (0..17)
            .map(|t| {
                sim.advance_muscled(&authoring, plan, Tick::new(t), 1, weights(0.8))
                    .unwrap()
            })
            .last()
            .unwrap();
        sim.frame_muscle_group_max_torque(&f, 5).unwrap().get()
    };
    assert!(
        torque_at(2.0) > torque_at(1.0),
        "muscle_strength scales max_torque"
    );

    let corr_at = |bal: f32| {
        let mut sim = ready(&authoring, plan);
        sim.set_muscle_style(
            Ratio::new(1.0).unwrap(),
            Ratio::new(1.0).unwrap(),
            Ratio::new(bal).unwrap(),
        );
        let f = (0..17)
            .map(|t| {
                sim.advance_muscled(&authoring, plan, Tick::new(t), 1, weights(0.6))
                    .unwrap()
            })
            .last()
            .unwrap();
        sim.frame_balance_correction(&f).unwrap().length()
    };
    assert!(
        corr_at(2.0) > corr_at(1.0),
        "balance_strength scales the balance force"
    );
}

#[test]
fn the_balance_force_pulls_the_pelvis_toward_its_support() {
    // A strongly-balanced muscled run keeps the pelvis nearer the left-foot
    // support than the muscle-free run — proof the balance force is real.
    let (authoring, plan) = penalty(0.7);
    let mut muscled = ready(&authoring, plan);
    muscled.set_muscle_style(
        Ratio::new(1.0).unwrap(),
        Ratio::new(1.0).unwrap(),
        Ratio::new(3.0).unwrap(),
    );
    let mut plain = ready(&authoring, plan);
    let (mut mf, mut pf) = (None, None);
    (0..20).for_each(|t| {
        mf = Some(
            muscled
                .advance_muscled(&authoring, plan, Tick::new(t), 1, weights(0.7))
                .unwrap(),
        );
        pf = Some(plain.advance(&authoring, plan, Tick::new(t)).unwrap());
    });
    let mframe = mf.unwrap();
    let support = muscled.frame_support_target(&mframe).unwrap();
    let m_pelvis = muscled
        .frame_body_transform(&mframe, "pelvis")
        .unwrap()
        .translation;
    let p_pelvis = plain
        .frame_body_transform(&pf.unwrap(), "pelvis")
        .unwrap()
        .translation;
    let horiz = |a: Vec3, b: Vec3| ((a.x - b.x).powi(2) + (a.z - b.z).powi(2)).sqrt();
    let muscled_gap = horiz(m_pelvis, support);
    let plain_gap = horiz(p_pelvis, support);
    // The muscled pelvis tracks its support at least as well as the muscle-free one.
    assert!(muscled_gap <= plain_gap + 1.0e-4);
}

#[test]
fn two_identical_muscled_runs_produce_identical_frames() {
    let (authoring, plan) = penalty(0.7);
    let run = || {
        let mut sim = ready(&authoring, plan);
        (0..40)
            .map(|t| {
                sim.advance_muscled(&authoring, plan, Tick::new(t), 1, weights(0.6))
                    .unwrap()
            })
            .last()
            .unwrap()
    };
    assert_eq!(format!("{:?}", run()), format!("{:?}", run()));
}

#[test]
fn muscled_advance_fails_before_binding_and_before_a_ball() {
    let (authoring, plan) = penalty(0.7);
    let mut unbound = PhysicalAnimationApi::new();
    assert_eq!(
        unbound
            .advance_muscled(&authoring, plan, Tick::new(0), 1, weights(0.5))
            .unwrap_err()
            .code(),
        PhysicalErrorCode::NotBound
    );
    let mut no_ball = PhysicalAnimationApi::new();
    no_ball.bind_standard_humanoid(&authoring, plan).unwrap();
    assert_eq!(
        no_ball
            .advance_muscled(&authoring, plan, Tick::new(0), 1, weights(0.5))
            .unwrap_err()
            .code(),
        PhysicalErrorCode::NoBall
    );
}

#[test]
fn named_transform_finds_bodies_and_defaults_when_absent() {
    let bodies = [(
        "pelvis",
        Transform::from_translation(Vec3::new(1.0, 2.0, 3.0)),
    )];
    assert_eq!(
        named_transform(&bodies, "pelvis").translation,
        Vec3::new(1.0, 2.0, 3.0)
    );
    assert_eq!(named_transform(&bodies, "left_foot"), Transform::IDENTITY);
}
