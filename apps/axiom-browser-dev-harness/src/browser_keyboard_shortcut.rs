//! The Backquote keyboard contract, classified as pure data.
//!
//! The DOM keydown handler lifts the browser `KeyboardEvent` into a [`KeyChord`]
//! (physical [`code`], modifier flags, and what currently owns focus) and calls
//! [`classify`]. Keeping the decision here — out of the event handler — makes the
//! whole contract testable without a browser, which is where the keyboard rules
//! actually live.
//!
//! Contract:
//! * Match on physical **`code == "Backquote"`**, never `key` (so it works on
//!   every layout regardless of what character the key produces).
//! * Backquote alone → toggle, Shift → cycle density, Ctrl → toggle pin,
//!   Alt → focus console.
//! * Never steal Backquote while a normal `input` / `textarea` /
//!   `contenteditable` element is focused — *except* when the overlay's own
//!   console input owns focus (then `` ` `` still controls the overlay).
//! * A held platform meta key (Cmd/Win) is left to the OS/browser.
//!
//! [`code`]: https://developer.mozilla.org/en-US/docs/Web/API/KeyboardEvent/code

/// What a handled Backquote chord asks the overlay to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayShortcut {
    /// Backquote with no modifiers.
    ToggleOverlay,
    /// Shift + Backquote.
    CycleDensity,
    /// Ctrl + Backquote.
    TogglePinned,
    /// Alt + Backquote.
    FocusConsole,
}

/// The keyboard situation, lifted out of the DOM `KeyboardEvent` so the rule is
/// testable. All-false is "some other key, nothing focused".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct KeyChord {
    /// `KeyboardEvent.code == "Backquote"`.
    pub code_is_backquote: bool,
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    /// A platform meta key (Cmd on macOS, Win on Windows) is held.
    pub meta: bool,
    /// Focus is in a normal text-entry element (`input` / `textarea` /
    /// `contenteditable`).
    pub target_is_text_entry: bool,
    /// Focus is specifically the overlay's own console input.
    pub console_owns_focus: bool,
}

/// Classify a key chord into an overlay shortcut, or `None` if the overlay
/// should not handle it (and therefore must not `preventDefault`).
pub fn classify(chord: KeyChord) -> Option<OverlayShortcut> {
    // Only the physical Backquote key participates.
    if !chord.code_is_backquote {
        return None;
    }
    // Leave Cmd/Win + Backquote to the OS (e.g. macOS window cycling).
    if chord.meta {
        return None;
    }
    // Don't hijack Backquote from someone typing in a normal field — unless that
    // field is our own console, which is allowed to keep driving the overlay.
    if chord.target_is_text_entry && !chord.console_owns_focus {
        return None;
    }
    // Modifier precedence is fixed and deterministic: ctrl, then alt, then shift,
    // else the bare toggle. (The contract only specifies single-modifier chords.)
    Some(if chord.ctrl {
        OverlayShortcut::TogglePinned
    } else if chord.alt {
        OverlayShortcut::FocusConsole
    } else if chord.shift {
        OverlayShortcut::CycleDensity
    } else {
        OverlayShortcut::ToggleOverlay
    })
}

/// A console-input action, classified from `KeyboardEvent.key` while the console
/// owns focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsoleKey {
    /// Enter — submit the current line.
    Submit,
    /// Escape — blur the console but keep the overlay open.
    Dismiss,
    /// ArrowUp — recall an older history entry.
    HistoryPrev,
    /// ArrowDown — recall a newer history entry.
    HistoryNext,
}

/// Classify a console keypress (matched on semantic `key`, since these are
/// layout-independent named keys). Returns `None` for ordinary typing.
pub fn classify_console_key(key: &str) -> Option<ConsoleKey> {
    match key {
        "Enter" => Some(ConsoleKey::Submit),
        "Escape" => Some(ConsoleKey::Dismiss),
        "ArrowUp" => Some(ConsoleKey::HistoryPrev),
        "ArrowDown" => Some(ConsoleKey::HistoryNext),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A Backquote chord with no modifiers and nothing focused.
    fn backquote() -> KeyChord {
        KeyChord {
            code_is_backquote: true,
            ..KeyChord::default()
        }
    }

    #[test]
    fn bare_backquote_toggles_the_overlay() {
        assert_eq!(classify(backquote()), Some(OverlayShortcut::ToggleOverlay));
    }

    #[test]
    fn shift_backquote_cycles_density() {
        let c = KeyChord { shift: true, ..backquote() };
        assert_eq!(classify(c), Some(OverlayShortcut::CycleDensity));
    }

    #[test]
    fn ctrl_backquote_toggles_pinned() {
        let c = KeyChord { ctrl: true, ..backquote() };
        assert_eq!(classify(c), Some(OverlayShortcut::TogglePinned));
    }

    #[test]
    fn alt_backquote_focuses_console() {
        let c = KeyChord { alt: true, ..backquote() };
        assert_eq!(classify(c), Some(OverlayShortcut::FocusConsole));
    }

    #[test]
    fn non_backquote_keys_are_ignored() {
        // e.g. an Escape or a letter: code_is_backquote is false.
        assert_eq!(classify(KeyChord::default()), None);
        let c = KeyChord { shift: true, ..KeyChord::default() };
        assert_eq!(classify(c), None);
    }

    #[test]
    fn backquote_is_ignored_while_a_normal_field_is_focused() {
        let c = KeyChord {
            target_is_text_entry: true,
            console_owns_focus: false,
            ..backquote()
        };
        assert_eq!(classify(c), None);
    }

    #[test]
    fn backquote_is_handled_when_the_console_owns_focus() {
        // The one exception: our own console input keeps driving the overlay.
        let c = KeyChord {
            target_is_text_entry: true,
            console_owns_focus: true,
            ..backquote()
        };
        assert_eq!(classify(c), Some(OverlayShortcut::ToggleOverlay));
    }

    #[test]
    fn meta_backquote_is_left_to_the_os() {
        let c = KeyChord { meta: true, ..backquote() };
        assert_eq!(classify(c), None);
    }

    #[test]
    fn modifier_precedence_is_ctrl_then_alt_then_shift() {
        // Ctrl wins over a simultaneously-held alt/shift.
        let c = KeyChord { ctrl: true, alt: true, shift: true, ..backquote() };
        assert_eq!(classify(c), Some(OverlayShortcut::TogglePinned));
        // Alt wins over shift.
        let c = KeyChord { alt: true, shift: true, ..backquote() };
        assert_eq!(classify(c), Some(OverlayShortcut::FocusConsole));
    }

    #[test]
    fn console_keys_map_enter_escape_and_arrows() {
        assert_eq!(classify_console_key("Enter"), Some(ConsoleKey::Submit));
        assert_eq!(classify_console_key("Escape"), Some(ConsoleKey::Dismiss));
        assert_eq!(classify_console_key("ArrowUp"), Some(ConsoleKey::HistoryPrev));
        assert_eq!(classify_console_key("ArrowDown"), Some(ConsoleKey::HistoryNext));
        // Ordinary typing is not a console action.
        assert_eq!(classify_console_key("a"), None);
        assert_eq!(classify_console_key("Backquote"), None);
    }
}
