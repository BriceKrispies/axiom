//! The play-selection layer: composing an offensive play against a chosen
//! defensive call, the deterministic-but-varied defensive selector, the huddle
//! call flow through a real run, and the drawable play diagram.

use axiom_end_zone::ai::playcall::call_weights;
use axiom_end_zone::ai::{select_defense, variation_key};
use axiom_end_zone::data::{
    offensive_playbook, showcase_play, DiagramRole, OffenseTag, PlayDiagram,
};
use axiom_end_zone::launch::RunConfig;
use axiom_end_zone::showcase::ShowcaseRun;

#[test]
fn the_showcase_play_is_the_default_offense_against_the_default_call() {
    let play = showcase_play();
    assert_eq!(play.name, "SLOT POST");
    assert_eq!(play.offense_formation.name, "spread");
    assert_eq!(play.defense_formation.name, "base");
}

#[test]
fn the_offensive_playbook_offers_several_distinct_plays() {
    let book = offensive_playbook();
    assert!(book.len() >= 3, "a real playbook has several plays");
    let mut names: Vec<&str> = book.iter().map(|p| p.name).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), book.len(), "play names are unique");
}

#[test]
fn the_selector_is_deterministic_in_its_key() {
    let key = variation_key(0xA11CE, 3, 2);
    let a = select_defense(OffenseTag::DeepPass, 2, 8.0, 3, key);
    let b = select_defense(OffenseTag::DeepPass, 2, 8.0, 3, key);
    assert_eq!(a, b, "same situation + same key ⇒ identical call");
}

#[test]
fn the_defense_does_not_line_up_identically_snap_to_snap() {
    let seed = 0xF00_1234;
    // The same offensive play on consecutive snaps draws different looks: a
    // different call and/or a different alignment — never a pixel-copy.
    let a = select_defense(OffenseTag::DeepPass, 1, 10.0, 3, variation_key(seed, 0, 1));
    let b = select_defense(OffenseTag::DeepPass, 1, 10.0, 3, variation_key(seed, 1, 1));
    assert_ne!(
        a.call.formation.slots, b.call.formation.slots,
        "consecutive snaps present a different defensive picture"
    );
}

#[test]
fn call_weights_make_football_sense() {
    // Never sit in a prevent shell on a short-yardage down.
    let short = call_weights(OffenseTag::QuickPass, 3, 2.0, 3);
    assert_eq!(short[4], 0, "no PREVENT on 3rd-and-2");
    // A long down puts the prevent/zone answers on the table.
    let long = call_weights(OffenseTag::DeepPass, 4, 13.0, 3);
    assert!(long[4] > 0, "PREVENT is viable on 4th-and-long");
    // Heat is the aggression dial: hotter defenses blitz at least as much.
    let cool = call_weights(OffenseTag::QuickPass, 1, 6.0, 1);
    let hot = call_weights(OffenseTag::QuickPass, 1, 6.0, 6);
    assert!(hot[3] >= cool[3], "more heat ⇒ at least as much blitz");
}

#[test]
fn a_called_play_lines_the_offense_up_in_that_play() {
    let mut run = ShowcaseRun::new_run(&RunConfig::new(0x0FFE_0001));
    // Advance to the first open huddle.
    let mut opened = false;
    for _ in 0..2000 {
        run.step(&[]);
        if run.huddle().is_some() {
            opened = true;
            break;
        }
    }
    assert!(opened, "the run opens a huddle before the first snap");

    // Call QUICK SLANTS (index 2) and let the huddle break.
    let want = offensive_playbook()[2].name;
    run.call_play(2);
    for _ in 0..1000 {
        run.step(&[]);
        if run.huddle().is_none() {
            break;
        }
    }
    assert_eq!(run.sim.play.name, want, "the offense runs the called play");
}

#[test]
fn a_hands_off_run_breaks_the_huddle_with_the_default_play() {
    let mut run = ShowcaseRun::new_run(&RunConfig::new(0x0FFE_0002));
    // With no call, the huddle auto-breaks and the default play lines up.
    let default_name = offensive_playbook()[0].name;
    let mut ran_a_play = false;
    for _ in 0..3000 {
        run.step(&[]);
        if run.huddle().is_none() && run.sim.play.name == default_name {
            ran_a_play = true;
            break;
        }
    }
    assert!(ran_a_play, "the huddle breaks on its own with the default play");
}

#[test]
fn the_play_diagram_draws_the_drop_and_the_primary_route() {
    let diagram = PlayDiagram::of(&offensive_playbook()[0]);
    assert_eq!(diagram.name, "SLOT POST");
    assert_eq!(diagram.marks.len(), 7);

    let qb = diagram
        .marks
        .iter()
        .find(|m| m.role == DiagramRole::Quarterback)
        .expect("a quarterback mark");
    assert_eq!(qb.route.len(), 2, "the QB drop is a two-point line");
    assert!(
        qb.route[1].downfield < qb.route[0].downfield,
        "the quarterback drops back"
    );

    let primary = diagram
        .marks
        .iter()
        .find(|m| m.primary)
        .expect("a primary read");
    assert!(primary.route.len() >= 2, "the primary has a route");
    assert_eq!(
        primary.route[0], primary.align,
        "a route polyline starts at the receiver's alignment"
    );

    let receivers = diagram
        .marks
        .iter()
        .filter(|m| m.role == DiagramRole::Receiver)
        .count();
    assert!(receivers >= 3, "the spread fields three receivers");
}
