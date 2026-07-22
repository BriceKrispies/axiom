//! The pre-snap huddle screen: the shell opens it, it draws the focused play's
//! chalkboard diagram and one button per play, focusing a play switches the
//! diagram, and confirming one calls that play and returns to the field.

use axiom_end_zone::data::offensive_playbook;
use axiom_end_zone::drive::HuddleView;
use axiom_end_zone::frontend::actions::FrontendCommand;
use axiom_end_zone::frontend::input::FrontendInputFrame;
use axiom_end_zone::frontend::persistence::FrontendProfile;
use axiom_end_zone::frontend::state::Screen;
use axiom_end_zone::frontend::widgets::{Label, Widget};
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

fn tap(app: &mut FrontendApp, token: &str) -> FrontendFrame {
    let frame = step_with(app, &[token]);
    step_with(app, &[]);
    frame
}

/// Title -> Menu -> PLAY -> InGame, then the shell opens the huddle.
fn to_huddle(fe: &mut FrontendApp) -> FrontendFrame {
    tap(fe, "Enter");
    tap(fe, "Enter");
    fe.enter_huddle(HuddleView {
        down: 2,
        yards_to_go: 6.0,
        goal_to_go: false,
        play_count: offensive_playbook().len(),
        default_index: 0,
    });
    step_with(fe, &[]) // rebuild focus + scene for the huddle
}

/// The names of every play card drawn this frame, in layout order.
fn diagram_names(frame: &FrontendFrame) -> Vec<String> {
    frame
        .scene
        .widgets
        .iter()
        .filter_map(|w| match &w.widget {
            Widget::Diagram(view) => Some(view.name.clone()),
            _ => None,
        })
        .collect()
}

/// The name on the currently-focused play card.
fn focused_card(frame: &FrontendFrame) -> Option<String> {
    frame.scene.widgets.iter().find_map(|w| match &w.widget {
        Widget::Diagram(view) if w.focused => Some(view.name.clone()),
        _ => None,
    })
}

fn has_label(frame: &FrontendFrame, text: &str) -> bool {
    frame.scene.widgets.iter().any(|w| {
        matches!(&w.widget, Widget::Label(Label { text: t, .. }) if t == text)
    })
}

#[test]
fn the_shell_opens_the_huddle_from_the_field() {
    let mut fe = app();
    let frame = to_huddle(&mut fe);
    assert_eq!(fe.state().screen, Screen::Huddle);
    assert!(has_label(&frame, "2ND & 6"), "shows the down and distance");
}

#[test]
fn every_play_is_drawn_as_its_own_card_at_once() {
    let mut fe = app();
    let frame = to_huddle(&mut fe);
    let names = diagram_names(&frame);
    let book = offensive_playbook();
    assert_eq!(names.len(), book.len(), "one card per play, all on screen");
    for play in &book {
        assert!(names.iter().any(|n| n == play.name), "a card for {}", play.name);
    }
}

#[test]
fn focus_lands_on_a_card_and_moves_between_them() {
    let mut fe = app();
    let frame = to_huddle(&mut fe);
    let book = offensive_playbook();
    assert_eq!(
        focused_card(&frame).as_deref(),
        Some(book[0].name),
        "the first card is focused by default"
    );
    let frame = tap(&mut fe, "ArrowRight");
    assert_eq!(
        focused_card(&frame).as_deref(),
        Some(book[1].name),
        "moving right focuses the next card"
    );
}

#[test]
fn clicking_a_card_calls_that_play_and_returns_to_the_field() {
    let mut fe = app();
    to_huddle(&mut fe);
    let frame = tap(&mut fe, "ArrowRight"); // focus play index 1
    assert_eq!(focused_card(&frame).as_deref(), Some(offensive_playbook()[1].name));
    let confirm = tap(&mut fe, "Enter");
    assert!(
        confirm
            .commands
            .iter()
            .any(|c| *c == FrontendCommand::CallPlay { index: 1 }),
        "confirming the focused card emits CallPlay for that play"
    );
    assert_eq!(fe.state().screen, Screen::InGame, "returns to the field");
}
