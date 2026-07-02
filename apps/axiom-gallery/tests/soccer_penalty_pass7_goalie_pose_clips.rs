//! Pass 7 proofs: goalie puppet pose clips + animated save volumes.
//!
//! Rig, clips, sampling, dive-lane selection, and animated volumes are fixed,
//! closed-form, and evaluated over explicit ordered arrays — no maps, no
//! wall-clock, no randomness. Determinism is proven by structural equality
//! across reruns.

use axiom_gallery::soccer_penalty::penalty_goalie::{
    PenaltyGoalieContactDetector, PenaltyGoalieContactKind, PenaltyGoalieDebugDescriptor,
    PenaltyGoalieVolumeKind,
};
use axiom_gallery::soccer_penalty::penalty_goalie_pose::{
    PenaltyGoalieAnimatedVolumeSet, PenaltyGoalieAnimation, PenaltyGoalieAnimationState,
    PenaltyGoalieClipLibrary, PenaltyGoalieDiveLane, PenaltyGoaliePartKind, PenaltyGoaliePose,
    PenaltyGoaliePoseSampler, CLIP_DURATION_TICKS,
};
use axiom_gallery::soccer_penalty::penalty_interaction::{PenaltyInteractionState, PenaltyShotFlightState};
use axiom_gallery::soccer_penalty::{PenaltyInputIntent, SoccerPenaltyApp};
use axiom_math::Vec3;

const EPS: f32 = 1.0e-4;
fn close(a: Vec3, b: Vec3) -> bool {
    (a.x - b.x).abs() < EPS && (a.y - b.y).abs() < EPS && (a.z - b.z).abs() < EPS
}
fn repeat(i: PenaltyInputIntent, n: usize) -> Vec<PenaltyInputIntent> {
    (0..n).map(|_| i).collect()
}

// --- part hierarchy ---------------------------------------------------------

#[test]
fn part_list_is_the_required_stable_order_with_ordinals() {
    use PenaltyGoaliePartKind::*;
    let expected = [
        Root, Pelvis, Torso, Head, LeftUpperArm, LeftForearm, LeftHand, RightUpperArm,
        RightForearm, RightHand, LeftThigh, LeftShin, LeftFoot, RightThigh, RightShin, RightFoot,
    ];
    assert_eq!(PenaltyGoaliePartKind::ALL, expected);
    PenaltyGoaliePartKind::ALL
        .iter()
        .enumerate()
        .for_each(|(i, k)| assert_eq!(k.ordinal(), i as u32));
}

#[test]
fn parent_child_relationships_are_deterministic() {
    use PenaltyGoaliePartKind::*;
    assert_eq!(Root.parent(), None);
    assert_eq!(Pelvis.parent(), Some(Root));
    assert_eq!(Torso.parent(), Some(Pelvis));
    assert_eq!(Head.parent(), Some(Torso));
    assert_eq!(LeftForearm.parent(), Some(LeftUpperArm));
    assert_eq!(LeftHand.parent(), Some(LeftForearm));
    assert_eq!(RightHand.parent(), Some(RightForearm));
    assert_eq!(RightFoot.parent(), Some(RightShin));
    // Every non-root parent precedes its child in ordinal order.
    PenaltyGoaliePartKind::ALL.iter().for_each(|k| {
        if let Some(p) = k.parent() {
            assert!(p.ordinal() < k.ordinal());
        }
    });
}

#[test]
fn idle_pose_resolves_transforms_for_all_parts() {
    let desc = PenaltyGoaliePose::idle().resolve();
    let parts = desc.parts();
    assert_eq!(parts.len(), 16);
    parts.iter().enumerate().for_each(|(i, p)| {
        assert_eq!(p.ordinal, i as u32);
        assert_eq!(p.parent_ordinal, p.kind.parent().map(|k| k.ordinal()));
    });
    // A couple of known world positions.
    assert!(close(desc.world_position(PenaltyGoaliePartKind::RightHand), Vec3::new(0.58, 1.04, 0.5)));
    assert!(close(desc.world_position(PenaltyGoaliePartKind::Torso), Vec3::new(0.0, 1.32, 0.5)));
}

// --- clips + sampling -------------------------------------------------------

#[test]
fn every_dive_clip_exists_with_at_least_five_frames() {
    PenaltyGoalieDiveLane::ALL.iter().for_each(|&lane| {
        let clip = PenaltyGoalieClipLibrary::clip(lane);
        assert_eq!(clip.lane, lane);
        assert!(clip.frames.len() >= 5, "{lane:?} must have >= 5 frames");
        assert_eq!(clip.duration_ticks, CLIP_DURATION_TICKS);
        // Frames are in ascending tick order.
        clip.frames.windows(2).for_each(|w| assert!(w[0].tick <= w[1].tick));
    });
}

