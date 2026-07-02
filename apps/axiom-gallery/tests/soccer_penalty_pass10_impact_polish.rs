//! Pass 10 proofs: deterministic impact polish (net wobble, post shake, save
//! flash + deflection, miss drift, crowd reaction, camera juice, banner, popup).
//!
//! Every effect is a pure function of `(result, tick, final pose, award)`; all
//! collections are explicit ordered vectors. Determinism is proven by equality
//! across reruns. Effects never change the Pass 8 result or Pass 9 score.

use axiom_gallery::soccer_penalty::penalty_effects::{
    PenaltyImpactEffectKind, GOAL_TICKS, MISS_TICKS, POST_TICKS, SAVE_TICKS, SESSION_COMPLETE_TICKS,
};
use axiom_gallery::soccer_penalty::penalty_render_plan::PenaltyDrawLayer;
use axiom_gallery::soccer_penalty::penalty_result::{
    PenaltyShotResult, PenaltyShotResultDetail, PenaltyShotResultKind,
};
use axiom_gallery::soccer_penalty::penalty_session::PenaltySessionState;
use axiom_gallery::soccer_penalty::{PenaltyInputIntent, SoccerPenaltyApp};
use axiom_math::Vec3;

fn goal() -> PenaltyShotResult {
    PenaltyShotResult { kind: PenaltyShotResultKind::Goal, detail: PenaltyShotResultDetail::Scored }
}
fn save_hand(right: bool) -> PenaltyShotResult {
    let detail = if right {
        PenaltyShotResultDetail::SavedByRightHand
    } else {
        PenaltyShotResultDetail::SavedByLeftHand
    };
    PenaltyShotResult { kind: PenaltyShotResultKind::Save, detail }
}
fn post_crossbar() -> PenaltyShotResult {
    PenaltyShotResult { kind: PenaltyShotResultKind::Post, detail: PenaltyShotResultDetail::HitCrossbar }
}
fn post_left() -> PenaltyShotResult {
    PenaltyShotResult { kind: PenaltyShotResultKind::Post, detail: PenaltyShotResultDetail::HitLeftPost }
}
fn miss() -> PenaltyShotResult {
    PenaltyShotResult { kind: PenaltyShotResultKind::Miss, detail: PenaltyShotResultDetail::MissedRight }
}

/// A session with one forced resolved result (the effect starts at tick 0).
fn resolved(result: PenaltyShotResult, impact: Vec3) -> PenaltySessionState {
    PenaltySessionState::new().record_resolved(0, 50, 50, result, impact)
}
fn tick(mut s: PenaltySessionState, n: u32) -> PenaltySessionState {
    for _ in 0..n {
        s = s.advance(PenaltyInputIntent::neutral());
    }
    s
}

// --- timelines --------------------------------------------------------------

#[test]
fn each_result_creates_its_effect_timeline_with_the_fixed_duration() {
    assert_eq!(resolved(goal(), Vec3::ZERO).effect.unwrap().kind, PenaltyImpactEffectKind::Goal);
    assert_eq!(resolved(save_hand(true), Vec3::ZERO).effect.unwrap().kind, PenaltyImpactEffectKind::Save);
    assert_eq!(resolved(post_crossbar(), Vec3::ZERO).effect.unwrap().kind, PenaltyImpactEffectKind::Post);
    assert_eq!(resolved(miss(), Vec3::ZERO).effect.unwrap().kind, PenaltyImpactEffectKind::Miss);
    assert_eq!(PenaltyImpactEffectKind::Goal.duration(), 72);
    assert_eq!(PenaltyImpactEffectKind::Save.duration(), 54);
    assert_eq!(PenaltyImpactEffectKind::Post.duration(), 54);
    assert_eq!(PenaltyImpactEffectKind::Miss.duration(), 42);
    assert_eq!(PenaltyImpactEffectKind::SessionComplete.duration(), 90);
    assert_eq!((GOAL_TICKS, SAVE_TICKS, POST_TICKS, MISS_TICKS, SESSION_COMPLETE_TICKS), (72, 54, 54, 42, 90));
}

