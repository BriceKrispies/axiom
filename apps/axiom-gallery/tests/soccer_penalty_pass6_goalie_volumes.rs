//! Pass 6 proofs: deterministic goalie save volumes + contact detection.
//!
//! Volumes and detection are fixed, closed-form, and evaluated over explicit
//! ordered arrays in strict priority order — no maps, no wall-clock time, no
//! randomness. Determinism is proven by structural equality across reruns.

use axiom_gallery::soccer_penalty::penalty_goalie::{
    PenaltyGoalieContactDetector, PenaltyGoalieContactKind, PenaltyGoalieDebugDescriptor,
    PenaltyGoalieVolumeKind, PenaltyGoalieVolumeSet,
};
use axiom_gallery::soccer_penalty::penalty_interaction::{PenaltyInteractionState, PenaltyShotFlightState};
use axiom_gallery::soccer_penalty::penalty_render_plan::PenaltyDrawLayer;
use axiom_gallery::soccer_penalty::penalty_scene::BALL_RADIUS;
use axiom_gallery::soccer_penalty::{PenaltyInputIntent, SoccerPenaltyApp};
use axiom_math::Vec3;

fn repeat(intent: PenaltyInputIntent, n: usize) -> Vec<PenaltyInputIntent> {
    (0..n).map(|_| intent).collect()
}

fn center_of(kind: PenaltyGoalieVolumeKind) -> Vec3 {
    PenaltyGoalieVolumeSet::stage1()
        .volumes()
        .iter()
        .find(|v| v.kind == kind)
        .map(|v| v.center)
        .expect("volume present")
}

// --- volume set -------------------------------------------------------------

#[test]
fn volume_set_has_the_four_volumes_in_priority_order() {
    let set = PenaltyGoalieVolumeSet::stage1();
    let kinds: Vec<PenaltyGoalieVolumeKind> = set.volumes().iter().map(|v| v.kind).collect();
    assert_eq!(
        kinds,
        vec![
            PenaltyGoalieVolumeKind::LeftHand,
            PenaltyGoalieVolumeKind::RightHand,
            PenaltyGoalieVolumeKind::Torso,
            PenaltyGoalieVolumeKind::Body,
        ]
    );
    // The declaration order IS the priority order (strictly increasing).
    kinds.windows(2).for_each(|w| assert!(w[0] < w[1]));
}

#[test]
fn volume_ordinals_are_stable() {
    let set = PenaltyGoalieVolumeSet::stage1();
    set.volumes().iter().enumerate().for_each(|(i, v)| assert_eq!(v.ordinal, i as u32));
    // Rebuilding gives the same set.
    assert_eq!(PenaltyGoalieVolumeSet::stage1(), set);
}

// --- per-volume contact -----------------------------------------------------

#[test]
fn ball_at_each_volume_center_reports_the_expected_contact() {
    let det = PenaltyGoalieContactDetector::stage1();

    let left = det.detect(center_of(PenaltyGoalieVolumeKind::LeftHand), 0);
    assert_eq!(left.contact_kind(), PenaltyGoalieContactKind::Hand);
    assert_eq!(left.contact.unwrap().volume_kind, PenaltyGoalieVolumeKind::LeftHand);

    let right = det.detect(center_of(PenaltyGoalieVolumeKind::RightHand), 0);
    assert_eq!(right.contact_kind(), PenaltyGoalieContactKind::Hand);
    assert_eq!(right.contact.unwrap().volume_kind, PenaltyGoalieVolumeKind::RightHand);

    // Torso center is also inside Body, but Torso has higher priority.
    let torso = det.detect(center_of(PenaltyGoalieVolumeKind::Torso), 0);
    assert_eq!(torso.contact_kind(), PenaltyGoalieContactKind::Torso);
    assert_eq!(torso.contact.unwrap().volume_kind, PenaltyGoalieVolumeKind::Torso);

    // A low point inside Body only.
    let body = det.detect(Vec3::new(0.6, 0.2, 0.5), 0);
    assert_eq!(body.contact_kind(), PenaltyGoalieContactKind::Body);
    assert_eq!(body.contact.unwrap().volume_kind, PenaltyGoalieVolumeKind::Body);
}

