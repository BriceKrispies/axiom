//! Pass 9 proofs: deterministic scoring, round advancement, and the 5-shot
//! playable loop.
//!
//! Scoring is a pure function; the session is a state machine over explicit
//! ordered vectors — no maps, no wall-clock, no randomness. Determinism is
//! proven by structural equality across reruns.

use axiom_gallery::soccer_penalty::penalty_hud::PenaltyHudModel;
use axiom_gallery::soccer_penalty::penalty_render_plan::PenaltyDrawLayer;
use axiom_gallery::soccer_penalty::penalty_result::{PenaltyShotResult, PenaltyShotResultDetail, PenaltyShotResultKind};
use axiom_gallery::soccer_penalty::penalty_scoring::PenaltyScoreRule;
use axiom_gallery::soccer_penalty::penalty_session::{PenaltyLoopState, PenaltySessionState, SESSION_ROUNDS};
use axiom_gallery::soccer_penalty::{PenaltyInputIntent, SoccerPenaltyApp};
use axiom_math::Vec3;

fn goal() -> PenaltyShotResult {
    PenaltyShotResult { kind: PenaltyShotResultKind::Goal, detail: PenaltyShotResultDetail::Scored }
}
fn save() -> PenaltyShotResult {
    PenaltyShotResult { kind: PenaltyShotResultKind::Save, detail: PenaltyShotResultDetail::SavedByBody }
}
fn post() -> PenaltyShotResult {
    PenaltyShotResult { kind: PenaltyShotResultKind::Post, detail: PenaltyShotResultDetail::HitRightPost }
}
fn miss() -> PenaltyShotResult {
    PenaltyShotResult { kind: PenaltyShotResultKind::Miss, detail: PenaltyShotResultDetail::MissedRight }
}
fn award(result: PenaltyShotResult, power: i32, tx: i32, ty: i32, score_before: u32, streak_before: u32) -> axiom_gallery::soccer_penalty::PenaltyScoreAward {
    PenaltyScoreRule::award(1, result, power, tx, ty, score_before, streak_before)
}

// --- fresh session ----------------------------------------------------------

#[test]
fn new_session_starts_at_round_one_score_zero_best_zero() {
    let s = PenaltySessionState::new();
    assert_eq!(s.score.score, 0);
    assert_eq!(s.round_number(), 1);
    assert_eq!(SESSION_ROUNDS, 5);
    assert_eq!(s.best.best, 0);
    assert_eq!(s.loop_state, PenaltyLoopState::RoundAiming);
    assert!(s.history.is_empty());
}

// --- base points ------------------------------------------------------------

#[test]
fn base_points_per_result() {
    assert_eq!(award(goal(), 50, 0, 50, 0, 0).base, 500);
    assert_eq!(award(save(), 50, 0, 50, 0, 0).base, 0);
    assert_eq!(award(post(), 50, 0, 50, 0, 0).base, 100);
    assert_eq!(award(miss(), 50, 0, 50, 0, 0).base, 0);
    // Totals for non-bonus cases.
    assert_eq!(award(goal(), 50, 0, 50, 0, 0).total, 500);
    assert_eq!(award(post(), 50, 0, 50, 0, 0).total, 100);
    assert_eq!(award(save(), 50, 0, 50, 0, 0).total, 0);
    assert_eq!(award(miss(), 50, 0, 50, 0, 0).total, 0);
}

// --- power bonus ------------------------------------------------------------

#[test]
fn power_accuracy_bonus() {
    assert_eq!(award(goal(), 70, 0, 50, 0, 0).power_bonus, 150);
    assert_eq!(award(goal(), 90, 0, 50, 0, 0).power_bonus, 150);
    assert_eq!(award(goal(), 91, 0, 50, 0, 0).power_bonus, 75);
    assert_eq!(award(goal(), 100, 0, 50, 0, 0).power_bonus, 75);
    assert_eq!(award(goal(), 69, 0, 50, 0, 0).power_bonus, 0);
    // Non-goal never gets a power bonus.
    assert_eq!(award(post(), 80, 0, 50, 0, 0).power_bonus, 0);
    assert_eq!(award(save(), 80, 0, 50, 0, 0).power_bonus, 0);
}