#[test]
fn session_complete_creates_its_effect_timeline() {
    let mut s = PenaltySessionState::new();
    for _ in 0..5 {
        s = s.record_resolved(0, 50, 50, goal(), Vec3::ZERO).advance(PenaltyInputIntent::continuing());
    }
    assert_eq!(s.effect.unwrap().kind, PenaltyImpactEffectKind::SessionComplete);
}

#[test]
fn effect_progress_is_deterministic() {
    let s = tick(resolved(goal(), Vec3::ZERO), 36); // half of 72
    let d = s.effect_descriptor().unwrap();
    assert_eq!(d.tick, 36);
    assert_eq!(d.progress, 500);
}

#[test]
fn effects_clear_on_continue_and_on_reset() {
    let s = resolved(goal(), Vec3::ZERO);
    assert!(s.effect.is_some());
    assert!(s.clone().advance(PenaltyInputIntent::continuing()).effect.is_none());
    assert!(s.advance(PenaltyInputIntent::resetting()).effect.is_none());
}

// --- net wobble -------------------------------------------------------------

#[test]
fn net_wobble_activates_only_for_goal() {
    assert!(resolved(goal(), Vec3::new(0.5, 1.0, 0.0)).effect_descriptor().unwrap().net_wobble.is_some());
    assert!(resolved(save_hand(true), Vec3::ZERO).effect_descriptor().unwrap().net_wobble.is_none());
    assert!(resolved(post_crossbar(), Vec3::ZERO).effect_descriptor().unwrap().net_wobble.is_none());
    assert!(resolved(miss(), Vec3::ZERO).effect_descriptor().unwrap().net_wobble.is_none());
}

#[test]
fn net_wobble_node_ordering_is_stable_and_deterministic() {
    let s = tick(resolved(goal(), Vec3::new(0.5, 1.0, 0.0)), 6);
    let w = s.effect_descriptor().unwrap().net_wobble.unwrap();
    let ordinals: Vec<u32> = w.rear.iter().map(|n| n.ordinal).collect();
    assert_eq!(ordinals, (0..w.rear.len() as u32).collect::<Vec<_>>());
    // Same impact + tick → identical nodes.
    let s2 = tick(resolved(goal(), Vec3::new(0.5, 1.0, 0.0)), 6);
    assert_eq!(w, s2.effect_descriptor().unwrap().net_wobble.unwrap());
}

#[test]
fn wobbled_net_render_items_sort_into_rear_and_front_net() {
    let s = tick(resolved(goal(), Vec3::new(0.5, 1.0, 0.0)), 6);
    let frame = SoccerPenaltyApp::build_session_frame(&s);
    let layer_of = |label: &str| {
        frame.render_plan.items.iter().find(|it| it.label == label).map(|it| it.layer())
    };
    assert_eq!(layer_of("net.wobble.rear"), Some(PenaltyDrawLayer::RearNet));
    assert_eq!(layer_of("net.wobble.front"), Some(PenaltyDrawLayer::FrontNet));
}

// --- post / crossbar shake --------------------------------------------------

#[test]
fn goal_frame_shake_activates_only_for_post_and_targets_the_hit_part() {
    use axiom_gallery::soccer_penalty::penalty_effects::PenaltyGoalFramePart;
    let cb = tick(resolved(post_crossbar(), Vec3::ZERO), 3).effect_descriptor().unwrap().frame_shake.unwrap();
    assert_eq!(cb.target, PenaltyGoalFramePart::Crossbar);
    let lp = tick(resolved(post_left(), Vec3::ZERO), 3).effect_descriptor().unwrap().frame_shake.unwrap();
    assert_eq!(lp.target, PenaltyGoalFramePart::LeftPost);
    // No shake for goal/save/miss.
    assert!(resolved(goal(), Vec3::ZERO).effect_descriptor().unwrap().frame_shake.is_none());
    assert!(resolved(save_hand(true), Vec3::ZERO).effect_descriptor().unwrap().frame_shake.is_none());
    assert!(resolved(miss(), Vec3::ZERO).effect_descriptor().unwrap().frame_shake.is_none());
}

// --- save flash + deflection ------------------------------------------------