#[test]
fn no_overlap_reports_no_contact() {
    let det = PenaltyGoalieContactDetector::stage1();
    let frame = det.detect(Vec3::new(0.0, 5.0, 0.5), 7);
    assert_eq!(frame.contact_kind(), PenaltyGoalieContactKind::None);
    assert!(frame.contact.is_none());
    assert_eq!(frame.tick, 7);
}

#[test]
fn priority_prefers_hand_and_torso_over_body_on_the_same_tick() {
    let det = PenaltyGoalieContactDetector::stage1();
    // The right-hand center lies inside Body too → Hand wins.
    let hand = det.detect(center_of(PenaltyGoalieVolumeKind::RightHand), 0);
    assert_eq!(hand.contact.unwrap().volume_kind, PenaltyGoalieVolumeKind::RightHand);
    // The torso center lies inside Body too → Torso wins.
    let torso = det.detect(center_of(PenaltyGoalieVolumeKind::Torso), 0);
    assert_eq!(torso.contact.unwrap().volume_kind, PenaltyGoalieVolumeKind::Torso);
}

#[test]
fn detection_uses_the_fixed_ball_radius_and_is_repeatable() {
    let det = PenaltyGoalieContactDetector::stage1();
    let p = center_of(PenaltyGoalieVolumeKind::LeftHand);
    let a = det.detect(p, 3);
    let b = det.detect(p, 3);
    assert_eq!(a, b, "identical ball positions produce identical contact frames");
    assert_eq!(a.ball_radius, BALL_RADIUS);
}

// --- reset ------------------------------------------------------------------

#[test]
fn reset_clears_contact_information() {
    let (contacted, _) = fly_until_contact(right_hand_shot());
    assert!(contacted.contact.is_some());
    let reset = contacted.advance(PenaltyInputIntent::resetting());
    assert_eq!(reset.state, PenaltyShotFlightState::Aiming);
    assert!(reset.contact.is_none());
    assert!(reset.flight.is_none());
}

// --- debug visualization ----------------------------------------------------

#[test]
fn debug_visualization_is_disabled_by_default() {
    let frame = SoccerPenaltyApp::build_stage1();
    assert!(!frame.render_plan.items.iter().any(|it| it.label == "goalie.debug.volume"));
    assert!(frame.hud.debug_contact.is_none());
    // Explicit disabled build matches the default build.
    let disabled =
        SoccerPenaltyApp::build_frame_with_debug(&PenaltyInteractionState::start(), PenaltyGoalieDebugDescriptor::DISABLED);
    assert_eq!(disabled, frame);
}

#[test]
fn debug_visualization_when_enabled_is_stable_and_in_foreground_effects() {
    let state = PenaltyInteractionState::start();
    let frame = SoccerPenaltyApp::build_frame_with_debug(&state, PenaltyGoalieDebugDescriptor::ENABLED);
    let debug: Vec<_> = frame.render_plan.items.iter().filter(|it| it.label == "goalie.debug.volume").collect();
    assert_eq!(debug.len(), 4, "one marker per goalie volume");
    debug.iter().for_each(|it| assert_eq!(it.layer(), PenaltyDrawLayer::ForegroundEffects));
    // Deterministic across rebuilds.
    assert_eq!(frame, SoccerPenaltyApp::build_frame_with_debug(&state, PenaltyGoalieDebugDescriptor::ENABLED));
    // The HUD carries a neutral debug label (never a final result word).
    assert_eq!(frame.hud.debug_contact, Some("NONE"));
}