// --- placement bonus --------------------------------------------------------

#[test]
fn placement_bonus_by_corner_zone() {
    assert_eq!(award(goal(), 50, -80, 80, 0, 0).placement_bonus, 250); // upper-left
    assert_eq!(award(goal(), 50, 80, 80, 0, 0).placement_bonus, 250); // upper-right
    assert_eq!(award(goal(), 50, -80, 20, 0, 0).placement_bonus, 150); // lower-left
    assert_eq!(award(goal(), 50, 80, 20, 0, 0).placement_bonus, 150); // lower-right
    assert_eq!(award(goal(), 50, 0, 50, 0, 0).placement_bonus, 0); // center
    // Non-goal never gets a placement bonus.
    assert_eq!(award(miss(), 50, 80, 80, 0, 0).placement_bonus, 0);
}

// --- streak bonus -----------------------------------------------------------

#[test]
fn streak_bonus_and_resets() {
    // 1st / 2nd / 3rd consecutive goal.
    assert_eq!(award(goal(), 50, 0, 50, 0, 0).streak_bonus, 0);
    assert_eq!(award(goal(), 50, 0, 50, 0, 1).streak_bonus, 100);
    assert_eq!(award(goal(), 50, 0, 50, 0, 2).streak_bonus, 200);
    assert_eq!(award(goal(), 50, 0, 50, 0, 0).streak_after, 1);
    assert_eq!(award(goal(), 50, 0, 50, 0, 2).streak_after, 3);
    // Non-goal resets the streak to 0.
    assert_eq!(award(save(), 50, 0, 50, 0, 5).streak_after, 0);
    assert_eq!(award(post(), 50, 0, 50, 0, 5).streak_after, 0);
    assert_eq!(award(miss(), 50, 0, 50, 0, 5).streak_after, 0);
}

#[test]
fn award_records_score_before_and_after() {
    let a = award(goal(), 50, 0, 50, 300, 0);
    assert_eq!(a.score_before, 300);
    assert_eq!(a.score_after, 800);
}

// --- award-once + history ---------------------------------------------------

#[test]
fn award_is_applied_exactly_once_and_history_appends_one_item() {
    let s = PenaltySessionState::new().record_resolved(0, 50, 50, goal(), Vec3::ZERO);
    assert_eq!(s.loop_state, PenaltyLoopState::BetweenRounds);
    assert_eq!(s.history.len(), 1);
    assert_eq!(s.score.score, 500);
    assert!(s.last_award.is_some());

    // Ticking again without continuing does not re-award.
    let again = s.clone().advance(PenaltyInputIntent::neutral()).advance(PenaltyInputIntent::neutral());
    assert_eq!(again.history.len(), 1);
    assert_eq!(again.score.score, 500);
    assert_eq!(again.loop_state, PenaltyLoopState::BetweenRounds);
}

#[test]
fn round_history_order_is_stable() {
    let s = PenaltySessionState::new()
        .record_resolved(0, 50, 50, goal(), Vec3::ZERO)
        .advance(PenaltyInputIntent::continuing())
        .record_resolved(0, 50, 50, post(), Vec3::ZERO)
        .advance(PenaltyInputIntent::continuing())
        .record_resolved(0, 50, 50, save(), Vec3::ZERO);
    let kinds: Vec<_> = s.history.iter().map(|r| r.result.kind).collect();
    assert_eq!(
        kinds,
        vec![PenaltyShotResultKind::Goal, PenaltyShotResultKind::Post, PenaltyShotResultKind::Save]
    );
    let rounds: Vec<u32> = s.history.iter().map(|r| r.round_number).collect();
    assert_eq!(rounds, vec![1, 2, 3]);
}

// --- round advancement ------------------------------------------------------

