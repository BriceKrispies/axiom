//! Screen state-machine flow: the full menu path into a match, consistent
//! backward cancel, pause/resume, settings origin restoration, attract-mode
//! entry/exit, and replayed-input determinism.

use axiom_end_zone::frontend::actions::FrontendCommand;
use axiom_end_zone::frontend::input::FrontendInputFrame;
use axiom_end_zone::frontend::persistence::FrontendProfile;
use axiom_end_zone::frontend::state::{Screen, ATTRACT_TIMEOUT};
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

fn settle(app: &mut FrontendApp, frames: usize) {
    for _ in 0..frames {
        step_with(app, &[]);
    }
}

#[test]
fn the_happy_path_reaches_a_launched_match_with_enter_alone() {
    let mut fe = app();
    assert_eq!(fe.state().screen, Screen::Title);
    tap(&mut fe, "Enter"); // PRESS START
    assert_eq!(fe.state().screen, Screen::MainMenu);
    tap(&mut fe, "Enter"); // START GAME
    assert_eq!(fe.state().screen, Screen::TeamSelect);
    tap(&mut fe, "Enter"); // lock player team
    assert!(fe.state().team_select.locked_player.is_some());
    assert_eq!(fe.state().screen, Screen::TeamSelect);
    tap(&mut fe, "Enter"); // lock opponent
    assert_eq!(fe.state().screen, Screen::MatchSetup);
    let frame = tap(&mut fe, "Enter"); // START MATCH
    assert_eq!(fe.state().screen, Screen::TransitionToGame);
    assert!(frame
        .commands
        .iter()
        .any(|c| matches!(c, FrontendCommand::LaunchMatch(_))));
    settle(&mut fe, 40);
    assert_eq!(fe.state().screen, Screen::InGame);
}

#[test]
fn cancel_walks_backward_consistently() {
    let mut fe = app();
    tap(&mut fe, "Enter");
    tap(&mut fe, "Enter");
    tap(&mut fe, "Enter"); // player locked, stage 2
    tap(&mut fe, "Escape"); // unlock, back to stage 1
    assert_eq!(fe.state().screen, Screen::TeamSelect);
    assert!(fe.state().team_select.locked_player.is_none());
    tap(&mut fe, "Escape");
    assert_eq!(fe.state().screen, Screen::MainMenu);
    tap(&mut fe, "Escape");
    assert_eq!(fe.state().screen, Screen::Title);
}

#[test]
fn match_setup_cancel_returns_to_team_select() {
    let mut fe = app();
    for _ in 0..4 {
        tap(&mut fe, "Enter");
    }
    assert_eq!(fe.state().screen, Screen::MatchSetup);
    tap(&mut fe, "Escape");
    assert_eq!(fe.state().screen, Screen::TeamSelect);
}

#[test]
fn pause_resume_and_return_to_menu_flow() {
    let mut fe = app();
    for _ in 0..5 {
        tap(&mut fe, "Enter");
    }
    settle(&mut fe, 40);
    assert_eq!(fe.state().screen, Screen::InGame);

    let frame = tap(&mut fe, "KeyP");
    assert_eq!(fe.state().screen, Screen::Paused);
    assert!(frame
        .commands
        .iter()
        .any(|c| matches!(c, FrontendCommand::SetPaused(true))));

    // Cancel resumes.
    let frame = tap(&mut fe, "Escape");
    assert_eq!(fe.state().screen, Screen::InGame);
    assert!(frame
        .commands
        .iter()
        .any(|c| matches!(c, FrontendCommand::SetPaused(false))));

    // Pause again, walk to RETURN TO MAIN MENU, confirm through the modal.
    tap(&mut fe, "KeyP");
    for _ in 0..3 {
        tap(&mut fe, "ArrowDown");
    }
    tap(&mut fe, "Enter"); // opens the modal (safe option focused)
    assert!(fe.state().modal.is_some());
    tap(&mut fe, "ArrowRight"); // move to RETURN TO MENU
    let frame = tap(&mut fe, "Enter");
    assert_eq!(fe.state().screen, Screen::TransitionToMenu);
    assert!(frame
        .commands
        .iter()
        .any(|c| matches!(c, FrontendCommand::ReturnToMenu)));
    settle(&mut fe, 32);
    assert_eq!(fe.state().screen, Screen::MainMenu);
    assert!(fe.state().launch.is_none());
}