#[test]
fn debug_visualization_does_not_affect_contact_results() {
    // The contact lives in the interaction state, computed before any rendering.
    let (contacted, _) = fly_until_contact(right_hand_shot());
    let plain = SoccerPenaltyApp::build_frame_with_debug(&contacted, PenaltyGoalieDebugDescriptor::DISABLED);
    let debug = SoccerPenaltyApp::build_frame_with_debug(&contacted, PenaltyGoalieDebugDescriptor::ENABLED);
    // Same HUD instruction, same ball, same contact — only debug items differ.
    assert_eq!(plain.hud.instruction, debug.hud.instruction);
    let non_debug: Vec<_> = debug.render_plan.items.iter().filter(|it| it.label != "goalie.debug.volume").collect();
    let plain_items: Vec<_> = plain.render_plan.items.iter().collect();
    assert_eq!(non_debug, plain_items, "debug markers are the only difference");
    // The debug label reflects the (already-decided) contact.
    assert_eq!(debug.hud.debug_contact, Some("HAND"));
}

// --- shared shot sequences --------------------------------------------------

/// A shot the Pass 7 diving keeper catches on its (animated) right hand
/// (discovered deterministically).
fn right_hand_shot() -> Vec<PenaltyInputIntent> {
    let mut seq = repeat(PenaltyInputIntent::aiming(100, 0), 6); // aim right (→ DiveRightHigh)
    seq.extend(repeat(PenaltyInputIntent::charging(0, 0), 10)); // charge 10
    seq.push(PenaltyInputIntent::releasing());
    seq
}

/// A shot aimed at the top-right corner, clear of every goalie volume.
fn clear_shot() -> Vec<PenaltyInputIntent> {
    let mut seq = repeat(PenaltyInputIntent::aiming(100, 0), 12); // aim far right
    seq.extend(repeat(PenaltyInputIntent::aiming(0, 100), 5)); // aim up
    seq.extend(repeat(PenaltyInputIntent::charging(0, 0), 6));
    seq.push(PenaltyInputIntent::releasing());
    seq
}

fn fly_until_contact(script: Vec<PenaltyInputIntent>) -> (PenaltyInteractionState, u32) {
    let mut state = PenaltyInteractionState::run(&script);
    let mut steps = 0;
    while !matches!(
        state.state,
        PenaltyShotFlightState::ContactDetected | PenaltyShotFlightState::ArrivedAtGoalPlane
    ) && steps < 500
    {
        state = state.advance(PenaltyInputIntent::neutral());
        steps += 1;
    }
    (state, steps)
}

// --- full-flow contact test -------------------------------------------------

#[test]
fn full_flow_right_hand_contact_is_deterministic() {
    let (a, steps_a) = fly_until_contact(right_hand_shot());

    assert_eq!(a.state, PenaltyShotFlightState::ContactDetected);
    let contact = a.contact.expect("a contact was recorded");
    assert_eq!(contact.contact_kind(), PenaltyGoalieContactKind::Hand);
    let hit = contact.contact.expect("contact present");
    assert_eq!(hit.volume_kind, PenaltyGoalieVolumeKind::RightHand);
    // The exact deterministic contact tick (shot-local flight tick) against the
    // animated (diving) right hand.
    assert_eq!(contact.tick, 30);

    // Re-run from a fresh state: identical contact frame and final state.
    let (b, steps_b) = fly_until_contact(right_hand_shot());
    assert_eq!(a, b, "the same shot must reproduce the same state");
    assert_eq!(a.contact, b.contact, "identical contact frames");
    assert_eq!(steps_a, steps_b);
}

// --- full-flow no-contact test ----------------------------------------------

#[test]
fn full_flow_clear_shot_records_no_contact() {
    let (a, steps_a) = fly_until_contact(clear_shot());
    assert_eq!(a.state, PenaltyShotFlightState::ArrivedAtGoalPlane);
    assert!(a.contact.is_none(), "a clear shot touches no goalie volume");

    let (b, steps_b) = fly_until_contact(clear_shot());
    assert_eq!(a, b, "the same clear shot must reproduce the same state");
    assert_eq!(steps_a, steps_b);
}
