#![cfg(feature = "experimental_physical_humanoid_kicker")]
//! EXPERIMENTAL — only compiled with `--features experimental_physical_humanoid_kicker`.
//! The default kicker is the authored/kinematic pose; this proves the *experimental*
//! physics-backed path still runs. See `tests/soccer_penalty_kick_animation.rs` for
//! the default-path proof.
//!
//! Proof that the soccer penalty game's kicker + ball now run on the
//! **physics-backed authored motion pipeline**:
//!
//! ```text
//! SoccerPenaltyKickMotionSpec (authored, 9 phases)
//!   → PhysicalAnimationApi (axiom-physical-animation over axiom-physics)
//!   → PhysicalKickFrame per tick   → the visible kicker boxes
//!   → a real ball impulse at the strike → the physics-driven ball flight
//! ```
//!
//! Everything here drives only the game's public surface.

use axiom_gallery::soccer_penalty::penalty_ball::{penalty_spot, world_target, PenaltyBallState};
use axiom_gallery::soccer_penalty::penalty_kick_motion::{
    SoccerPenaltyKickMotionSpec, SoccerPenaltyKickStyle, PHASE_NAMES, PHASE_SAMPLE_TICKS, STRIKE_CONTACT_TICK,
};
use axiom_gallery::soccer_penalty::penalty_kicker::{kicker_frame, KickerRig};
use axiom_gallery::soccer_penalty::penalty_physics_kick::PenaltyPhysicsKick;
use axiom_gallery::soccer_penalty::{PenaltyInputIntent, PenaltyInteractionState};
use axiom_kernel::Tick;

// --- the authored motion plan ----------------------------------------------

#[test]
fn the_motion_spec_compiles_with_all_nine_phases_in_order() {
    let spec = SoccerPenaltyKickMotionSpec::author(SoccerPenaltyKickStyle::default_style());
    assert_eq!(PHASE_NAMES.len(), 9);
    for (i, &t) in PHASE_SAMPLE_TICKS.iter().enumerate() {
        assert_eq!(
            spec.authoring().active_phase_name(spec.plan(), Tick::new(t)).unwrap().as_deref(),
            Some(PHASE_NAMES[i]),
        );
    }
    // The phase names are exactly the authored nine, in the required order.
    assert_eq!(
        PHASE_NAMES,
        ["setup", "sprint_approach", "pre_plant", "plant", "backswing", "hip_drive", "strike", "follow_through", "recover"]
    );
}

#[test]
fn style_power_changes_the_strike_impulse_deterministically() {
    let speed = |p: i32| PenaltyPhysicsKick::from_power_kick(p).strike_launch_speed();
    assert!(speed(90) > speed(30), "more power → faster physics ball");
    // Deterministic: identical style → identical physics strike speed.
    assert_eq!(speed(60), speed(60));
}

// --- the physics-backed kick behaviours ------------------------------------

#[test]
fn the_kick_runs_up_plants_winds_up_strikes_and_recovers_through_physics() {
    let kick = PenaltyPhysicsKick::default_kick();

    // sprint_approach drives the pelvis toward the ball (physics force on the
    // dynamic root body).
    assert!(kick.frame(18).pelvis().z > kick.frame(6).pelvis().z, "the kicker runs up");
    assert!(kick.frame(12).root_velocity.is_some(), "sprint carries a root-velocity objective");

    // plant pins the left foot.
    assert!(kick.frame(32).foot_plant, "the plant applies a left-foot plant objective");

    // hip_drive drives harder than backswing.
    assert!(kick.frame(49).motor_drive > kick.frame(41).motor_drive, "hips drive harder than the wind-up");

    // backswing draws the right foot behind; follow-through sends it past the ball.
    assert!(kick.frame(64).right_foot().z > kick.frame(41).right_foot().z, "the leg sweeps through");

    // exactly one ball_contact, applying a real physics impulse toward the net.
    let strikes: Vec<u64> = (0..kick.duration()).filter(|&t| kick.frame(t).strike).collect();
    assert_eq!(strikes, vec![STRIKE_CONTACT_TICK]);
    assert!(kick.frame(STRIKE_CONTACT_TICK).ball_impulse.is_some(), "the strike impulses the ball");
    let vel = kick.frame(STRIKE_CONTACT_TICK + 1).ball_velocity;
    assert!(vel.z > 0.0 && vel.length() > 1.0, "ball flies toward the net through physics");

    // recover settles softer than the strike.
    assert!(kick.frame(71).motor_drive < kick.frame(STRIKE_CONTACT_TICK).motor_drive);
}

