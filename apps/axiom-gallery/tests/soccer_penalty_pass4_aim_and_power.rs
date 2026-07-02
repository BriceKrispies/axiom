//! Pass 4 proofs: deterministic aim + shot-power interaction (the ball never
//! moves).
//!
//! Everything is fixed-tick integer arithmetic driven by `PenaltyInputIntent`;
//! determinism is proven by structural equality across independent reruns. No
//! randomness, no wall-clock time, and all construction is over explicit
//! ordered vectors — never a map.

use axiom_gallery::soccer_penalty::penalty_hud::PenaltyHudModel;
use axiom_gallery::soccer_penalty::penalty_interaction::{
    PenaltyInteractionState, PenaltyShotFlightState, PenaltyShotPreview,
};
use axiom_gallery::soccer_penalty::penalty_render_plan::{
    PenaltyDrawLayer, PenaltyHudElement, PenaltyRenderContent,
};
use axiom_gallery::soccer_penalty::{PenaltyInputIntent, SoccerPenaltyApp};

fn repeat(intent: PenaltyInputIntent, n: usize) -> Vec<PenaltyInputIntent> {
    (0..n).map(|_| intent).collect()
}

// --- aim --------------------------------------------------------------------

#[test]
fn default_aim_starts_centered() {
    let s = PenaltyInteractionState::start();
    assert_eq!(s.aim.target_x, 0);
    assert_eq!(s.aim.target_y, 50);
    assert_eq!(s.power.power, 0);
    assert_eq!(s.state, PenaltyShotFlightState::Aiming);
    assert_eq!(s.preview, None);
}

#[test]
fn aim_moves_left_right_up_down_deterministically() {
    let step = PenaltyInteractionState::start();
    // Full-axis motion is 8 target units per tick.
    assert_eq!(step.advance(PenaltyInputIntent::aiming(100, 0)).aim.target_x, 8);
    assert_eq!(step.advance(PenaltyInputIntent::aiming(-100, 0)).aim.target_x, -8);
    assert_eq!(step.advance(PenaltyInputIntent::aiming(0, 100)).aim.target_y, 58);
    assert_eq!(step.advance(PenaltyInputIntent::aiming(0, -100)).aim.target_y, 42);
}

#[test]
fn aim_clamps_at_goal_bounds() {
    let right = PenaltyInteractionState::run(&repeat(PenaltyInputIntent::aiming(100, 0), 40));
    assert_eq!(right.aim.target_x, 100);
    let left = PenaltyInteractionState::run(&repeat(PenaltyInputIntent::aiming(-100, 0), 40));
    assert_eq!(left.aim.target_x, -100);
    let up = PenaltyInteractionState::run(&repeat(PenaltyInputIntent::aiming(0, 100), 40));
    assert_eq!(up.aim.target_y, 100);
    let down = PenaltyInteractionState::run(&repeat(PenaltyInputIntent::aiming(0, -100), 40));
    assert_eq!(down.aim.target_y, 0);
}

// --- power ------------------------------------------------------------------

#[test]
fn power_starts_at_zero() {
    assert_eq!(PenaltyInteractionState::start().power.power, 0);
}

#[test]
fn power_charges_by_fixed_tick_increments() {
    let charge = PenaltyInputIntent::charging(0, 0);
    assert_eq!(PenaltyInteractionState::run(&repeat(charge, 1)).power.power, 8);
    assert_eq!(PenaltyInteractionState::run(&repeat(charge, 2)).power.power, 16);
    assert_eq!(PenaltyInteractionState::run(&repeat(charge, 3)).power.power, 24);
    // Charging moves the phase to Charging but never moves the ball/aim.
    let s = PenaltyInteractionState::run(&repeat(charge, 3));
    assert_eq!(s.state, PenaltyShotFlightState::Charging);
    assert_eq!(s.aim.target_x, 0);
    assert_eq!(s.aim.target_y, 50);
}

#[test]
fn power_clamps_at_100() {
    let s = PenaltyInteractionState::run(&repeat(PenaltyInputIntent::charging(0, 0), 30));
    assert_eq!(s.power.power, 100);
}

// --- release / locked preview ----------------------------------------------

#[test]
fn releasing_creates_a_locked_preview() {
    let mut seq = repeat(PenaltyInputIntent::charging(0, 0), 4);
    seq.push(PenaltyInputIntent::releasing());
    let s = PenaltyInteractionState::run(&seq);
    assert_eq!(s.state, PenaltyShotFlightState::LockedPreview);
    assert_eq!(
        s.preview,
        Some(PenaltyShotPreview { target_x: 0, target_y: 50, power: 32, release_tick: 5 })
    );
}

#[test]
fn locked_preview_freezes_target_and_power() {
    // Charge with a rightward aim for 3 ticks, then release.
    let mut seq = repeat(PenaltyInputIntent::charging(50, 0), 3);
    seq.push(PenaltyInputIntent::releasing());
    let locked = PenaltyInteractionState::run(&seq);

    // The release freezes the exact aim + power the player committed to. (Pass 5
    // launches the ball on the following tick; that transition is tested there.)
    assert_eq!(locked.state, PenaltyShotFlightState::LockedPreview);
    let preview = locked.preview.expect("release must lock a preview");
    assert_eq!(preview.target_x, locked.aim.target_x);
    assert_eq!(preview.target_y, locked.aim.target_y);
    assert_eq!(preview.power, locked.power.power);
    assert_eq!(preview.power, 24); // 3 charge ticks * 8
}

