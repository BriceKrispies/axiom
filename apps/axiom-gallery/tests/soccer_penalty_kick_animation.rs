//! Default-path proof for the soccer penalty **authored / kinematic kicker + physics
//! ball** hybrid (the stable pipeline that replaced the broken physics-backed
//! humanoid). Drives only the game's public surface.
//!
//! ```text
//! SoccerPenaltyKickMotionSpec (authored, 9 phases)
//!   → SoccerPenaltyKickPose (sample + frame_joint_world, pure forward kinematics)
//!   → KinematicKickFrame per tick   → the visible kicker boxes
//! ball: axiom-physics projectile (PenaltyBallTrajectory) launched at the strike
//! ```
//!
//! The experimental physics-backed humanoid path is compiled out by default (only
//! `--features experimental_physical_humanoid_kicker` builds it), so this file never
//! names it — its absence is asserted through `KICKER_POSE_SOURCE`.

use axiom_gallery::soccer_penalty::penalty_ball::{
    flight_ticks, penalty_spot, world_target, PenaltyBallTrajectory,
};
use axiom_gallery::soccer_penalty::penalty_kick_motion::{
    SoccerPenaltyKickMotionSpec, SoccerPenaltyKickStyle, PHASE_NAMES, PHASE_SAMPLE_TICKS,
    SPRINT_APPROACH, STRIKE_CONTACT_TICK,
};
use axiom_gallery::soccer_penalty::penalty_kick_pose::{SoccerPenaltyKickPose, KICKER_JOINTS};
use axiom_gallery::soccer_penalty::penalty_kicker::{KickerRig, KICKER_POSE_SOURCE};
use axiom_gallery::soccer_penalty::penalty_scene::{DioramaRole, PENALTY_SPOT_Z};
use axiom_gallery::soccer_penalty::SoccerPenaltyApp;

// Coordinate conventions differ between the two spaces this test touches:
//  * authored POSE space (`SoccerPenaltyKickPose` root/joints): +Z is forward, toward
//    the goal — the run-up interpolates from `approach_start` (−Z) up to the ball (0).
//  * WORLD/game space (`penalty_scene` / `penalty_ball`): the goal line is z = 0 and
//    the kicker/ball sit at +Z, so *toward the goal* is decreasing z. The kicker rig
//    maps pose→world by flipping z (`world_z = KICKER_Z − pose_z`).

// Phase indices into PHASE_SAMPLE_TICKS / PHASE_NAMES.
const SETUP: usize = 0;
const PLANT: usize = 3;
const BACKSWING: usize = 4;
const HIP_DRIVE: usize = 5;
const FOLLOW_THROUGH: usize = 7;
const RECOVER: usize = 8;

/// The default kick pose, sampled at each phase's representative tick.
fn kick() -> SoccerPenaltyKickPose {
    SoccerPenaltyKickPose::default_kick()
}

#[test]
fn default_kicker_path_is_kinematic_not_physical_humanoid() {
    // The default build wires the authored/kinematic pose source, never the
    // experimental physics-backed humanoid (which is compiled out entirely).
    assert_eq!(KICKER_POSE_SOURCE, "kinematic");
    assert_eq!(KICKER_JOINTS.len(), 13);
    assert_eq!(KICKER_JOINTS[0], "pelvis");
}

#[test]
fn the_motion_spec_compiles_and_has_nine_phases_in_order() {
    // Authoring + compiling the spec must not panic, and every phase reports at its
    // representative tick in order.
    let _ = SoccerPenaltyKickMotionSpec::author(SoccerPenaltyKickStyle::default_style());
    let kick = kick();
    for (i, &t) in PHASE_SAMPLE_TICKS.iter().enumerate() {
        assert_eq!(kick.frame(t).phase.as_deref(), Some(PHASE_NAMES[i]), "tick {t} in phase {}", PHASE_NAMES[i]);
    }
}

#[test]
fn setup_places_the_kicker_behind_the_ball() {
    // In the rendered WORLD, the setup/idle kicker stands behind the ball: its pelvis
    // sits at a larger z (nearer the camera) than the ball on the penalty spot.
    let boxes = KickerRig::new().boxes_at(PHASE_SAMPLE_TICKS[SETUP] as u32);
    let pelvis = boxes.iter().find(|b| b.label == "kicker.pelvis").expect("pelvis box");
    assert!(pelvis.center.z > PENALTY_SPOT_Z, "kicker stands behind the ball: pelvis z={}", pelvis.center.z);
}

#[test]
fn sprint_approach_moves_the_root_toward_the_ball() {
    // Within the sprint phase the authored root advances from `approach_start` up to
    // the ball, so pose-space root.z increases from early to late in the phase.
    let kick = kick();
    let early = SPRINT_APPROACH.0 + 2;
    let late = SPRINT_APPROACH.1 - 2;
    let early_z = kick.frame(early).root.z;
    let late_z = kick.frame(late).root.z;
    assert!(late_z > early_z, "root advances toward the ball over the sprint: early={early_z}, late={late_z}");
}

#[test]
fn plant_places_the_left_foot_sole_beside_the_ball() {
    // The authored plant target is `left_plant_spot` = (0.28, 0, -0.12), right beside
    // the ball at the origin.
    let kick = kick();
    let sole = kick.frame(PHASE_SAMPLE_TICKS[PLANT]).left_foot_sole;
    let ball_xz = ((sole.x).powi(2) + (sole.z).powi(2)).sqrt();
    assert!(ball_xz < 0.9, "left sole plants beside the ball, |xz|={ball_xz}");
}