#[test]
fn save_creates_impact_flash_and_deflection_without_changing_the_result() {
    let impact = Vec3::new(0.58, 1.0, 0.5);
    let s = tick(resolved(save_hand(true), impact), 4);
    let d = s.effect_descriptor().unwrap();
    // Impact flash at the contact point.
    assert!(!d.foreground.is_empty());
    assert_eq!(d.foreground[0].label, "impact.flash");
    assert_eq!(d.foreground[0].position, impact);
    // A fake deflection exists and starts at the contact point.
    let defl = d.ball_deflection.unwrap();
    assert_eq!(defl.start, impact);
    assert_ne!(defl.end, impact);
    assert!(d.net_wobble.is_none());
    // The Pass 8 result + Pass 9 score are untouched by the effect.
    assert_eq!(s.history[0].result.kind, PenaltyShotResultKind::Save);
    assert_eq!(s.score.score, 0);
}

// --- miss drift -------------------------------------------------------------

#[test]
fn miss_creates_drift_without_wobble_or_shake() {
    let s = tick(resolved(miss(), Vec3::new(4.0, 1.0, 0.0)), 4);
    let d = s.effect_descriptor().unwrap();
    let defl = d.ball_deflection.unwrap();
    assert!(defl.end.z < defl.start.z, "the miss drifts past the goal plane");
    assert!(d.net_wobble.is_none());
    assert!(d.frame_shake.is_none());
}

// --- crowd reaction ---------------------------------------------------------

#[test]
fn crowd_reaction_is_stable_ordered_and_deterministic() {
    let a = tick(resolved(goal(), Vec3::ZERO), 5).effect_descriptor().unwrap().crowd;
    let b = tick(resolved(goal(), Vec3::ZERO), 5).effect_descriptor().unwrap().crowd;
    assert_eq!(a, b);
    // Offsets are a pure function of the stable card ordinal.
    let offsets: Vec<Vec3> = (0..9).map(|o| a.card_offset(o)).collect();
    let offsets2: Vec<Vec3> = (0..9).map(|o| a.card_offset(o)).collect();
    assert_eq!(offsets, offsets2);
    // A goal bounces the crowd more than a miss.
    let miss_crowd = tick(resolved(miss(), Vec3::ZERO), 5).effect_descriptor().unwrap().crowd;
    assert!(a.amplitude > miss_crowd.amplitude);
}

// --- camera juice -----------------------------------------------------------

#[test]
fn camera_juice_is_zero_before_start_nonzero_on_impact_and_zero_after() {
    // Before any effect → zero.
    assert_eq!(PenaltySessionState::new().camera_offset(), Vec3::ZERO);
    // Early impact ticks → nonzero.
    let early = tick(resolved(goal(), Vec3::ZERO), 2).camera_offset();
    assert_ne!(early, Vec3::ZERO);
    // Well past the shake window → back to zero.
    let late = tick(resolved(goal(), Vec3::ZERO), 40).camera_offset();
    assert_eq!(late, Vec3::ZERO);
}

// --- banner + score popup ---------------------------------------------------

#[test]
fn banner_text_matches_result_and_popup_shows_awarded_points() {
    let s = tick(resolved(goal(), Vec3::ZERO), 3);
    let hud = axiom_gallery::soccer_penalty::penalty_hud::PenaltyHudModel::from_session(&s);
    assert_eq!(hud.banner.unwrap().text, "GOAL");
    assert_eq!(hud.score_popup.unwrap().points, 500); // center goal base award
    assert_eq!(hud.score, 500);

    assert_eq!(tick(resolved(save_hand(true), Vec3::ZERO), 3).effect_descriptor().unwrap().banner.text, "SAVE");
    assert_eq!(tick(resolved(post_crossbar(), Vec3::ZERO), 3).effect_descriptor().unwrap().banner.text, "POST");
    assert_eq!(tick(resolved(miss(), Vec3::ZERO), 3).effect_descriptor().unwrap().banner.text, "MISS");
}

#[test]
fn banner_and_popup_render_in_the_hud_layer() {
    let s = tick(resolved(goal(), Vec3::ZERO), 3);
    let frame = SoccerPenaltyApp::build_session_frame(&s);
    // The banner rides the instruction slot (Hud layer, unlit); the popup is a
    // HUD-model descriptor.
    let instruction = frame.render_plan.items.iter().find(|it| it.label == "hud.instruction").unwrap();
    assert_eq!(instruction.layer(), PenaltyDrawLayer::Hud);
    assert!(!instruction.is_lit());
    assert!(frame.hud.banner.is_some());
    assert!(frame.hud.score_popup.is_some());
}

