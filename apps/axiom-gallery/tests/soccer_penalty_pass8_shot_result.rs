//! Pass 8 proofs: deterministic shot result resolution.
//!
//! Resolution is a fixed priority order over explicit ordered arrays (goalie
//! contact → post → goal → miss) — no maps, no wall-clock, no randomness.
//! Determinism is proven by structural equality across reruns.

use axiom_gallery::soccer_penalty::penalty_goalie::PenaltyGoalieContactDetector;
use axiom_gallery::soccer_penalty::penalty_goalie_pose::{
    PenaltyGoalieAnimatedVolumeSet, PenaltyGoaliePartKind, PenaltyGoaliePose,
};
use axiom_gallery::soccer_penalty::penalty_interaction::{PenaltyInteractionState, PenaltyShotFlightState};
use axiom_gallery::soccer_penalty::penalty_render_plan::{PenaltyDrawLayer, PenaltyRenderContent};
use axiom_gallery::soccer_penalty::penalty_result::{
    PenaltyGoalFrameVolumeKind, PenaltyGoalFrameVolumeSet, PenaltyGoalMouth,
    PenaltyGoalPlaneCrossing,
};
use axiom_gallery::soccer_penalty::penalty_scene::{BALL_RADIUS, GOAL_HALF_WIDTH, GOAL_HEIGHT, GOAL_LINE_Z};
use axiom_gallery::soccer_penalty::{
    PenaltyInputIntent, PenaltyShotResultDetail, PenaltyShotResultKind, PenaltyShotResultResolver,
    SoccerPenaltyApp,
};
use axiom_math::Vec3;

fn repeat(i: PenaltyInputIntent, n: usize) -> Vec<PenaltyInputIntent> {
    (0..n).map(|_| i).collect()
}

/// Drive an aim-right / aim-up / charge / release script to `Resolved`.
fn resolve_shot(right: usize, up: usize, charge: usize) -> PenaltyInteractionState {
    let mut seq = repeat(PenaltyInputIntent::aiming(100, 0), right);
    seq.extend(repeat(PenaltyInputIntent::aiming(0, 100), up));
    seq.extend(repeat(PenaltyInputIntent::charging(0, 0), charge));
    seq.push(PenaltyInputIntent::releasing());
    let mut s = PenaltyInteractionState::run(&seq);
    let mut n = 0;
    while s.state != PenaltyShotFlightState::Resolved && n < 500 {
        s = s.advance(PenaltyInputIntent::neutral());
        n += 1;
    }
    s
}

fn result(s: &PenaltyInteractionState) -> (PenaltyShotResultKind, PenaltyShotResultDetail) {
    let r = s.resolved.expect("resolved").result;
    (r.kind, r.detail)
}

// --- resolver unit tests (contact) ------------------------------------------

#[test]
fn every_hand_torso_body_contact_resolves_to_a_keyed_save() {
    // Detect against the idle animated volumes at each part center, then resolve.
    let desc = PenaltyGoaliePose::idle().resolve();
    let det = PenaltyGoalieContactDetector::new(
        PenaltyGoalieAnimatedVolumeSet::from_descriptor(&desc).set,
    );
    let resolve_at = |kind: PenaltyGoaliePartKind| {
        let f = det.detect(desc.world_position(kind), 0);
        PenaltyShotResultResolver::from_contact(&f)
    };
    let cases = [
        (PenaltyGoaliePartKind::LeftHand, PenaltyShotResultDetail::SavedByLeftHand),
        (PenaltyGoaliePartKind::RightHand, PenaltyShotResultDetail::SavedByRightHand),
        (PenaltyGoaliePartKind::Torso, PenaltyShotResultDetail::SavedByTorso),
    ];
    cases.iter().for_each(|&(kind, detail)| {
        let r = resolve_at(kind);
        assert_eq!(r.kind, PenaltyShotResultKind::Save);
        assert_eq!(r.detail, detail);
    });
    // A low point in the pelvis/body only → SavedByBody.
    let body = det.detect(Vec3::new(0.55, 0.5, 0.5), 0);
    let r = PenaltyShotResultResolver::from_contact(&body);
    assert_eq!(r.kind, PenaltyShotResultKind::Save);
    assert_eq!(r.detail, PenaltyShotResultDetail::SavedByBody);
}

// --- resolver unit tests (crossing) -----------------------------------------

fn crossing_at(x: f32, y: f32) -> PenaltyGoalPlaneCrossing {
    PenaltyGoalPlaneCrossing::at(0, Vec3::new(x, y, GOAL_LINE_Z), 0, 0)
}

#[test]
fn crossing_inside_mouth_is_a_goal() {
    let r = PenaltyShotResultResolver::from_crossing(&crossing_at(0.0, GOAL_HEIGHT * 0.5));
    assert_eq!(r.kind, PenaltyShotResultKind::Goal);
    assert_eq!(r.detail, PenaltyShotResultDetail::Scored);
}

