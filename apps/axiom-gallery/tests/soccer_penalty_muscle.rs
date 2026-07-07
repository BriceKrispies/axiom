//! Proof that the soccer penalty kicker runs on the **virtual-muscle active
//! control layer** (the engine's `VirtualMuscleController`, configured by the
//! soccer per-phase muscle policy). The kicker holds itself over its support
//! through every phase instead of collapsing into a ragdoll.
//!
//! Drives only the game's public surface (`PenaltyPhysicsKick` + its captured
//! muscle readouts) plus the engine facade for the strength/balance scaling.

use axiom_gallery::soccer_penalty::penalty_kick_motion::{
    SoccerPenaltyKickMotionSpec, SoccerPenaltyKickStyle, PHASE_SAMPLE_TICKS,
};
use axiom_gallery::soccer_penalty::penalty_muscle::{
    GROUP_CORE, GROUP_LEFT_ANKLE, GROUP_LEFT_LEG, GROUP_PELVIS, GROUP_RIGHT_LEG, SUPPORT_BOTH_FEET,
    SUPPORT_LEFT_FOOT,
};
use axiom_gallery::soccer_penalty::penalty_physics_kick::PenaltyPhysicsKick;
use axiom_kernel::Tick;
use axiom_physical_animation::PhysicalAnimationApi;

/// Phase tick order (matches PHASE_NAMES): setup, sprint, pre_plant, plant,
/// backswing, hip_drive, strike, follow_through, recover.
const PLANT_T: u64 = PHASE_SAMPLE_TICKS[3];
const BACKSWING_T: u64 = PHASE_SAMPLE_TICKS[4];
const HIP_DRIVE_T: u64 = PHASE_SAMPLE_TICKS[5];
const STRIKE_T: u64 = PHASE_SAMPLE_TICKS[6];
const FOLLOW_T: u64 = PHASE_SAMPLE_TICKS[7];
const RECOVER_T: u64 = PHASE_SAMPLE_TICKS[8];

#[test]
fn the_game_kick_runs_through_the_muscle_controller() {
    let kick = PenaltyPhysicsKick::default_kick();
    // Every phase carries a muscle command (support mode + CoM + group weights).
    let snap = kick.debug_snapshot();
    assert_eq!(snap.len(), 9);
    assert!(snap.iter().all(|s| s.pelvis_weight > 0.0), "each phase actuates the pelvis muscle group");
    assert!(snap.iter().any(|s| s.support_mode == SUPPORT_LEFT_FOOT), "some phase stands on the left foot");
    assert!(snap.iter().any(|s| s.support_mode == SUPPORT_BOTH_FEET), "some phase stands on both feet");
}

#[test]
fn plant_selects_left_foot_and_strengthens_the_support_side() {
    let kick = PenaltyPhysicsKick::default_kick();
    let plant = kick.frame(PLANT_T);
    assert_eq!(plant.support_mode, SUPPORT_LEFT_FOOT);
    assert!(plant.group_weight(GROUP_LEFT_LEG) >= 0.85, "left leg stabilizes the plant");
    assert!(plant.group_weight(GROUP_LEFT_ANKLE) >= 0.85, "left ankle stabilizes the plant");
    assert!(plant.group_weight(GROUP_CORE) >= 0.7, "core braces");
    assert!(plant.plant_strength > 0.0, "a plant hold is active");
}

#[test]
fn backswing_loads_the_right_leg_while_the_left_foot_stays_planted() {
    let kick = PenaltyPhysicsKick::default_kick();
    let plant = kick.frame(PLANT_T);
    let back = kick.frame(BACKSWING_T);
    assert_eq!(back.support_mode, SUPPORT_LEFT_FOOT, "still on the left foot");
    assert!(back.group_weight(GROUP_LEFT_LEG) >= 0.8, "left support preserved");
    assert!(back.group_weight(GROUP_RIGHT_LEG) > plant.group_weight(GROUP_RIGHT_LEG), "right leg loads");
}

#[test]
fn hip_drive_drives_pelvis_and_right_leg_harder_than_backswing() {
    let kick = PenaltyPhysicsKick::default_kick();
    let back = kick.frame(BACKSWING_T);
    let hip = kick.frame(HIP_DRIVE_T);
    assert_eq!(hip.support_mode, SUPPORT_LEFT_FOOT);
    assert!(hip.group_weight(GROUP_PELVIS) > back.group_weight(GROUP_PELVIS), "pelvis drives harder");
    assert!(hip.group_weight(GROUP_RIGHT_LEG) > back.group_weight(GROUP_RIGHT_LEG), "kicking leg drives harder");
}

