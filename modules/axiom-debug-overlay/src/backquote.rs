//! The overlay's **Backquote binding** — the debug-specific mapping of a
//! `` ` `` chord to an overlay action.
//!
//! The neutral question "should this chord route as a global hotkey at all?"
//! belongs to `axiom-interface` ([`InterfaceInputEvent::routes_global_hotkey`]).
//! *Which* action each modifier selects is a debug-overlay policy, so it stays
//! here: plain → toggle, Shift → cycle density, Ctrl → pin, Alt → focus console.
//! Branchless: mutually-exclusive modifier masks sum to a table index.

use axiom_interface::InterfaceInputEvent;

/// An overlay action selected by a Backquote chord. Discriminants are explicit so
/// the value indexes the op table in [`crate::overlay_state::OverlayState::apply_shortcut`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OverlayShortcut {
    ToggleOverlay = 0,
    CycleDensity = 1,
    TogglePinned = 2,
    FocusConsole = 3,
}

/// Classify a Backquote chord. `None` when the chord should be left alone (a meta
/// chord, or a foreign text field without console focus); otherwise the modifier
/// selects the action.
pub(crate) fn classify_backquote(event: InterfaceInputEvent) -> Option<OverlayShortcut> {
    event
        .routes_global_hotkey()
        .then(|| shortcut_for(event.shift, event.ctrl, event.alt))
}

/// Select the action from the modifiers, with priority Shift > Ctrl > Alt > none.
/// The masks are mutually exclusive, so their weighted sum is the table index.
fn shortcut_for(shift: bool, ctrl: bool, alt: bool) -> OverlayShortcut {
    const TABLE: [OverlayShortcut; 4] = [
        OverlayShortcut::ToggleOverlay,
        OverlayShortcut::CycleDensity,
        OverlayShortcut::TogglePinned,
        OverlayShortcut::FocusConsole,
    ];
    let index =
        (shift as usize) + ((!shift & ctrl) as usize) * 2 + ((!shift & !ctrl & alt) as usize) * 3;
    TABLE[index]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event() -> InterfaceInputEvent {
        InterfaceInputEvent::default()
    }

    #[test]
    fn each_modifier_selects_its_action() {
        assert_eq!(
            classify_backquote(event()),
            Some(OverlayShortcut::ToggleOverlay)
        );
        assert_eq!(
            classify_backquote(InterfaceInputEvent {
                shift: true,
                ..event()
            }),
            Some(OverlayShortcut::CycleDensity)
        );
        assert_eq!(
            classify_backquote(InterfaceInputEvent {
                ctrl: true,
                ..event()
            }),
            Some(OverlayShortcut::TogglePinned)
        );
        assert_eq!(
            classify_backquote(InterfaceInputEvent {
                alt: true,
                ..event()
            }),
            Some(OverlayShortcut::FocusConsole)
        );
    }

    #[test]
    fn modifier_priority_is_shift_then_ctrl_then_alt() {
        assert_eq!(
            classify_backquote(InterfaceInputEvent {
                shift: true,
                ctrl: true,
                ..event()
            }),
            Some(OverlayShortcut::CycleDensity)
        );
        assert_eq!(
            classify_backquote(InterfaceInputEvent {
                ctrl: true,
                alt: true,
                ..event()
            }),
            Some(OverlayShortcut::TogglePinned)
        );
    }

    #[test]
    fn non_routing_chords_select_nothing() {
        assert_eq!(
            classify_backquote(InterfaceInputEvent {
                meta: true,
                ..event()
            }),
            None
        );
        assert_eq!(
            classify_backquote(InterfaceInputEvent {
                in_text_field: true,
                ..event()
            }),
            None
        );
        // A foreign text field with the console focused still routes.
        assert_eq!(
            classify_backquote(InterfaceInputEvent {
                in_text_field: true,
                console_focus: true,
                ..event()
            }),
            Some(OverlayShortcut::ToggleOverlay)
        );
    }
}