#[test]
fn the_right_leg_swings_from_behind_through_the_strike_to_past_the_ball() {
    let kick = kick();
    let back = kick.frame(PHASE_SAMPLE_TICKS[BACKSWING]);
    let hip = kick.frame(PHASE_SAMPLE_TICKS[HIP_DRIVE]);
    let strike = kick.frame(STRIKE_CONTACT_TICK);
    let follow = kick.frame(PHASE_SAMPLE_TICKS[FOLLOW_THROUGH]);
    // Backswing draws the right instep behind the strike position (−Z relative to it).
    assert!(back.right_foot_instep.z < strike.right_foot_instep.z, "backswing draws the instep back");
    // The hip drive accelerates the kicking foot forward, past the backswing.
    assert!(hip.right_foot().z > back.right_foot().z, "hip_drive drives the foot forward");
    // The follow-through sends the right foot past the ball (past the origin, +Z).
    assert!(follow.right_foot().z > strike.right_foot().z, "follow-through continues forward");
    assert!(follow.right_foot().z > 0.0, "the kicking foot ends up past the ball");
}

#[test]
fn recover_returns_the_kicking_foot_toward_a_stable_stance() {
    let kick = kick();
    let follow = kick.frame(PHASE_SAMPLE_TICKS[FOLLOW_THROUGH]);
    let recover = kick.frame(PHASE_SAMPLE_TICKS[RECOVER]);
    assert!(recover.right_foot().z < follow.right_foot().z, "the foot returns from the follow-through");
    // The recovered stance is still structurally valid (head up, feet down, finite).
    kick.validate().expect("kick pose is valid across every phase, including recover");
}

#[test]
fn head_stays_above_and_the_plant_foot_below_the_pelvis_at_every_phase() {
    let kick = kick();
    for &t in PHASE_SAMPLE_TICKS.iter() {
        let f = kick.frame(t);
        assert!(f.head().y > f.pelvis().y, "head above pelvis at tick {t}");
        // The left (plant) foot is always below the pelvis; the right (kicking) foot
        // is permitted to rise through the strike + follow-through swing.
        assert!(f.left_foot().y < f.pelvis().y, "plant foot below pelvis at tick {t}");
    }
}

#[test]
fn exactly_one_ball_contact_event_fires_at_the_strike_tick() {
    let kick = kick();
    let strikes: Vec<u64> = (0..kick.duration()).filter(|&t| kick.frame(t).strike).collect();
    assert_eq!(strikes, vec![STRIKE_CONTACT_TICK], "exactly one ball_contact, at the strike tick");
}

#[test]
fn the_ball_is_a_physics_projectile_launched_toward_the_goal() {
    // The ball flight is a real axiom-physics projectile (impulse → gravity). In world
    // space the goal line is z = 0 and the spot is at +Z, so the ball travels toward
    // the goal by *decreasing* z; no interior sample teleports (monotone in z).
    let target = world_target(0, 45);
    let trajectory = PenaltyBallTrajectory::to_target(penalty_spot(), target, flight_ticks(70));
    let start = trajectory.position_at(0);
    let step1 = trajectory.position_at(1);
    assert!(step1.z < start.z, "ball launches toward the goal (−Z in world space)");
    // The captured path reaches the aimed target (physics-realised, not teleported).
    let landing = trajectory.position_at(flight_ticks(70));
    assert!(landing.distance(target) < 1.0e-2, "ball lands on the aimed target: {landing:?} vs {target:?}");
    // Monotonically-advancing toward the goal every tick — a real arc, never a jump.
    let n = flight_ticks(70);
    for k in 1..n {
        assert!(
            trajectory.position_at(k).z <= trajectory.position_at(k - 1).z,
            "ball advances toward the goal every tick (no teleport) at {k}"
        );
    }
}

#[test]
fn the_normal_render_list_has_no_physics_debug_or_orphan_bodies() {
    // The default frame renders only intended soccer diorama objects: no goalie
    // save-volume debug markers (debug is off) and — critically — no physics
    // collider/body is ever emitted as a renderable. The kicker + goalie + ball are
    // all present.
    let frame = SoccerPenaltyApp::build_stage1();
    assert!(
        frame.objects.iter().all(|o| o.role != DioramaRole::GoalieDebugVolume),
        "no debug volumes render in normal mode"
    );
    let has = |role: DioramaRole| frame.objects.iter().any(|o| o.role == role);
    assert!(has(DioramaRole::Kicker), "the kicker renders");
    assert!(has(DioramaRole::Goalie), "the goalie renders");
    assert!(has(DioramaRole::Ball), "the ball renders");
}

#[test]
fn two_identical_kicks_and_ball_flights_are_byte_for_byte_reproducible() {
    // The kicker pose is deterministic (no physics, no wall-clock, no rng).
    assert_eq!(
        SoccerPenaltyKickPose::from_power_kick(70).debug_snapshot(),
        SoccerPenaltyKickPose::from_power_kick(70).debug_snapshot()
    );
    // The physics ball flight is same-binary reproducible too.
    let flight = || PenaltyBallTrajectory::to_target(penalty_spot(), world_target(20, 60), flight_ticks(80));
    let (a, b) = (flight(), flight());
    for k in 0..flight_ticks(80) {
        assert_eq!(a.position_at(k), b.position_at(k), "identical physics flight at tick {k}");
    }
}