#[test]
fn sampling_is_deterministic_and_clamped() {
    let clip = PenaltyGoalieClipLibrary::clip(PenaltyGoalieDiveLane::DiveRightHigh);
    // Same tick → same pose.
    assert_eq!(
        PenaltyGoaliePoseSampler::sample(&clip, 10),
        PenaltyGoaliePoseSampler::sample(&clip, 10)
    );
    // Before the start clamps to the first frame's pose.
    assert_eq!(
        PenaltyGoaliePoseSampler::sample(&clip, 0),
        clip.frames.first().unwrap().pose
    );
    // After the duration clamps to the final frame's pose.
    assert_eq!(
        PenaltyGoaliePoseSampler::sample(&clip, CLIP_DURATION_TICKS + 100),
        clip.frames.last().unwrap().pose
    );
}

// --- dive lane selection ----------------------------------------------------

#[test]
fn dive_lane_selection_matches_the_table() {
    use PenaltyGoalieDiveLane::*;
    assert_eq!(PenaltyGoalieDiveLane::select(-40, 40), DiveLeftLow);
    assert_eq!(PenaltyGoalieDiveLane::select(-40, 60), DiveLeftHigh);
    assert_eq!(PenaltyGoalieDiveLane::select(40, 40), DiveRightLow);
    assert_eq!(PenaltyGoalieDiveLane::select(40, 60), DiveRightHigh);
    assert_eq!(PenaltyGoalieDiveLane::select(0, 50), DiveCenter);
    // Boundaries: |x| must exceed 35 to dive to a side.
    assert_eq!(PenaltyGoalieDiveLane::select(-35, 10), DiveCenter);
    assert_eq!(PenaltyGoalieDiveLane::select(35, 90), DiveCenter);
}

#[test]
fn releasing_chooses_exactly_one_deterministic_lane() {
    let mut seq = repeat(PenaltyInputIntent::aiming(100, 0), 6);
    seq.extend(repeat(PenaltyInputIntent::charging(0, 0), 4));
    seq.push(PenaltyInputIntent::releasing());
    let a = PenaltyInteractionState::run(&seq);
    assert_eq!(a.state, PenaltyShotFlightState::LockedPreview);
    assert!(a.goalie.lane.is_some());
    // Deterministic.
    let b = PenaltyInteractionState::run(&seq);
    assert_eq!(a.goalie.lane, b.goalie.lane);
}

#[test]
fn reset_returns_goalie_animation_to_idle() {
    let mut seq = repeat(PenaltyInputIntent::aiming(100, 0), 6);
    seq.extend(repeat(PenaltyInputIntent::charging(0, 0), 4));
    seq.push(PenaltyInputIntent::releasing());
    let diving = PenaltyInteractionState::run(&seq)
        .advance(PenaltyInputIntent::neutral())
        .advance(PenaltyInputIntent::neutral());
    assert!(matches!(
        diving.goalie.state,
        PenaltyGoalieAnimationState::Diving | PenaltyGoalieAnimationState::TrackingShot
    ));
    let reset = diving.advance(PenaltyInputIntent::resetting());
    assert_eq!(reset.goalie.state, PenaltyGoalieAnimationState::Idle);
    assert_eq!(reset.goalie.lane, None);
    assert_eq!(reset.goalie.clip_tick, 0);
}

// --- animated volumes -------------------------------------------------------

#[test]
fn animated_volumes_follow_their_attached_parts() {
    // A mid-dive pose (right-high), sampled directly.
    let anim = PenaltyGoalieAnimation { state: PenaltyGoalieAnimationState::Diving, lane: Some(PenaltyGoalieDiveLane::DiveRightHigh), clip_tick: 16 };
    let desc = anim.descriptor();
    let set = PenaltyGoalieAnimatedVolumeSet::from_descriptor(&desc).set;
    let center = |kind: PenaltyGoalieVolumeKind| {
        set.volumes().iter().find(|v| v.kind == kind).map(|v| v.center).unwrap()
    };
    assert!(close(center(PenaltyGoalieVolumeKind::LeftHand), desc.world_position(PenaltyGoaliePartKind::LeftHand)));
    assert!(close(center(PenaltyGoalieVolumeKind::RightHand), desc.world_position(PenaltyGoaliePartKind::RightHand)));
    assert!(close(center(PenaltyGoalieVolumeKind::Torso), desc.world_position(PenaltyGoaliePartKind::Torso)));
    // Body attaches to the pelvis.
    assert!(close(center(PenaltyGoalieVolumeKind::Body), desc.world_position(PenaltyGoaliePartKind::Pelvis)));
}

