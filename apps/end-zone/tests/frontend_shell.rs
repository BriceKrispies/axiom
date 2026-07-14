//! The composed shell: launching swaps in the real match, pausing freezes
//! the authoritative state exactly, restart reproduces the launch, and the
//! ambient showcase runs behind menus without ever seeing user input.

use axiom_end_zone::app::TouchInput;
use axiom_end_zone::frontend::input::FrontendInputFrame;
use axiom_end_zone::frontend::persistence::FrontendProfile;
use axiom_end_zone::frontend::state::Screen;
use axiom_end_zone::frontend::SimDirective;
use axiom_end_zone::shell::EndZoneShell;

fn shell() -> EndZoneShell {
    EndZoneShell::new(99, FrontendProfile::default())
}

fn step(shell: &mut EndZoneShell, held: &[&str]) {
    let input = FrontendInputFrame {
        keys_down: held.iter().map(|s| s.to_string()).collect(),
        ..Default::default()
    };
    let _ = shell.frame(&input, TouchInput::default(), 1280.0, 720.0);
}

fn tap(shell: &mut EndZoneShell, token: &str) {
    step(shell, &[token]);
    step(shell, &[]);
}

fn launch_match(shell: &mut EndZoneShell) {
    for _ in 0..5 {
        tap(shell, "Enter");
    }
    for _ in 0..40 {
        step(shell, &[]);
    }
    assert_eq!(shell.frontend.state().screen, Screen::InGame);
}

#[test]
fn menus_run_the_ambient_showcase_and_launch_swaps_in_the_match() {
    let mut sh = shell();
    assert_eq!(sh.frontend.sim_directive(), SimDirective::Menu);
    let tick_a = sh.app.run.sim.tick;
    step(&mut sh, &[]);
    step(&mut sh, &[]);
    assert!(
        sh.app.run.sim.tick > tick_a,
        "the menu background sim advances"
    );

    launch_match(&mut sh);
    assert_eq!(sh.frontend.sim_directive(), SimDirective::Live);
    // The launched match is a fresh run seeded from the frozen config.
    let launch = sh.frontend.state().launch.expect("launch stored");
    assert_eq!(launch.seed, sh.app.run.sim.seed);
}

#[test]
fn pausing_freezes_the_authoritative_state_exactly() {
    let mut sh = shell();
    launch_match(&mut sh);
    for _ in 0..30 {
        step(&mut sh, &[]);
    }
    tap(&mut sh, "KeyP");
    assert_eq!(sh.frontend.state().screen, Screen::Paused);
    assert!(sh.paused());
    let frozen = sh.app.run.sim.digest();
    for _ in 0..45 {
        step(&mut sh, &[]);
    }
    assert_eq!(
        sh.app.run.sim.digest(),
        frozen,
        "no sim advance while paused"
    );

    tap(&mut sh, "KeyP"); // resume
    assert!(!sh.paused());
    for _ in 0..5 {
        step(&mut sh, &[]);
    }
    assert_ne!(
        sh.app.run.sim.digest(),
        frozen,
        "resume continues the match"
    );
}

#[test]
fn pause_settings_keeps_the_match_frozen_behind_the_editor() {
    let mut sh = shell();
    launch_match(&mut sh);
    tap(&mut sh, "KeyP");
    tap(&mut sh, "ArrowDown");
    tap(&mut sh, "ArrowDown");
    tap(&mut sh, "Enter"); // SETTINGS from pause
    assert_eq!(sh.frontend.state().screen, Screen::Settings);
    assert_eq!(sh.frontend.sim_directive(), SimDirective::Frozen);
    let frozen = sh.app.run.sim.digest();
    for _ in 0..30 {
        step(&mut sh, &[]);
    }
    assert_eq!(sh.app.run.sim.digest(), frozen);
}

#[test]
fn restart_match_reboots_from_the_same_frozen_config() {
    let mut sh = shell();
    launch_match(&mut sh);
    let launch = sh.frontend.state().launch.expect("launch stored");
    // Reference: the same frozen config replayed headless with no input.
    let mut reference = axiom_end_zone::showcase::ShowcaseRun::new_match(&launch);
    for _ in 0..50 {
        let _ = reference.step(&[]);
    }
    for _ in 0..60 {
        step(&mut sh, &[]);
    }
    tap(&mut sh, "KeyP");
    tap(&mut sh, "ArrowDown"); // RESTART MATCH
    tap(&mut sh, "Enter");
    assert_eq!(sh.frontend.state().screen, Screen::InGame);
    while sh.app.run.sim.tick < 50 {
        step(&mut sh, &[]);
    }
    assert_eq!(
        sh.app.run.sim.digest(),
        reference.sim.digest(),
        "restart replays the launch byte-for-byte"
    );
}

#[test]
fn return_to_menu_restores_the_ambient_showcase() {
    let mut sh = shell();
    launch_match(&mut sh);
    tap(&mut sh, "KeyP");
    for _ in 0..3 {
        tap(&mut sh, "ArrowDown");
    }
    tap(&mut sh, "Enter"); // modal
    tap(&mut sh, "ArrowRight");
    tap(&mut sh, "Enter"); // confirm RETURN TO MENU
    for _ in 0..32 {
        step(&mut sh, &[]);
    }
    assert_eq!(sh.frontend.state().screen, Screen::MainMenu);
    assert_eq!(sh.frontend.sim_directive(), SimDirective::Menu);
    assert!(sh.frontend.state().launch.is_none());
}

#[test]
fn attract_mode_runs_the_real_showcase_not_a_recording() {
    let mut sh = shell();
    // Idle on the title until attract fires.
    for _ in 0..1900 {
        step(&mut sh, &[]);
    }
    assert_eq!(sh.frontend.state().screen, Screen::Attract);
    assert_eq!(sh.frontend.sim_directive(), SimDirective::Menu);
    let tick_a = sh.app.run.sim.tick;
    for _ in 0..10 {
        step(&mut sh, &[]);
    }
    assert!(
        sh.app.run.sim.tick >= tick_a + 10,
        "the live sim keeps stepping"
    );
}

#[test]
fn user_input_never_reaches_the_menu_background_sim() {
    // Two shells: one idles, one mashes movement keys in the menus. The
    // ambient showcase state must match tick-for-tick.
    let mut idle = shell();
    let mut mashing = shell();
    for i in 0..120 {
        step(&mut idle, &[]);
        // Movement keys only (no Enter — that would change screens).
        let held: &[&str] = if i % 2 == 0 {
            &["KeyW", "KeyA"]
        } else {
            &["KeyD"]
        };
        step(&mut mashing, held);
    }
    assert_eq!(idle.app.run.sim.digest(), mashing.app.run.sim.digest());
}
