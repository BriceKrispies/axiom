//! The HUD view model and the authoritative drive state it derives from: down
//! display, yards-to-go from state, first-down reset, line-to-gain movement,
//! touchdown scoring, bounded heat, and the absence of every removed statistic.

use axiom_end_zone::drive::{DriveEvent, DriveState};
use axiom_end_zone::launch::MAX_HEAT;
use axiom_end_zone::presentation::HudView;

#[test]
fn a_fresh_drive_reads_first_and_ten_from_the_own_25() {
    let d = DriveState::new(1);
    assert_eq!(d.down, 1);
    assert_eq!(d.los_yard, 25.0);
    assert_eq!(d.first_down_yard, 35.0);
    let hud = HudView::from_drive(&d);
    assert_eq!(hud.down_distance, "1ST & 10");
    assert_eq!(hud.score, "SCORE 000000");
    assert_eq!(hud.heat, "HEAT 1");
    assert_eq!(hud.to_gain, "TO GAIN 10");
}

#[test]
fn yards_to_go_is_derived_from_authoritative_state() {
    let mut d = DriveState::new(1);
    let event = d.resolve(30.0); // 5 yards short of the line to gain
    assert_eq!(event, DriveEvent::NextDown);
    assert_eq!(d.down, 2);
    assert_eq!(d.yards_to_go(), 5.0);
    assert_eq!(HudView::from_drive(&d).down_distance, "2ND & 5");
}

#[test]
fn crossing_the_line_to_gain_resets_the_down_count() {
    let mut d = DriveState::new(1);
    d.resolve(30.0); // now 2nd & 5
    assert_eq!(d.down, 2);
    let event = d.resolve(36.0); // past the 35-yard line to gain
    assert_eq!(event, DriveEvent::FirstDown);
    assert_eq!(d.down, 1);
    assert_eq!(d.first_downs, 1);
}

#[test]
fn the_line_to_gain_moves_on_a_first_down() {
    let mut d = DriveState::new(1);
    assert_eq!(d.first_down_yard, 35.0);
    d.resolve(40.0); // first down at the 40
    assert_eq!(d.first_down_yard, 50.0);
    assert_eq!(d.los_yard, 40.0);
}

#[test]
fn a_touchdown_updates_the_score_and_starts_a_new_drive() {
    let mut d = DriveState::new(1);
    let event = d.resolve(100.0);
    assert_eq!(event, DriveEvent::Touchdown);
    assert_eq!(d.touchdowns, 1);
    assert!(d.score >= 700);
    assert_eq!(d.down, 1);
    assert_eq!(d.los_yard, 25.0);
}

#[test]
fn heat_stays_bounded_however_far_the_run_goes() {
    let mut d = DriveState::new(1);
    for _ in 0..50 {
        d.resolve(100.0);
    }
    assert!(d.heat >= 1 && d.heat <= MAX_HEAT);
}

#[test]
fn near_the_goal_line_the_hud_reads_goal() {
    let mut d = DriveState::new(1);
    d.los_yard = 95.0;
    d.first_down_yard = 100.0;
    assert!(d.goal_to_go());
    let hud = HudView::from_drive(&d);
    assert_eq!(hud.down_distance, "1ST & GOAL");
    assert_eq!(hud.to_gain, "GOAL LINE");
}

#[test]
fn the_hud_view_carries_only_the_five_required_readouts() {
    // Exhaustive destructuring pins the shape: any added statistic breaks this
    // test. The assertions confirm every readout is actually populated.
    let HudView {
        score,
        down_distance,
        to_gain,
        heat,
    } = HudView::from_drive(&DriveState::new(3));
    assert!(!score.is_empty(), "score readout is populated");
    assert!(!down_distance.is_empty(), "down/distance readout is populated");
    assert!(!to_gain.is_empty(), "to-gain readout is populated");
    assert!(!heat.is_empty(), "heat readout is populated");
}