#[test]
fn animated_volume_priority_matches_pass6_order() {
    let desc = PenaltyGoaliePose::idle().resolve();
    let set = PenaltyGoalieAnimatedVolumeSet::from_descriptor(&desc).set;
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
    set.volumes().iter().enumerate().for_each(|(i, v)| assert_eq!(v.ordinal, i as u32));
}

#[test]
fn contact_against_animated_volumes_reports_the_right_kind() {
    let desc = PenaltyGoaliePose::idle().resolve();
    let set = PenaltyGoalieAnimatedVolumeSet::from_descriptor(&desc).set;
    let det = PenaltyGoalieContactDetector::new(set);

    let at = |kind: PenaltyGoaliePartKind| desc.world_position(kind);
    assert_eq!(det.detect(at(PenaltyGoaliePartKind::LeftHand), 0).contact_kind(), PenaltyGoalieContactKind::Hand);
    assert_eq!(det.detect(at(PenaltyGoaliePartKind::RightHand), 0).contact_kind(), PenaltyGoalieContactKind::Hand);
    assert_eq!(det.detect(at(PenaltyGoaliePartKind::Torso), 0).contact_kind(), PenaltyGoalieContactKind::Torso);
    // A low point near the pelvis but away from hands/torso → Body.
    let body = det.detect(Vec3::new(0.55, 0.5, 0.5), 0);
    assert_eq!(body.contact_kind(), PenaltyGoalieContactKind::Body);
    // Right-hand center lies inside body too → hand wins by priority.
    let hand = det.detect(at(PenaltyGoaliePartKind::RightHand), 0);
    assert_eq!(hand.contact.unwrap().volume_kind, PenaltyGoalieVolumeKind::RightHand);
}

// --- determinism of histories ----------------------------------------------

fn record(script: &[PenaltyInputIntent]) -> (Vec<PenaltyGoalieAnimation>, Vec<Option<PenaltyGoalieContactKind>>) {
    let mut s = PenaltyInteractionState::run(script);
    let mut anim = vec![s.goalie];
    let mut contact = vec![s.contact.map(|f| f.contact_kind())];
    let mut steps = 0;
    while !matches!(s.state, PenaltyShotFlightState::ContactDetected | PenaltyShotFlightState::ArrivedAtGoalPlane)
        && steps < 200
    {
        s = s.advance(PenaltyInputIntent::neutral());
        anim.push(s.goalie);
        contact.push(s.contact.map(|f| f.contact_kind()));
        steps += 1;
    }
    (anim, contact)
}

fn right_high_shot() -> Vec<PenaltyInputIntent> {
    let mut seq = repeat(PenaltyInputIntent::aiming(100, 0), 6);
    seq.extend(repeat(PenaltyInputIntent::charging(0, 0), 10));
    seq.push(PenaltyInputIntent::releasing());
    seq
}

#[test]
fn identical_sequences_produce_identical_animation_and_contact_histories() {
    let (anim_a, con_a) = record(&right_high_shot());
    let (anim_b, con_b) = record(&right_high_shot());
    assert_eq!(anim_a, anim_b, "goalie animation histories must be identical");
    assert_eq!(con_a, con_b, "animated contact histories must be identical");
}

// --- debug visualization ----------------------------------------------------

#[test]
fn debug_volumes_are_disabled_by_default() {
    let frame = SoccerPenaltyApp::build_stage1();
    assert!(!frame.render_plan.items.iter().any(|it| it.label == "goalie.debug.volume"));
}

#[test]
fn debug_volumes_follow_animated_parts_deterministically() {
    // A diving state.
    let mut seq = repeat(PenaltyInputIntent::aiming(100, 0), 6);
    seq.extend(repeat(PenaltyInputIntent::charging(0, 0), 4));
    seq.push(PenaltyInputIntent::releasing());
    let diving = (0..8).fold(PenaltyInteractionState::run(&seq), |s, _| s.advance(PenaltyInputIntent::neutral()));

    let frame = SoccerPenaltyApp::build_frame_with_debug(&diving, PenaltyGoalieDebugDescriptor::ENABLED);
    let debug: Vec<_> = frame.render_plan.items.iter().filter(|it| it.label == "goalie.debug.volume").collect();
    assert_eq!(debug.len(), 4);

    // The debug markers sit on the animated volume centers.
    let set = diving.goalie.animated_volumes();
    let centers: Vec<Vec3> = set.volumes().iter().map(|v| v.center).collect();
    debug.iter().for_each(|it| {
        if let axiom_gallery::soccer_penalty::penalty_render_plan::PenaltyRenderContent::World { position, .. } = it.content {
            assert!(centers.iter().any(|c| close(*c, position)), "debug marker must sit on an animated volume");
        }
    });

    // Deterministic rebuild.
    assert_eq!(frame, SoccerPenaltyApp::build_frame_with_debug(&diving, PenaltyGoalieDebugDescriptor::ENABLED));
}

