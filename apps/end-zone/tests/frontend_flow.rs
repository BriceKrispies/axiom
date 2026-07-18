//! The six-state screen flow: title straight into gameplay, pause/resume,
//! restart, settings/controls returning to pause, return-to-title, game over,
//! and play again — plus replayed-input determinism.

use axiom_end_zone::drive::RunSummary;
use axiom_end_zone::frontend::actions::FrontendCommand;
use axiom_end_zone::frontend::input::FrontendInputFrame;
use axiom_end_zone::frontend::persistence::FrontendProfile;
use axiom_end_zone::frontend::state::Screen;
use axiom_end_zone::frontend::{FrontendApp, FrontendFrame};

fn app() -> FrontendApp {
    FrontendApp::new(7, FrontendProfile::default())
}

fn step_with(app: &mut FrontendApp, held: &[&str]) -> FrontendFrame {
    let input = FrontendInputFrame {
        keys_down: held.iter().map(|s| s.to_string()).collect(),
        ..Default::default()
    };
    app.frame(&input, 1280.0, 720.0)
}

/// Press and release one key (two frames), returning the press frame.
fn tap(app: &mut FrontendApp, token: &str) -> FrontendFrame {
    let frame = step_with(app, &[token]);
    step_with(app, &[]);
    frame
}

fn has(frame: &FrontendFrame, want: FrontendCommand) -> bool {
    frame.commands.iter().any(|c| *c == want)
}

fn to_ingame(fe: &mut FrontendApp) -> FrontendFrame {
    tap(fe, "Enter")
}

/// Enter game over the way the shell does, then let one frame rebuild focus.
fn to_game_over(fe: &mut FrontendApp) {
    fe.enter_game_over(summary());
    step_with(fe, &[]);
}

fn summary() -> RunSummary {
    RunSummary {
        score: 12500,
        touchdowns: 2,
        first_downs: 5,
        longest_play: 38,
    }
}

#[test]
fn title_confirm_starts_gameplay_immediately() {
    let mut fe = app();
    assert_eq!(fe.state().screen, Screen::Title);
    let frame = to_ingame(&mut fe);
    assert_eq!(fe.state().screen, Screen::InGame);
    assert!(frame
        .commands
        .iter()
        .any(|c| matches!(c, FrontendCommand::LaunchRun { .. })));
    assert!(has(&frame, FrontendCommand::SetPaused(false)));
}

#[test]
fn there_is_no_main_menu_between_title_and_game() {
    // A single confirm goes title -> gameplay with no intermediate screen.
    let mut fe = app();
    to_ingame(&mut fe);
    assert_eq!(fe.state().screen, Screen::InGame);
}

#[test]
fn pause_enters_the_pause_menu_and_resume_preserves_the_run() {
    let mut fe = app();
    to_ingame(&mut fe);
    let frame = tap(&mut fe, "KeyP");
    assert_eq!(fe.state().screen, Screen::Paused);
    assert!(has(&frame, FrontendCommand::SetPaused(true)));

    let frame = tap(&mut fe, "Escape");
    assert_eq!(fe.state().screen, Screen::InGame);
    assert!(has(&frame, FrontendCommand::SetPaused(false)));
    // Resume never re-launches — the same run continues.
    assert!(!frame
        .commands
        .iter()
        .any(|c| matches!(c, FrontendCommand::LaunchRun { .. })));
}

#[test]
fn restart_run_launches_a_fresh_run() {
    let mut fe = app();
    to_ingame(&mut fe);
    tap(&mut fe, "KeyP");
    tap(&mut fe, "ArrowDown"); // RESTART RUN
    let frame = tap(&mut fe, "Enter");
    assert_eq!(fe.state().screen, Screen::InGame);
    assert!(has(&frame, FrontendCommand::RestartRun));
}

#[test]
fn settings_and_controls_return_to_pause() {
    let mut fe = app();
    to_ingame(&mut fe);
    tap(&mut fe, "KeyP");
    tap(&mut fe, "ArrowDown");
    tap(&mut fe, "ArrowDown"); // RESUME -> RESTART -> SETTINGS
    tap(&mut fe, "Enter");
    assert_eq!(fe.state().screen, Screen::Settings);
    tap(&mut fe, "Escape");
    assert_eq!(fe.state().screen, Screen::Paused);

    // Focus is restored to SETTINGS on return; one step down reaches CONTROLS.
    tap(&mut fe, "ArrowDown"); // SETTINGS -> CONTROLS
    tap(&mut fe, "Enter");
    assert_eq!(fe.state().screen, Screen::Controls);
    tap(&mut fe, "Escape");
    assert_eq!(fe.state().screen, Screen::Paused);
}

#[test]
fn return_to_title_disposes_the_run() {
    let mut fe = app();
    to_ingame(&mut fe);
    tap(&mut fe, "KeyP");
    for _ in 0..4 {
        tap(&mut fe, "ArrowDown"); // RETURN TO TITLE
    }
    let frame = tap(&mut fe, "Enter");
    assert_eq!(fe.state().screen, Screen::Title);
    assert!(has(&frame, FrontendCommand::ReturnToTitle));
}

#[test]
fn game_over_offers_play_again_and_return_to_title() {
    let mut fe = app();
    to_ingame(&mut fe);
    to_game_over(&mut fe);
    assert_eq!(fe.state().screen, Screen::GameOver);
    assert_eq!(fe.state().summary, Some(summary()));

    // PLAY AGAIN (first item) launches a fresh run.
    let frame = tap(&mut fe, "Enter");
    assert_eq!(fe.state().screen, Screen::InGame);
    assert!(frame
        .commands
        .iter()
        .any(|c| matches!(c, FrontendCommand::LaunchRun { .. })));
    assert_eq!(fe.state().summary, None);
}

#[test]
fn game_over_can_return_to_title() {
    let mut fe = app();
    to_ingame(&mut fe);
    to_game_over(&mut fe);
    tap(&mut fe, "ArrowDown"); // RETURN TO TITLE
    let frame = tap(&mut fe, "Enter");
    assert_eq!(fe.state().screen, Screen::Title);
    assert!(has(&frame, FrontendCommand::ReturnToTitle));
}

#[test]
fn play_again_uses_a_fresh_seed_each_time() {
    let mut fe = app();
    let seed_of = |frame: &FrontendFrame| -> Option<u64> {
        frame.commands.iter().find_map(|c| match c {
            FrontendCommand::LaunchRun { seed } => Some(*seed),
            _ => None,
        })
    };
    let first = seed_of(&to_ingame(&mut fe)).expect("first launch seed");
    to_game_over(&mut fe);
    let second = seed_of(&tap(&mut fe, "Enter")).expect("play-again seed");
    assert_ne!(first, second, "each run uses a fresh explicit seed");
}

#[test]
fn identical_input_scripts_replay_identically() {
    let run = |seed: u64| -> (Vec<(u64, Screen)>, String) {
        let mut fe = FrontendApp::new(seed, FrontendProfile::default());
        let mut last = String::new();
        for token in ["Enter", "KeyP", "ArrowDown", "ArrowDown", "Enter", "Escape"] {
            last = format!("{:?}", tap(&mut fe, token).scene);
        }
        (fe.state().history.clone(), last)
    };
    let (history_a, scene_a) = run(42);
    let (history_b, scene_b) = run(42);
    assert_eq!(history_a, history_b);
    assert_eq!(scene_a, scene_b);
}