#[test]
fn reset_returns_to_centered_aim_and_zero_power() {
    // From a charging state.
    let charging = PenaltyInteractionState::run(&repeat(PenaltyInputIntent::charging(80, 40), 3));
    let reset = charging.advance(PenaltyInputIntent::resetting());
    assert_eq!(reset.aim.target_x, 0);
    assert_eq!(reset.aim.target_y, 50);
    assert_eq!(reset.power.power, 0);
    assert_eq!(reset.state, PenaltyShotFlightState::Aiming);
    assert_eq!(reset.preview, None);

    // From a locked preview.
    let mut seq = repeat(PenaltyInputIntent::charging(0, 0), 2);
    seq.push(PenaltyInputIntent::releasing());
    let locked = PenaltyInteractionState::run(&seq);
    let reset2 = locked.advance(PenaltyInputIntent::resetting());
    assert_eq!(reset2.state, PenaltyShotFlightState::Aiming);
    assert_eq!(reset2.aim.target_x, 0);
    assert_eq!(reset2.power.power, 0);
    assert_eq!(reset2.preview, None);
}

// --- the required deterministic sequence -----------------------------------

fn scripted_sequence() -> Vec<PenaltyInputIntent> {
    let mut seq = Vec::new();
    seq.extend(repeat(PenaltyInputIntent::aiming(100, 0), 5)); // aim right 5 ticks
    seq.extend(repeat(PenaltyInputIntent::aiming(0, 100), 3)); // aim up 3 ticks
    seq.extend(repeat(PenaltyInputIntent::charging(0, 0), 12)); // hold charge 12 ticks
    seq.push(PenaltyInputIntent::releasing()); // release
    seq
}

#[test]
fn scripted_sequence_locks_expected_preview() {
    let state = PenaltyInteractionState::run(&scripted_sequence());
    assert_eq!(state.state, PenaltyShotFlightState::LockedPreview);
    assert_eq!(state.aim.target_x, 40); // 5 * 8
    assert_eq!(state.aim.target_y, 74); // 50 + 3 * 8
    assert_eq!(state.power.power, 96); // 12 * 8
    assert_eq!(
        state.preview,
        Some(PenaltyShotPreview { target_x: 40, target_y: 74, power: 96, release_tick: 21 })
    );
}

#[test]
fn identical_sequences_produce_identical_states_and_huds() {
    let a = PenaltyInteractionState::run(&scripted_sequence());
    let b = PenaltyInteractionState::run(&scripted_sequence());
    assert_eq!(a, b, "identical intent sequences must produce identical states");
    assert_eq!(
        PenaltyHudModel::from_state(&a),
        PenaltyHudModel::from_state(&b),
        "identical states must produce identical HUD descriptors",
    );
    // And a whole rebuilt frame is identical.
    assert_eq!(SoccerPenaltyApp::build_frame(&a), SoccerPenaltyApp::build_frame(&b));
}

// --- HUD reflects state -----------------------------------------------------

#[test]
fn hud_reflects_aim_and_power_state() {
    // Charging.
    let charging = PenaltyInteractionState::run(&repeat(PenaltyInputIntent::charging(0, 0), 2));
    let hud = PenaltyHudModel::from_state(&charging);
    assert_eq!(hud.power.power, 16);
    assert!((hud.power.fill - 0.16).abs() < 1.0e-6);
    assert!(!hud.power.locked);
    assert_eq!(hud.power.label, "POWER");
    assert_eq!(hud.instruction, "HOLD");

    // Locked.
    let mut seq = repeat(PenaltyInputIntent::charging(0, 0), 2);
    seq.push(PenaltyInputIntent::releasing());
    let locked = PenaltyHudModel::from_state(&PenaltyInteractionState::run(&seq));
    assert!(locked.power.locked);
    assert_eq!(locked.power.label, "LOCKED");
    assert_eq!(locked.instruction, "RELEASE");

    // Aiming right moves the reticle right of center on screen.
    let aimed = PenaltyInteractionState::run(&repeat(PenaltyInputIntent::aiming(100, 0), 4));
    let hud_aim = PenaltyHudModel::from_state(&aimed);
    assert!(hud_aim.reticle.target_x > 0);
    assert!(hud_aim.reticle.position.x > 0.5, "reticle should sit right of center");
}

// --- render ordering preserved ---------------------------------------------

#[test]
fn reticle_and_power_items_stay_in_the_hud_layer() {
    let mut seq = repeat(PenaltyInputIntent::charging(60, 20), 5);
    seq.push(PenaltyInputIntent::releasing());
    let frame = SoccerPenaltyApp::build_frame(&PenaltyInteractionState::run(&seq));

    let element_layer = |want: PenaltyHudElement| {
        frame
            .render_plan
            .items
            .iter()
            .find_map(|it| match it.content {
                PenaltyRenderContent::Hud { element, .. } if element == want => Some(it.layer()),
                _ => None,
            })
            .expect("hud element present")
    };
    assert_eq!(element_layer(PenaltyHudElement::Reticle), PenaltyDrawLayer::Hud);
    assert_eq!(element_layer(PenaltyHudElement::PowerMeter), PenaltyDrawLayer::Hud);
}

#[test]
fn hud_still_sorts_after_all_world_items() {
    let frame = SoccerPenaltyApp::build_frame(&PenaltyInteractionState::run(&scripted_sequence()));
    let last_world = frame
        .render_plan
        .items
        .iter()
        .enumerate()
        .filter(|(_, it)| matches!(it.content, PenaltyRenderContent::World { .. }))
        .map(|(i, _)| i)
        .max()
        .expect("world items");
    let first_hud = frame
        .render_plan
        .items
        .iter()
        .enumerate()
        .filter(|(_, it)| matches!(it.content, PenaltyRenderContent::Hud { .. }))
        .map(|(i, _)| i)
        .min()
        .expect("hud items");
    assert!(first_hud > last_world, "HUD must render after every world item");
}
