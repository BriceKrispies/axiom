//! Focus + input translation: deterministic initial focus, grid movement,
//! hover focus, pointer activation, repeat delay/cadence, modal confinement,
//! and stable device hints.

use axiom_end_zone::frontend::actions::{DeviceAction, FrontendAction, InputDevice, NavDirection};
use axiom_end_zone::frontend::bindings::ControlBindings;
use axiom_end_zone::frontend::input::{
    FrontendInputFrame, InputTranslator, REPEAT_CADENCE, REPEAT_DELAY,
};
use axiom_end_zone::frontend::layout::rect;
use axiom_end_zone::frontend::navigation::{FocusEntry, FocusList, MoveOutcome, WidgetId};
use axiom_end_zone::frontend::persistence::FrontendProfile;
use axiom_end_zone::frontend::state::Screen;
use axiom_end_zone::frontend::FrontendApp;

fn frame_of(held: &[&str]) -> FrontendInputFrame {
    FrontendInputFrame {
        keys_down: held.iter().map(|s| s.to_string()).collect(),
        ..Default::default()
    }
}

fn grid() -> FocusList {
    // 2×2 grid: ids 1 2 / 3 4.
    FocusList::new(
        vec![
            FocusEntry::new(WidgetId(1), rect(0.0, 0.0, 10.0, 10.0), 0, 0),
            FocusEntry::new(WidgetId(2), rect(20.0, 0.0, 10.0, 10.0), 0, 1),
            FocusEntry::new(WidgetId(3), rect(0.0, 20.0, 10.0, 10.0), 1, 0),
            FocusEntry::new(WidgetId(4), rect(20.0, 20.0, 10.0, 10.0), 1, 1),
        ],
        None,
    )
}

#[test]
fn first_enabled_entry_is_focused_by_default() {
    assert_eq!(grid().focused(), Some(WidgetId(1)));
}

#[test]
fn directional_movement_walks_the_grid_deterministically() {
    let mut list = grid();
    assert_eq!(list.step(1, 0), MoveOutcome::Moved);
    assert_eq!(list.focused(), Some(WidgetId(2)));
    assert_eq!(list.step(0, 1), MoveOutcome::Moved);
    assert_eq!(list.focused(), Some(WidgetId(4)));
    assert_eq!(list.step(-1, 0), MoveOutcome::Moved);
    assert_eq!(list.focused(), Some(WidgetId(3)));
    assert_eq!(list.step(0, -1), MoveOutcome::Moved);
    assert_eq!(list.focused(), Some(WidgetId(1)));
    // Edges do not wrap.
    assert_eq!(list.step(-1, 0), MoveOutcome::Edge);
    assert_eq!(list.focused(), Some(WidgetId(1)));
}

#[test]
fn disabled_entries_are_skipped() {
    let mut list = FocusList::new(
        vec![
            FocusEntry::new(WidgetId(1), rect(0.0, 0.0, 10.0, 10.0), 0, 0),
            FocusEntry::new(WidgetId(2), rect(0.0, 20.0, 10.0, 10.0), 1, 0).disabled(),
            FocusEntry::new(WidgetId(3), rect(0.0, 40.0, 10.0, 10.0), 2, 0),
        ],
        None,
    );
    assert_eq!(list.step(0, 1), MoveOutcome::Moved);
    assert_eq!(list.focused(), Some(WidgetId(3)));
}

#[test]
fn remembered_focus_is_restored_when_valid() {
    let entries = grid().entries().to_vec();
    let list = FocusList::new(entries.clone(), Some(WidgetId(3)));
    assert_eq!(list.focused(), Some(WidgetId(3)));
    // An unknown remembered id falls back to the first enabled entry.
    let list = FocusList::new(entries, Some(WidgetId(99)));
    assert_eq!(list.focused(), Some(WidgetId(1)));
}

#[test]
fn hover_focuses_and_activation_hits_the_hovered_widget() {
    let mut list = grid();
    assert_eq!(list.hover(25.0, 25.0), Some(WidgetId(4)));
    assert_eq!(list.focused(), Some(WidgetId(4)));
    assert_eq!(list.activate_at(5.0, 5.0), Some(WidgetId(1)));
    assert_eq!(list.hover(15.0, 15.0), None);
}