#[test]
fn two_identical_kicks_produce_identical_frames_and_ball_state() {
    let a = PenaltyPhysicsKick::default_kick();
    let b = PenaltyPhysicsKick::default_kick();
    // Every captured tick is byte-identical (poses + ball state).
    for t in 0..a.duration() {
        assert_eq!(a.frame(t).joints, b.frame(t).joints, "poses match at tick {t}");
        assert_eq!(a.frame(t).ball_velocity, b.frame(t).ball_velocity, "ball state matches at tick {t}");
    }
}

#[test]
fn the_debug_snapshot_inspects_every_phase() {
    let snap = PenaltyPhysicsKick::default_kick().debug_snapshot();
    assert_eq!(snap.len(), 9);
    assert!(snap.iter().any(|s| s.phase == "sprint_approach" && s.sprinting));
    assert!(snap.iter().any(|s| s.phase == "plant" && s.foot_plant));
    assert!(snap.iter().any(|s| s.phase == "strike" && s.striking));
}

// --- the game consumes it --------------------------------------------------

#[test]
fn the_visible_kicker_is_posed_by_the_physics_kick_and_animates_across_the_shot() {
    let rig = KickerRig::new();
    // The kicker is 13 physics-posed boxes.
    let ready = rig.boxes_at(kicker_frame(&PenaltyInteractionState::start()));
    assert_eq!(ready.len(), 13);

    // Drive a full shot; the kicker pose is different mid-flight (strike/follow) than
    // at rest — the authored motion visibly animates the kicker, not a static puppet.
    let launched = PenaltyInteractionState::run(&vec![PenaltyInputIntent::charging(0, 0); 40]);
    let flying = rig.boxes_at(kicker_frame(&launched));
    let ready_foot = ready[8].center; // right foot
    let flying_foot = flying[8].center;
    assert!(ready_foot.distance(flying_foot) > 0.1, "the kicking foot moves across the shot");
}

#[test]
fn the_ball_is_physics_driven_from_the_spot_to_the_aimed_target() {
    // Aim far top-right (beyond the keeper), charge, and fire — the ball leaves the
    // spot and lands on the aimed goal-plane target, driven through axiom-physics.
    let mut script = Vec::new();
    script.extend(vec![PenaltyInputIntent::aiming(100, 0); 12]);
    script.extend(vec![PenaltyInputIntent::aiming(0, 100); 3]);
    script.extend(vec![PenaltyInputIntent::charging(0, 0); 12]);
    script.push(PenaltyInputIntent::releasing());
    let mut state = PenaltyInteractionState::run(&script);

    let mut steps = 0;
    while state.ball_state() != PenaltyBallState::ArrivedAtGoalPlane && steps < 400 {
        state = state.advance(PenaltyInputIntent::neutral());
        steps += 1;
    }
    let landed = state.ball_pose().position;
    let aim = state.preview.map(|p| world_target(p.target_x, p.target_y)).unwrap();
    assert!(landed.distance(aim) < 0.01, "physics ball lands on the aimed target {aim:?}, landed {landed:?}");
    // It genuinely left the spot along a forward (toward-goal, −Z) flight.
    assert!(landed.z < penalty_spot().z, "the ball travelled toward the goal");
}