#[test]
fn debug_visualization_does_not_affect_contact() {
    let (contacted, _) = {
        let mut s = PenaltyInteractionState::run(&right_high_shot());
        let mut steps = 0;
        while !matches!(s.state, PenaltyShotFlightState::ContactDetected | PenaltyShotFlightState::ArrivedAtGoalPlane) && steps < 200 {
            s = s.advance(PenaltyInputIntent::neutral());
            steps += 1;
        }
        (s, steps)
    };
    let plain = SoccerPenaltyApp::build_frame_with_debug(&contacted, PenaltyGoalieDebugDescriptor::DISABLED);
    let debug = SoccerPenaltyApp::build_frame_with_debug(&contacted, PenaltyGoalieDebugDescriptor::ENABLED);
    let non_debug: Vec<_> = debug.render_plan.items.iter().filter(|it| it.label != "goalie.debug.volume").collect();
    let plain_items: Vec<_> = plain.render_plan.items.iter().collect();
    assert_eq!(non_debug, plain_items, "debug markers are the only difference");
}

// --- the required full-flow animated save-volume test -----------------------

#[test]
fn full_flow_right_high_dive_catches_on_the_animated_right_hand() {
    let script = right_high_shot();
    let idle_right_hand = PenaltyGoaliePose::idle().resolve().world_position(PenaltyGoaliePartKind::RightHand);

    let run = || {
        let mut s = PenaltyInteractionState::run(&script);
        // The lane is chosen at lock.
        assert_eq!(s.goalie.lane, Some(PenaltyGoalieDiveLane::DiveRightHigh));
        let mut steps = 0;
        while !matches!(s.state, PenaltyShotFlightState::ContactDetected | PenaltyShotFlightState::ArrivedAtGoalPlane) && steps < 200 {
            s = s.advance(PenaltyInputIntent::neutral());
            steps += 1;
        }
        s
    };

    let a = run();
    assert_eq!(a.goalie.lane, Some(PenaltyGoalieDiveLane::DiveRightHigh));

    // The right hand has moved from its idle world position.
    let desc = a.goalie.descriptor();
    let posed_right_hand = desc.world_position(PenaltyGoaliePartKind::RightHand);
    assert!(!close(posed_right_hand, idle_right_hand), "the right hand must have dived");

    // The right-hand save volume follows the right-hand part.
    let set = a.goalie.animated_volumes();
    let rh_center = set.volumes().iter().find(|v| v.kind == PenaltyGoalieVolumeKind::RightHand).unwrap().center;
    assert!(close(rh_center, posed_right_hand));

    // The contact is deterministic: the animated right hand.
    let contact = a.contact.expect("a contact was recorded");
    assert_eq!(contact.contact_kind(), PenaltyGoalieContactKind::Hand);
    assert_eq!(contact.contact.unwrap().volume_kind, PenaltyGoalieVolumeKind::RightHand);
    assert_eq!(contact.tick, 30);

    // Re-run: identical state, pose history, and contact.
    let b = run();
    assert_eq!(a, b);
    assert_eq!(a.contact, b.contact);
}

// --- the required center-dive test ------------------------------------------

#[test]
fn full_flow_center_dive_reaches_landed_then_resets_to_idle() {
    let mut seq = repeat(PenaltyInputIntent::charging(0, 0), 8); // centered aim
    seq.push(PenaltyInputIntent::releasing());
    let landed = (0..CLIP_DURATION_TICKS + 6)
        .fold(PenaltyInteractionState::run(&seq), |s, _| s.advance(PenaltyInputIntent::neutral()));

    assert_eq!(landed.goalie.lane, Some(PenaltyGoalieDiveLane::DiveCenter));
    assert_eq!(landed.goalie.state, PenaltyGoalieAnimationState::Landed);

    let reset = landed.advance(PenaltyInputIntent::resetting());
    assert_eq!(reset.goalie.state, PenaltyGoalieAnimationState::Idle);
    assert_eq!(reset.goalie.lane, None);
}