#[test]
fn navigation_repeat_has_a_delay_then_a_cadence() {
    let mut translator = InputTranslator::new();
    let bindings = ControlBindings::default();
    let held = frame_of(&["ArrowDown"]);
    let mut fired = Vec::new();
    for tick in 0..(REPEAT_DELAY + REPEAT_CADENCE * 3 + 1) {
        let actions = translator.tick(&held, &bindings);
        if actions
            .iter()
            .any(|a| a.action == FrontendAction::Navigate(NavDirection::Down))
        {
            fired.push(tick);
        }
    }
    assert_eq!(fired[0], 0, "immediate first fire");
    assert_eq!(fired[1], REPEAT_DELAY, "repeat starts after the delay");
    assert_eq!(fired[2], REPEAT_DELAY + REPEAT_CADENCE, "then the cadence");
}

#[test]
fn confirm_is_edge_triggered_not_level_triggered() {
    let mut translator = InputTranslator::new();
    let bindings = ControlBindings::default();
    let held = frame_of(&["Enter"]);
    let first = translator.tick(&held, &bindings);
    assert!(first.iter().any(|a| a.action == FrontendAction::Confirm));
    let second = translator.tick(&held, &bindings);
    assert!(!second.iter().any(|a| a.action == FrontendAction::Confirm));
}

#[test]
fn device_hint_is_stable_against_single_pointer_events() {
    let mut translator = InputTranslator::new();
    let bindings = ControlBindings::default();
    assert_eq!(translator.hint_device(), InputDevice::Keyboard);
    // One stray pointer move does NOT flip the hint...
    let moved = FrontendInputFrame {
        pointer: Some((10.0, 10.0)),
        ..Default::default()
    };
    let _ = translator.tick(&moved, &bindings);
    assert_eq!(translator.hint_device(), InputDevice::Keyboard);
    // ...but a press does.
    let pressed = FrontendInputFrame {
        pointer: Some((10.0, 10.0)),
        pointer_pressed: true,
        ..Default::default()
    };
    let _ = translator.tick(&pressed, &bindings);
    assert_eq!(translator.hint_device(), InputDevice::Pointer);
    // And a key press flips it straight back.
    let _ = translator.tick(&frame_of(&["ArrowDown"]), &bindings);
    assert_eq!(translator.hint_device(), InputDevice::Keyboard);
}

#[test]
fn gamepad_tokens_translate_like_keys() {
    let mut translator = InputTranslator::new();
    let bindings = ControlBindings::default();
    let pad = FrontendInputFrame {
        pad_down: vec!["PadA".to_string(), "PadDown".to_string()],
        ..Default::default()
    };
    let actions = translator.tick(&pad, &bindings);
    assert!(actions.contains(&DeviceAction::new(
        FrontendAction::Confirm,
        InputDevice::Gamepad
    )));
    assert!(actions.contains(&DeviceAction::new(
        FrontendAction::Navigate(NavDirection::Down),
        InputDevice::Gamepad
    )));
    assert_eq!(translator.hint_device(), InputDevice::Gamepad);
}

#[test]
fn a_modal_confines_focus_to_its_options() {
    let mut fe = FrontendApp::new(3, FrontendProfile::default());
    let step = |fe: &mut FrontendApp, held: &[&str]| {
        let input = frame_of(held);
        fe.frame(&input, 1280.0, 720.0)
    };
    let tap = |fe: &mut FrontendApp, token: &str| {
        step(fe, &[token]);
        step(fe, &[]);
    };
    // Reach the pause menu and open the return-to-menu dialog.
    for _ in 0..5 {
        tap(&mut fe, "Enter");
    }
    for _ in 0..40 {
        step(&mut fe, &[]);
    }
    tap(&mut fe, "KeyP");
    for _ in 0..3 {
        tap(&mut fe, "ArrowDown");
    }
    tap(&mut fe, "Enter");
    assert!(fe.state().modal.is_some());
    step(&mut fe, &[]);
    // Only the two dialog options are focusable now.
    let ids: Vec<u32> = fe.state().focus.entries().iter().map(|e| e.id.0).collect();
    assert_eq!(ids, vec![9000, 9001]);
    assert_eq!(fe.state().screen, Screen::Paused);
}