#[test]
fn strike_applies_the_ball_impulse_through_physics() {
    let kick = PenaltyPhysicsKick::default_kick();
    assert!(kick.frame(STRIKE_T).strike, "the ball_contact fires at the strike");
    assert!(kick.frame(STRIKE_T).ball_impulse.is_some(), "a real impulse is applied");
    let vel = kick.frame(STRIKE_T + 1).ball_velocity;
    assert!(vel.z > 0.0 && vel.length() > 1.0, "the ball flies toward the net through physics");
}

#[test]
fn follow_through_releases_the_plant_after_the_strike() {
    let kick = PenaltyPhysicsKick::default_kick();
    let strike_plant = kick.frame(STRIKE_T).plant_strength;
    let follow_plant = kick.frame(FOLLOW_T).plant_strength;
    assert!(follow_plant < strike_plant, "plant stiffness drops after the strike: strike={strike_plant}, follow={follow_plant}");
    assert_eq!(kick.frame(FOLLOW_T).support_mode, SUPPORT_BOTH_FEET, "weight moves off the plant foot");
}

#[test]
fn recover_reduces_strike_drive_and_restores_rest_posture() {
    let kick = PenaltyPhysicsKick::default_kick();
    let strike = kick.frame(STRIKE_T);
    let recover = kick.frame(RECOVER_T);
    assert_eq!(recover.support_mode, SUPPORT_BOTH_FEET);
    assert!(recover.group_weight(GROUP_PELVIS) < strike.group_weight(GROUP_PELVIS), "aggressive drive drops");
    // Rest posture is restored: recovery damping is stronger than at the strike.
    assert!(recover.recovery_damping > strike.recovery_damping, "the body settles");
    assert!(recover.group_weight(GROUP_CORE) > 0.0, "postural stabilization remains");
}

#[test]
fn muscle_strength_scales_max_torque_and_balance_strength_scales_correction() {
    let torque = |strength: f32| {
        let mut s = SoccerPenaltyKickStyle::default_style();
        s.muscle_strength = strength;
        PenaltyPhysicsKick::simulate(s).frame(STRIKE_T).group_max_torque(GROUP_RIGHT_LEG)
    };
    assert!(torque(2.0) > torque(1.0), "muscle_strength scales max_torque");

    let correction = |balance: f32| {
        let mut s = SoccerPenaltyKickStyle::default_style();
        s.balance_strength = balance;
        PenaltyPhysicsKick::simulate(s).frame(PLANT_T).balance_correction.length()
    };
    assert!(correction(3.0) > correction(1.0), "balance_strength scales the balance force");
}

#[test]
fn two_identical_muscled_kicks_produce_identical_frames_and_ball_state() {
    let a = PenaltyPhysicsKick::default_kick();
    let b = PenaltyPhysicsKick::default_kick();
    for t in 0..a.duration() {
        assert_eq!(a.frame(t).joints, b.frame(t).joints, "poses match at tick {t}");
        assert_eq!(a.frame(t).ball_velocity, b.frame(t).ball_velocity, "ball state matches at tick {t}");
        assert_eq!(a.frame(t).support_mode, b.frame(t).support_mode);
        assert_eq!(a.frame(t).balance_correction, b.frame(t).balance_correction);
    }
}

#[test]
fn the_engine_facade_scales_torque_and_balance_deterministically() {
    // Direct facade check: muscle_strength → max_torque, balance_strength →
    // correction, both deterministic (the engine control layer, driven as the game
    // drives it).
    let spec = SoccerPenaltyKickMotionSpec::author(SoccerPenaltyKickStyle::default_style());
    let torque = |strength: f32| {
        let mut sim = PhysicalAnimationApi::new();
        sim.bind_standard_humanoid(spec.authoring(), spec.plan()).unwrap();
        sim.attach_ball(spec.authoring(), spec.plan()).unwrap();
        sim.set_muscle_style(ratio(strength), ratio(1.0), ratio(1.0));
        let weights = [ratio(0.8); 10];
        let f = (0..=PLANT_T).map(|t| sim.advance_muscled(spec.authoring(), spec.plan(), Tick::new(t), 1, weights).unwrap()).last().unwrap();
        sim.frame_muscle_group_max_torque(&f, GROUP_RIGHT_LEG as u8).unwrap().get()
    };
    assert!(torque(2.0) > torque(1.0));
    assert_eq!(torque(1.5), torque(1.5), "deterministic");
}

fn ratio(v: f32) -> axiom_kernel::Ratio {
    axiom_kernel::Ratio::new(v).unwrap()
}