#[test]
fn crossing_on_the_frame_is_a_post() {
    // Right post.
    let r = PenaltyShotResultResolver::from_crossing(&crossing_at(GOAL_HALF_WIDTH, GOAL_HEIGHT * 0.5));
    assert_eq!(r.kind, PenaltyShotResultKind::Post);
    assert_eq!(r.detail, PenaltyShotResultDetail::HitRightPost);
    // Left post.
    let l = PenaltyShotResultResolver::from_crossing(&crossing_at(-GOAL_HALF_WIDTH, GOAL_HEIGHT * 0.5));
    assert_eq!(l.detail, PenaltyShotResultDetail::HitLeftPost);
    // Crossbar.
    let c = PenaltyShotResultResolver::from_crossing(&crossing_at(0.0, GOAL_HEIGHT));
    assert_eq!(c.kind, PenaltyShotResultKind::Post);
    assert_eq!(c.detail, PenaltyShotResultDetail::HitCrossbar);
}

#[test]
fn crossing_outside_the_mouth_is_a_keyed_miss() {
    let left = PenaltyShotResultResolver::from_crossing(&crossing_at(-(GOAL_HALF_WIDTH + 1.0), 1.0));
    assert_eq!(left.kind, PenaltyShotResultKind::Miss);
    assert_eq!(left.detail, PenaltyShotResultDetail::MissedLeft);
    let right = PenaltyShotResultResolver::from_crossing(&crossing_at(GOAL_HALF_WIDTH + 1.0, 1.0));
    assert_eq!(right.detail, PenaltyShotResultDetail::MissedRight);
    let high = PenaltyShotResultResolver::from_crossing(&crossing_at(0.0, GOAL_HEIGHT + 1.0));
    assert_eq!(high.detail, PenaltyShotResultDetail::MissedHigh);
}

#[test]
fn post_takes_priority_over_goal() {
    // A ball center just inside the mouth but overlapping the right post → Post.
    let x = GOAL_HALF_WIDTH - 0.2;
    let crossing = crossing_at(x, GOAL_HEIGHT * 0.5);
    assert!(crossing.inside_mouth, "center is inside the mouth");
    assert!(crossing.frame_hit.is_some(), "but it also overlaps a post");
    let r = PenaltyShotResultResolver::from_crossing(&crossing);
    assert_eq!(r.kind, PenaltyShotResultKind::Post);
}

#[test]
fn goal_frame_volumes_are_ordered_and_overlap_the_ball() {
    let set = PenaltyGoalFrameVolumeSet::stage1();
    let kinds: Vec<_> = set.volumes().iter().map(|v| v.kind).collect();
    assert_eq!(
        kinds,
        vec![
            PenaltyGoalFrameVolumeKind::LeftPost,
            PenaltyGoalFrameVolumeKind::RightPost,
            PenaltyGoalFrameVolumeKind::Crossbar,
        ]
    );
    assert_eq!(set.first_hit(Vec3::new(GOAL_HALF_WIDTH, 1.0, 0.0), BALL_RADIUS), Some(PenaltyGoalFrameVolumeKind::RightPost));
    assert_eq!(set.first_hit(Vec3::new(0.0, 5.0, 0.0), BALL_RADIUS), None);
    // The goal mouth uses the true frame dimensions.
    let mouth = PenaltyGoalMouth::stage1();
    assert_eq!(mouth.left_post_x, -GOAL_HALF_WIDTH);
    assert_eq!(mouth.right_post_x, GOAL_HALF_WIDTH);
    assert_eq!(mouth.crossbar_y, GOAL_HEIGHT);
    assert!(mouth.contains_center(0.0, 1.0));
    assert!(!mouth.contains_center(0.0, GOAL_HEIGHT + 0.5));
}

// --- priority: goalie contact over post -------------------------------------

#[test]
fn goalie_contact_takes_priority_over_post_when_it_happens_first() {
    // A shot the diving keeper catches before the ball reaches the frame.
    let s = resolve_shot(6, 0, 10);
    assert_eq!(result(&s).0, PenaltyShotResultKind::Save);
    assert!(s.contact.is_some(), "contact happened during flight, before arrival");
}

// --- full-flow GOAL ---------------------------------------------------------