// --- invariants -------------------------------------------------------------

#[test]
fn effects_never_change_score_or_result() {
    let s = tick(resolved(goal(), Vec3::ZERO), 50);
    assert_eq!(s.score.score, 500);
    assert_eq!(s.history[0].result.kind, PenaltyShotResultKind::Goal);
    assert_eq!(s.history.len(), 1);
}

// --- full-flow polish tests -------------------------------------------------

fn goal_effect_history() -> Vec<axiom_gallery::soccer_penalty::PenaltyEffectDescriptor> {
    let mut s = PenaltySessionState::new().record_resolved(0, 50, 50, goal(), Vec3::new(0.4, 1.1, 0.0));
    let mut h = Vec::new();
    for _ in 0..GOAL_TICKS {
        h.push(s.effect_descriptor().unwrap());
        s = s.advance(PenaltyInputIntent::neutral());
    }
    h
}

#[test]
fn full_flow_goal_polish_is_deterministic() {
    let a = goal_effect_history();
    // Net wobble + crowd + banner present throughout; popup reflects the award.
    assert!(a.iter().all(|d| d.net_wobble.is_some()));
    assert!(a[0].crowd.amplitude > 0.0);
    assert_eq!(a[5].banner.text, "GOAL");
    assert_eq!(a[5].score_popup.unwrap().points, 500);
    // Reproducible.
    let b = goal_effect_history();
    assert_eq!(a, b);
}

#[test]
fn full_flow_save_polish_is_deterministic() {
    let impact = Vec3::new(-0.58, 1.0, 0.5);
    let build = || {
        let mut s = PenaltySessionState::new().record_resolved(0, 50, 50, save_hand(false), impact);
        let mut h = Vec::new();
        for _ in 0..SAVE_TICKS {
            h.push(s.effect_descriptor().unwrap());
            s = s.advance(PenaltyInputIntent::neutral());
        }
        (h, s)
    };
    let (a, sa) = build();
    assert!(!a[3].foreground.is_empty());
    assert_eq!(a[3].foreground[0].position, impact);
    assert!(a[3].ball_deflection.is_some());
    assert!(a.iter().all(|d| d.net_wobble.is_none()));
    assert_eq!(a[3].banner.text, "SAVE");
    assert_eq!(sa.score.score, 0);
    let (b, _) = build();
    assert_eq!(a, b);
}

#[test]
fn full_flow_post_polish_is_deterministic() {
    let build = || {
        let mut s = PenaltySessionState::new().record_resolved(0, 50, 50, post_crossbar(), Vec3::new(0.0, 2.4, 0.0));
        let mut h = Vec::new();
        for _ in 0..POST_TICKS {
            h.push(s.effect_descriptor().unwrap());
            s = s.advance(PenaltyInputIntent::neutral());
        }
        (h, s)
    };
    use axiom_gallery::soccer_penalty::penalty_effects::PenaltyGoalFramePart;
    let (a, sa) = build();
    assert_eq!(a[3].frame_shake.unwrap().target, PenaltyGoalFramePart::Crossbar);
    assert!(a.iter().all(|d| d.net_wobble.is_none()));
    assert_eq!(a[3].banner.text, "POST");
    assert_eq!(sa.score.score, 100);
    let (b, _) = build();
    assert_eq!(a, b);
}

#[test]
fn full_flow_miss_polish_is_deterministic() {
    let build = || {
        let mut s = PenaltySessionState::new().record_resolved(100, 50, 50, miss(), Vec3::new(4.5, 1.0, 0.0));
        let mut h = Vec::new();
        for _ in 0..MISS_TICKS {
            h.push(s.effect_descriptor().unwrap());
            s = s.advance(PenaltyInputIntent::neutral());
        }
        (h, s)
    };
    let (a, sa) = build();
    assert!(a[3].ball_deflection.is_some());
    assert!(a.iter().all(|d| d.net_wobble.is_none()));
    assert!(a.iter().all(|d| d.frame_shake.is_none()));
    assert_eq!(a[3].banner.text, "MISS");
    assert_eq!(sa.score.score, 0);
    let (b, _) = build();
    assert_eq!(a, b);
}
