//! Pause semantics over the composed shell: the authoritative simulation does
//! not advance while paused, resume produces no time jump, restart builds a
//! fresh simulation, and returning to the title disposes of the run.

use axiom_end_zone::app::TouchInput;
use axiom_end_zone::frontend::input::FrontendInputFrame;
use axiom_end_zone::frontend::persistence::FrontendProfile;
use axiom_end_zone::frontend::state::Screen;
use axiom_end_zone::shell::EndZoneShell;

fn shell() -> EndZoneShell {
    EndZoneShell::new(0xE2D0, FrontendProfile::default())
}

fn frame(s: &mut EndZoneShell, keys: &[&str]) {
    let input = FrontendInputFrame {
        keys_down: keys.iter().map(|k| k.to_string()).collect(),
        ..Default::default()
    };
    s.frame(&input, TouchInput::default(), 1280.0, 720.0);
}

fn tap(s: &mut EndZoneShell, token: &str) {
    frame(s, &[token]);
    frame(s, &[]);
}

/// Title -> Menu -> PLAY drops straight onto the field. There is no play-call
/// screen: the prototype runs one concept and asks its only question on the
/// field, inside the decision window.
fn start_run(s: &mut EndZoneShell) {
    tap(s, "Enter"); // Title -> Menu
    tap(s, "Enter"); // Menu PLAY -> InGame
    assert_eq!(s.frontend.screen(), Screen::InGame);
}

#[test]
fn play_drops_straight_onto_the_field_with_the_offense_set() {
    let mut s = shell();
    tap(&mut s, "Enter"); // Title -> Menu
    tap(&mut s, "Enter"); // Menu PLAY
    assert_eq!(s.frontend.screen(), Screen::InGame);
    // The offense is lining up; the automatic snap has not fired yet.
    assert_eq!(
        s.app.run.sim.phase,
        axiom_end_zone::state::PlayPhase::PreSnap,
        "the attempt begins with the offense set, not mid-play"
    );
}

#[test]
fn the_simulation_does_not_advance_while_paused() {
    let mut s = shell();
    start_run(&mut s);
    for _ in 0..6 {
        frame(&mut s, &[]);
    }
    tap(&mut s, "KeyP");
    assert_eq!(s.frontend.screen(), Screen::Paused);
    assert!(s.paused());
    let frozen = s.app.run.sim.tick;
    for _ in 0..12 {
        frame(&mut s, &[]);
    }
    assert_eq!(
        s.app.run.sim.tick, frozen,
        "no simulation advance while paused"
    );
}

#[test]
fn resume_produces_no_time_jump() {
    let mut s = shell();
    start_run(&mut s);
    for _ in 0..5 {
        frame(&mut s, &[]);
    }
    tap(&mut s, "KeyP");
    for _ in 0..30 {
        frame(&mut s, &[]); // a long pause
    }
    tap(&mut s, "Escape"); // resume
    assert_eq!(s.frontend.screen(), Screen::InGame);
    let a = s.app.run.sim.tick;
    frame(&mut s, &[]);
    let b = s.app.run.sim.tick;
    assert_eq!(b - a, 1, "one tick per live frame — no catch-up jump");
}

#[test]
fn restart_uses_a_fresh_simulation() {
    let mut s = shell();
    start_run(&mut s);
    for _ in 0..200 {
        frame(&mut s, &[]);
    }
    assert!(s.app.run.sim.tick > 100);
    tap(&mut s, "KeyP");
    tap(&mut s, "ArrowDown"); // RESTART RUN
    tap(&mut s, "Enter");
    // A fresh run drops back onto the field at tick zero.
    assert_eq!(s.frontend.screen(), Screen::InGame);
    assert!(
        s.app.run.sim.tick < 10,
        "restart rebuilds the simulation from tick zero"
    );
}

#[test]
fn return_to_title_disposes_the_run() {
    let mut s = shell();
    start_run(&mut s);
    for _ in 0..10 {
        frame(&mut s, &[]);
    }
    tap(&mut s, "KeyP");
    for _ in 0..5 {
        tap(&mut s, "ArrowDown"); // walk to RETURN TO TITLE
    }
    tap(&mut s, "Enter");
    assert_eq!(s.frontend.screen(), Screen::Title);
    assert!(
        s.app.run.attempt().is_none(),
        "the attempt session is gone; only the ambient showcase remains"
    );
}