#[test]
fn continue_advances_rounds_then_completes_after_five() {
    let mut s = PenaltySessionState::new();
    for round in 1..=5 {
        assert_eq!(s.round_number(), round);
        s = s.record_resolved(0, 50, 50, save(), Vec3::ZERO);
        assert_eq!(s.loop_state, PenaltyLoopState::BetweenRounds);
        s = s.advance(PenaltyInputIntent::continuing());
    }
    assert_eq!(s.loop_state, PenaltyLoopState::SessionComplete);
    assert_eq!(s.history.len(), 5);
    // Session complete is frozen except for reset.
    let still = s.clone().advance(PenaltyInputIntent::neutral());
    assert_eq!(still.loop_state, PenaltyLoopState::SessionComplete);
}

#[test]
fn next_round_resets_the_shot_but_keeps_score_and_history() {
    let s = PenaltySessionState::new()
        .record_resolved(80, 80, 80, goal(), Vec3::ZERO)
        .advance(PenaltyInputIntent::continuing());
    assert_eq!(s.round_number(), 2);
    assert_eq!(s.loop_state, PenaltyLoopState::RoundAiming);
    // Ball/aim/power/goalie reset for the new round.
    assert_eq!(s.shot, axiom_gallery::soccer_penalty::PenaltyInteractionState::start());
    // Score + history preserved.
    assert_eq!(s.score.score, 900);
    assert_eq!(s.history.len(), 1);
}

// --- session reset + best score --------------------------------------------

#[test]
fn session_reset_clears_score_and_history_but_preserves_best() {
    let s = PenaltySessionState::new().record_resolved(80, 80, 80, goal(), Vec3::ZERO);
    assert_eq!(s.best.best, 900);
    let reset = s.advance(PenaltyInputIntent::resetting());
    assert_eq!(reset.round_number(), 1);
    assert_eq!(reset.score.score, 0);
    assert_eq!(reset.score.streak, 0);
    assert!(reset.history.is_empty());
    assert_eq!(reset.loop_state, PenaltyLoopState::RoundAiming);
    // Best score is preserved.
    assert_eq!(reset.best.best, 900);
}

#[test]
fn best_score_updates_when_current_exceeds_it() {
    let s = PenaltySessionState::new();
    assert_eq!(s.best.best, 0);
    let s = s.record_resolved(0, 50, 50, post(), Vec3::ZERO); // +100
    assert_eq!(s.best.best, 100);
    let s = s.advance(PenaltyInputIntent::continuing()).record_resolved(0, 50, 50, goal(), Vec3::ZERO); // +500
    assert_eq!(s.score.score, 600);
    assert_eq!(s.best.best, 600);
}

// --- HUD --------------------------------------------------------------------

#[test]
fn hud_reflects_score_round_best_award_and_prompts() {
    let s = PenaltySessionState::new().record_resolved(80, 80, 80, goal(), Vec3::ZERO);
    let hud = PenaltyHudModel::from_session(&s);
    assert_eq!(hud.score, 900);
    assert_eq!(hud.round_current, 1);
    assert_eq!(hud.round_total, 5);
    assert_eq!(hud.best, 900);
    assert_eq!(hud.award_text(), Some("+900".to_string()));
    assert_eq!(hud.prompt, Some("CONTINUE"));
    assert!(!hud.session_complete);

    // Session complete HUD.
    let mut done = s;
    for _ in 0..5 {
        done = done.advance(PenaltyInputIntent::continuing());
        if done.loop_state != PenaltyLoopState::SessionComplete {
            done = done.record_resolved(0, 50, 50, save(), Vec3::ZERO);
        }
    }
    let hud = PenaltyHudModel::from_session(&done);
    assert!(hud.session_complete);
    assert_eq!(hud.prompt, Some("PLAY AGAIN"));
    assert_eq!(hud.score, 900);
}

#[test]
fn result_hud_items_stay_in_the_hud_layer() {
    let s = PenaltySessionState::new().record_resolved(80, 80, 80, goal(), Vec3::ZERO);
    let frame = SoccerPenaltyApp::build_session_frame(&s);
    let instruction = frame
        .render_plan
        .items
        .iter()
        .find(|it| it.label == "hud.instruction")
        .expect("instruction present");
    assert_eq!(instruction.layer(), PenaltyDrawLayer::Hud);
    assert!(!instruction.is_lit());
}

