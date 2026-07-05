//! Stage 1 (Pass 1) proofs: determinism, camera stability, HUD contents, and
//! scene composition. Render *ordering* proofs live in
//! `pass2_depth_ordering.rs`.
//!
//! These tests exercise only the public app surface. The core determinism
//! proof is equality across independent rebuilds — a scene that consulted the
//! wall clock or an RNG could not satisfy it.

use axiom_gallery::soccer_penalty::penalty_hud::PenaltyHudModel;
use axiom_gallery::soccer_penalty::penalty_scene::{DioramaRole, ObjectId};
use axiom_gallery::soccer_penalty::static_diorama::{CameraConfig, StaticDiorama};
use axiom_gallery::soccer_penalty::SoccerPenaltyApp;

/// The exact number of primitives the diorama emits. Pinned so an accidental
/// change to the composition is caught. (Pass 7: the goalie is now a 16-part
/// articulated rig instead of the earlier 10-object puppet. Visual-convergence
/// pass: the crowd is now 3 stacked rows of 26 cards (78) rather than 9, the
/// kicker is an 11-part posed figure — neck + hair added — rather than 9 boxes,
/// and the ball carries 6 dark panel spots rather than 2.)
const EXPECTED_OBJECT_COUNT: usize = 246;

#[test]
fn build_is_fully_deterministic() {
    let a = SoccerPenaltyApp::build_stage1();
    let b = SoccerPenaltyApp::build_stage1();
    // Objects, render plan (sorted draw list + camera + lighting), and HUD are
    // all byte-for-byte equal on independent rebuilds — no wall-clock, no RNG.
    assert_eq!(a, b, "two independent Stage 1 builds must be equal");
}

#[test]
fn diorama_descriptor_rebuilds_equal() {
    let a = StaticDiorama::stage1();
    let b = StaticDiorama::stage1();
    assert_eq!(a.objects, b.objects);
    assert_eq!(a.camera, b.camera);
    assert_eq!(a.style_pass, b.style_pass);
    assert_eq!(a.hud, b.hud);
}

#[test]
fn object_list_has_expected_size_and_sequential_ids() {
    let stage1 = SoccerPenaltyApp::build_stage1();
    assert_eq!(stage1.objects.len(), EXPECTED_OBJECT_COUNT);
    // Ids are assigned 0..n in build order, with no gaps or repeats.
    stage1.objects.iter().enumerate().for_each(|(i, o)| {
        assert_eq!(o.id, ObjectId(i as u32), "object {i} has a non-sequential id");
    });
}

#[test]
fn camera_config_is_deterministic_and_fixed() {
    assert_eq!(CameraConfig::stage1(), CameraConfig::stage1());
    let stage1 = SoccerPenaltyApp::build_stage1();
    assert_eq!(stage1.render_plan.camera, CameraConfig::stage1());
    let cam = CameraConfig::stage1();
    // Behind the kicker (positive Z), elevated, aimed at the goal (lower Z).
    assert!(cam.eye.z > cam.target.z, "camera must sit behind its target");
    assert!(cam.eye.y > cam.target.y, "camera must be elevated above its target");
    assert_eq!(cam.fov_y_degrees, 19.5);
    // Forward points toward the goal (-Z).
    assert!(cam.forward().z < 0.0);
}

#[test]
fn hud_model_carries_expected_values() {
    let hud = PenaltyHudModel::stage1();
    assert_eq!(hud.score, 1250);
    assert_eq!(hud.round_current, 3);
    assert_eq!(hud.round_total, 5);
    assert_eq!(hud.best, 2520);
    assert_eq!(hud.power.segments, 10);
    assert_eq!(hud.power.power, 0);
    assert_eq!(hud.power.fill, 0.0, "the default power meter is empty");
    assert!(!hud.power.locked, "default HUD is not in a locked preview");
    assert!(hud.reticle.visible, "aim reticle is visible");
    assert_eq!(hud.instruction, "AIM", "default phase instruction");

    assert_eq!(hud.score_text(), "SCORE 1250");
    assert_eq!(hud.round_text(), "ROUND 3 / 5");
    assert_eq!(hud.best_text(), "BEST 2520");
}

#[test]
fn reticle_sits_over_the_goal() {
    let hud = PenaltyHudModel::stage1();
    // Roughly horizontally centered and in the upper-middle of the frame, where
    // the goal is composed.
    assert!((hud.reticle.position.x - 0.5).abs() < 0.1);
    assert!(hud.reticle.position.y > 0.25 && hud.reticle.position.y < 0.55);
    assert!(hud.reticle.radius > 0.0);
}

#[test]
fn all_required_scene_elements_are_present() {
    let stage1 = SoccerPenaltyApp::build_stage1();
    let has = |role: DioramaRole| stage1.objects.iter().any(|o| o.role == role);
    for role in [
        DioramaRole::Field,
        DioramaRole::FieldLine,
        DioramaRole::PenaltySpot,
        DioramaRole::GoalFrame,
        DioramaRole::RearNet,
        DioramaRole::Kicker,
        DioramaRole::Ball,
        DioramaRole::Goalie,
        DioramaRole::StadiumWall,
        DioramaRole::CrowdCard,
        DioramaRole::AdBoard,
        DioramaRole::BlobShadow,
    ] {
        assert!(has(role), "diorama is missing a {role:?} object");
    }

    // Exactly one ball body (dark panels share the Ball role but not the label).
    let balls = stage1.objects.iter().filter(|o| o.label == "ball").count();
    assert_eq!(balls, 1);

    // The "AXIOM" ad board exists.
    assert!(
        stage1.objects.iter().any(|o| o.label == "ad.board.axiom"),
        "the AXIOM ad board must be present",
    );

    // Three blob shadows: kicker, ball, goalie.
    let shadows = stage1.objects.iter().filter(|o| o.role == DioramaRole::BlobShadow).count();
    assert_eq!(shadows, 3);
}