#[test]
fn full_flow_goal_is_deterministic_and_leaves_score_unchanged() {
    let a = resolve_shot(7, 0, 8);
    assert_eq!(result(&a).0, PenaltyShotResultKind::Goal);
    // Score/round/best are untouched in Pass 8.
    let hud = SoccerPenaltyApp::build_frame(&a).hud;
    assert_eq!(hud.score, 1250);
    assert_eq!(hud.round_current, 3);
    assert_eq!(hud.best, 2520);
    assert_eq!(hud.result.unwrap().result_text, "GOAL");

    let b = resolve_shot(7, 0, 8);
    assert_eq!(a, b);
    assert_eq!(
        a.resolved.unwrap().final_ball_position,
        b.resolved.unwrap().final_ball_position
    );
}

// --- full-flow SAVE ---------------------------------------------------------

#[test]
fn full_flow_save_is_deterministic_with_the_expected_detail() {
    let a = resolve_shot(6, 0, 10);
    let (kind, detail) = result(&a);
    assert_eq!(kind, PenaltyShotResultKind::Save);
    assert_eq!(detail, PenaltyShotResultDetail::SavedByRightHand);
    assert_eq!(SoccerPenaltyApp::build_frame(&a).hud.result.unwrap().result_text, "SAVE");
    assert_eq!(SoccerPenaltyApp::build_frame(&a).hud.score, 1250);

    let b = resolve_shot(6, 0, 10);
    assert_eq!(a, b);
    assert_eq!(a.contact, b.contact);
    assert_eq!(a.resolved.unwrap().final_ball_position, b.resolved.unwrap().final_ball_position);
}

// --- full-flow POST ---------------------------------------------------------

#[test]
fn full_flow_post_is_deterministic_with_the_expected_detail() {
    let a = resolve_shot(10, 0, 8);
    let (kind, detail) = result(&a);
    assert_eq!(kind, PenaltyShotResultKind::Post);
    assert_eq!(detail, PenaltyShotResultDetail::HitRightPost);
    assert_eq!(SoccerPenaltyApp::build_frame(&a).hud.result.unwrap().result_text, "POST");
    assert_eq!(SoccerPenaltyApp::build_frame(&a).hud.score, 1250);

    let b = resolve_shot(10, 0, 8);
    assert_eq!(a, b);
    assert_eq!(a.resolved.unwrap().final_ball_position, b.resolved.unwrap().final_ball_position);
}

// --- full-flow MISS ---------------------------------------------------------

#[test]
fn full_flow_miss_is_deterministic_with_the_expected_detail() {
    let a = resolve_shot(13, 0, 8);
    let (kind, detail) = result(&a);
    assert_eq!(kind, PenaltyShotResultKind::Miss);
    assert_eq!(detail, PenaltyShotResultDetail::MissedRight);
    assert_eq!(SoccerPenaltyApp::build_frame(&a).hud.result.unwrap().result_text, "MISS");
    assert_eq!(SoccerPenaltyApp::build_frame(&a).hud.score, 1250);

    let b = resolve_shot(13, 0, 8);
    assert_eq!(a, b);
    assert_eq!(a.resolved.unwrap().final_ball_position, b.resolved.unwrap().final_ball_position);
}

// --- resolved freeze + reset + HUD layer ------------------------------------

#[test]
fn resolved_state_freezes_and_reset_clears_it() {
    let a = resolve_shot(7, 0, 8);
    assert_eq!(a.state, PenaltyShotFlightState::Resolved);
    let frozen = a.advance(PenaltyInputIntent::neutral()).advance(PenaltyInputIntent::neutral());
    assert_eq!(frozen.state, PenaltyShotFlightState::Resolved);
    assert_eq!(frozen.resolved, a.resolved);
    assert_eq!(frozen.ball_pose().position, a.ball_pose().position);

    let reset = a.advance(PenaltyInputIntent::resetting());
    assert_eq!(reset.state, PenaltyShotFlightState::Aiming);
    assert_eq!(reset.resolved, None);
}

#[test]
fn result_hud_renders_in_the_hud_layer_and_score_is_unchanged() {
    let a = resolve_shot(7, 0, 8);
    let frame = SoccerPenaltyApp::build_frame(&a);
    // The instruction slot (which carries the result word) is in the Hud layer.
    let instruction = frame
        .render_plan
        .items
        .iter()
        .find(|it| it.label == "hud.instruction")
        .expect("instruction present");
    assert_eq!(instruction.layer(), PenaltyDrawLayer::Hud);
    assert!(!instruction.is_lit(), "HUD is unlit");
    assert!(matches!(instruction.content, PenaltyRenderContent::Hud { .. }));
    // The scoreboard is unchanged across all resolved results.
    [resolve_shot(7, 0, 8), resolve_shot(6, 0, 10), resolve_shot(10, 0, 8), resolve_shot(13, 0, 8)]
        .iter()
        .for_each(|s| {
            let hud = SoccerPenaltyApp::build_frame(s).hud;
            assert_eq!(hud.score, 1250);
            assert_eq!(hud.round_current, 3);
            assert_eq!(hud.best, 2520);
        });
}