// --- scoring must not happen mid-round --------------------------------------

#[test]
fn no_score_change_while_aiming_or_charging() {
    let mut s = PenaltySessionState::new();
    // Aim + charge a few ticks; no shot resolves, so no award.
    for _ in 0..4 {
        s = s.advance(PenaltyInputIntent::aiming(100, 0));
    }
    for _ in 0..4 {
        s = s.advance(PenaltyInputIntent::charging(0, 0));
    }
    assert_eq!(s.score.score, 0);
    assert!(s.history.is_empty());
    assert_eq!(s.loop_state, PenaltyLoopState::RoundCharging);
}

// --- integration: a driven shot awards through advance() --------------------

#[test]
fn a_driven_shot_awards_once_through_advance() {
    // A real Goal shot (clears the keeper, inside the mouth).
    let mut script = (0..7).map(|_| PenaltyInputIntent::aiming(100, 0)).collect::<Vec<_>>();
    script.extend((0..8).map(|_| PenaltyInputIntent::charging(0, 0)));
    script.push(PenaltyInputIntent::releasing());
    let mut s = PenaltySessionState::new();
    for i in &script {
        s = s.advance(*i);
    }
    // Then neutral ticks until the shot resolves and the round is awarded.
    let mut n = 0;
    while s.loop_state != PenaltyLoopState::BetweenRounds && n < 200 {
        s = s.advance(PenaltyInputIntent::neutral());
        n += 1;
    }
    assert_eq!(s.loop_state, PenaltyLoopState::BetweenRounds);
    assert_eq!(s.history.len(), 1);
    assert_eq!(s.history[0].result.kind, PenaltyShotResultKind::Goal);
    assert!(s.score.score >= 500);
}

// --- the required full-session scoring test ---------------------------------

fn play_full_session() -> PenaltySessionState {
    PenaltySessionState::new()
        .record_resolved(80, 80, 80, goal(), Vec3::ZERO) // R1 upper-right goal, power 80
        .advance(PenaltyInputIntent::continuing())
        .record_resolved(0, 50, 95, goal(), Vec3::ZERO) // R2 center goal, power 95
        .advance(PenaltyInputIntent::continuing())
        .record_resolved(0, 50, 50, post(), Vec3::ZERO) // R3 post
        .advance(PenaltyInputIntent::continuing())
        .record_resolved(0, 50, 50, save(), Vec3::ZERO) // R4 save
        .advance(PenaltyInputIntent::continuing())
        .record_resolved(100, 50, 50, miss(), Vec3::ZERO) // R5 miss
        .advance(PenaltyInputIntent::continuing()) // → SessionComplete
}

#[test]
fn full_session_scores_deterministically() {
    let a = play_full_session();

    // Per-round awards.
    let awards: Vec<u32> = a.history.iter().map(|r| r.award.total).collect();
    assert_eq!(awards, vec![900, 675, 100, 0, 0]);
    // R1 = 500 + 150 + 250 + 0; R2 = 500 + 75 + 0 + 100.
    assert_eq!(a.history[0].award.base, 500);
    assert_eq!(a.history[0].award.power_bonus, 150);
    assert_eq!(a.history[0].award.placement_bonus, 250);
    assert_eq!(a.history[0].award.streak_bonus, 0);
    assert_eq!(a.history[1].award.base, 500);
    assert_eq!(a.history[1].award.power_bonus, 75);
    assert_eq!(a.history[1].award.streak_bonus, 100);
    // Streak reset after the post.
    assert_eq!(a.history[2].award.streak_after, 0);

    assert_eq!(a.loop_state, PenaltyLoopState::SessionComplete);
    assert_eq!(a.score.score, 900 + 675 + 100);
    assert_eq!(a.summary().final_score, 1675);
    assert_eq!(a.best.best, 1675);

    // Reproducible: identical history, summary, and HUD.
    let b = play_full_session();
    assert_eq!(a, b);
    assert_eq!(a.summary(), b.summary());
    assert_eq!(PenaltyHudModel::from_session(&a), PenaltyHudModel::from_session(&b));
}