#[test]
fn modal_cancel_keeps_the_match() {
    let mut fe = app();
    for _ in 0..5 {
        tap(&mut fe, "Enter");
    }
    settle(&mut fe, 40);
    tap(&mut fe, "KeyP");
    for _ in 0..3 {
        tap(&mut fe, "ArrowDown");
    }
    tap(&mut fe, "Enter");
    assert!(fe.state().modal.is_some());
    tap(&mut fe, "Escape"); // dismiss dialog
    assert!(fe.state().modal.is_none());
    assert_eq!(fe.state().screen, Screen::Paused);
    assert!(fe.state().launch.is_some());
}

#[test]
fn settings_returns_to_its_exact_origin_with_focus_restored() {
    let mut fe = app();
    tap(&mut fe, "Enter"); // MainMenu
    tap(&mut fe, "ArrowDown"); // focus SETTINGS
    let focused_before = fe.state().focus.focused();
    tap(&mut fe, "Enter");
    assert_eq!(fe.state().screen, Screen::Settings);
    tap(&mut fe, "Escape"); // no changes → straight back
    assert_eq!(fe.state().screen, Screen::MainMenu);
    // A frame passes so focus is rebuilt from memory.
    step_with(&mut fe, &[]);
    assert_eq!(fe.state().focus.focused(), focused_before);
}

#[test]
fn settings_from_pause_returns_to_pause() {
    let mut fe = app();
    for _ in 0..5 {
        tap(&mut fe, "Enter");
    }
    settle(&mut fe, 40);
    tap(&mut fe, "KeyP");
    tap(&mut fe, "ArrowDown");
    tap(&mut fe, "ArrowDown"); // SETTINGS
    tap(&mut fe, "Enter");
    assert_eq!(fe.state().screen, Screen::Settings);
    tap(&mut fe, "Escape");
    assert_eq!(fe.state().screen, Screen::Paused);
    // The match is still resident.
    assert!(fe.state().launch.is_some());
}

#[test]
fn credits_opens_and_any_confirm_returns() {
    let mut fe = app();
    tap(&mut fe, "Enter");
    tap(&mut fe, "ArrowDown");
    tap(&mut fe, "ArrowDown"); // CREDITS
    tap(&mut fe, "Enter");
    assert_eq!(fe.state().screen, Screen::Credits);
    tap(&mut fe, "Enter");
    assert_eq!(fe.state().screen, Screen::MainMenu);
}

#[test]
fn attract_mode_enters_on_inactivity_and_exits_on_input() {
    let mut fe = app();
    for _ in 0..=ATTRACT_TIMEOUT as usize {
        step_with(&mut fe, &[]);
    }
    assert_eq!(fe.state().screen, Screen::Attract);
    tap(&mut fe, "Enter");
    assert_eq!(fe.state().screen, Screen::Title);
}

#[test]
fn attract_never_triggers_deep_in_the_menus() {
    let mut fe = app();
    for _ in 0..3 {
        tap(&mut fe, "Enter");
    }
    assert_eq!(fe.state().screen, Screen::TeamSelect);
    for _ in 0..=ATTRACT_TIMEOUT as usize {
        step_with(&mut fe, &[]);
    }
    assert_eq!(fe.state().screen, Screen::TeamSelect);
}

#[test]
fn identical_input_scripts_replay_identically() {
    let run = |seed: u64| -> (Vec<(u64, Screen)>, String) {
        let mut fe = FrontendApp::new(seed, FrontendProfile::default());
        let mut last_scene = String::new();
        let script: [&str; 9] = [
            "Enter",
            "Enter",
            "ArrowRight",
            "Enter",
            "ArrowLeft",
            "Enter",
            "Enter",
            "Escape",
            "Enter",
        ];
        for token in script {
            let frame = tap(&mut fe, token);
            last_scene = format!("{:?}", frame.scene);
        }
        (fe.state().history.clone(), last_scene)
    };
    let (history_a, scene_a) = run(42);
    let (history_b, scene_b) = run(42);
    assert_eq!(history_a, history_b);
    assert_eq!(scene_a, scene_b);
}
