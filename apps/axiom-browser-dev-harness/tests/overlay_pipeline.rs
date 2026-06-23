//! End-to-end pipeline test: drive the overlay the way the DOM controller does —
//! classify keyboard chords into shortcuts, apply them to the pure state, and run
//! console commands through the real registry — all without a browser. This is
//! the proof that the keyboard contract and the command pipeline are real and
//! composable, not just individually unit-tested.

use axiom_browser_dev_harness::browser_keyboard_shortcut::{
    classify, classify_console_key, ConsoleKey, KeyChord, OverlayShortcut,
};
use axiom_browser_dev_harness::debug_command_registry::CommandRegistry;
use axiom_browser_dev_harness::debug_overlay_density::OverlayDensity;
use axiom_browser_dev_harness::debug_overlay_state::OverlayState;

/// Mirror the DOM controller's `apply_shortcut`: a classified chord mutates state.
fn apply(state: &mut OverlayState, shortcut: OverlayShortcut) {
    match shortcut {
        OverlayShortcut::ToggleOverlay => state.toggle(),
        OverlayShortcut::CycleDensity => state.cycle_density(),
        OverlayShortcut::TogglePinned => state.toggle_pin(),
        OverlayShortcut::FocusConsole => state.focus_console(),
    }
}

/// Press the physical Backquote with the given modifiers, as the window handler
/// would, and apply whatever it classifies to.
fn press_backquote(state: &mut OverlayState, shift: bool, ctrl: bool, alt: bool) {
    let chord = KeyChord {
        code_is_backquote: true,
        shift,
        ctrl,
        alt,
        ..KeyChord::default()
    };
    if let Some(shortcut) = classify(chord) {
        apply(state, shortcut);
    }
}

#[test]
fn backquote_session_drives_visibility_density_pin_and_console() {
    let mut state = OverlayState::new();
    assert!(!state.is_visible(), "starts hidden");

    // Backquote opens it.
    press_backquote(&mut state, false, false, false);
    assert!(state.is_visible());

    // Shift+Backquote cycles density normal → verbose.
    press_backquote(&mut state, true, false, false);
    assert_eq!(state.density(), OverlayDensity::Verbose);

    // Ctrl+Backquote pins; a subsequent bare Backquote can't dismiss it.
    press_backquote(&mut state, false, true, false);
    assert!(state.is_pinned());
    press_backquote(&mut state, false, false, false);
    assert!(state.is_visible(), "pin protects against accidental close");

    // Alt+Backquote focuses the console.
    press_backquote(&mut state, false, false, true);
    assert!(state.console().is_focused());
}

#[test]
fn console_session_submits_navigates_history_and_dismisses() {
    let mut state = OverlayState::new();
    let registry = CommandRegistry::standard();
    state.focus_console();

    // Submit two commands (Enter), as the input keydown handler does.
    assert_eq!(classify_console_key("Enter"), Some(ConsoleKey::Submit));
    registry.execute(&mut state, "overlay.compact");
    registry.execute(&mut state, "help");
    assert_eq!(state.density(), OverlayDensity::Compact);
    assert_eq!(state.command_history_count(), 2);

    // ArrowUp recalls newest-first; ArrowDown returns toward the live line.
    assert_eq!(classify_console_key("ArrowUp"), Some(ConsoleKey::HistoryPrev));
    assert_eq!(state.history_prev(), Some("help".to_string()));
    assert_eq!(state.history_prev(), Some("overlay.compact".to_string()));
    assert_eq!(classify_console_key("ArrowDown"), Some(ConsoleKey::HistoryNext));
    assert_eq!(state.history_next(), Some("help".to_string()));
    assert_eq!(state.history_next(), Some(String::new())); // live line

    // Escape blurs the console but leaves the overlay open.
    assert_eq!(classify_console_key("Escape"), Some(ConsoleKey::Dismiss));
    state.blur_console();
    assert!(!state.console().is_focused());
    assert!(state.is_visible());
}

#[test]
fn clear_then_unknown_command_leaves_one_error_line() {
    let mut state = OverlayState::new();
    let registry = CommandRegistry::standard();

    registry.execute(&mut state, "help");
    registry.execute(&mut state, "backend.report");
    assert_eq!(state.recent_results().len(), 2);

    // `clear` empties the visible log…
    registry.execute(&mut state, "clear");
    assert!(state.recent_results().is_empty());

    // …and a following unknown command leaves exactly its own error line.
    registry.execute(&mut state, "nope");
    assert_eq!(state.recent_results().len(), 1);
    assert!(!state.recent_results()[0].ok);
}
