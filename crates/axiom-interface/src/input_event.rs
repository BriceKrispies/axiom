//! Neutral keyboard/text input as data.
//!
//! [`InterfaceInputEvent`] is the modifier + focus context lifted out of any
//! browser event, with the generic guard a global hotkey needs (don't fire while
//! a foreign text field is focused; leave a platform meta key to the OS). The
//! *binding* of a specific physical key to a specific action stays with the
//! consumer — this layer only owns the neutral event and the console-key
//! classification.

/// A neutral key chord: modifiers plus what currently owns focus. Carries no
/// physical key — the consumer matches that and pairs it with this context.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct InterfaceInputEvent {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub meta: bool,
    /// Focus is in a foreign text-entry element (input/textarea/contenteditable).
    pub in_text_field: bool,
    /// Focus is the interface's own console input.
    pub console_focus: bool,
}

impl InterfaceInputEvent {
    /// Whether a global hotkey chord should be routed: not a platform meta chord,
    /// and not stolen from a foreign text field unless the console owns focus.
    /// Branchless.
    pub fn routes_global_hotkey(self) -> bool {
        !self.meta & (!self.in_text_field | self.console_focus)
    }
}

/// A semantic console-input action, classified from a key name. Discriminants
/// are explicit so the value indexes the branchless op table in
/// [`crate::InterfaceApi`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConsoleKey {
    Submit = 0,
    Dismiss = 1,
    HistoryPrev = 2,
    HistoryNext = 3,
}

/// Classify a console keypress (matched on the semantic key name). `None` for
/// ordinary typing. Branchless equality chain.
pub(crate) fn classify_console_key(key: &str) -> Option<ConsoleKey> {
    (key == "Enter")
        .then_some(ConsoleKey::Submit)
        .or_else(|| (key == "Escape").then_some(ConsoleKey::Dismiss))
        .or_else(|| (key == "ArrowUp").then_some(ConsoleKey::HistoryPrev))
        .or_else(|| (key == "ArrowDown").then_some(ConsoleKey::HistoryNext))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event() -> InterfaceInputEvent {
        InterfaceInputEvent::default()
    }

    #[test]
    fn bare_chord_routes() {
        assert!(event().routes_global_hotkey());
        assert!(InterfaceInputEvent {
            ctrl: true,
            ..event()
        }
        .routes_global_hotkey());
    }

    #[test]
    fn meta_chord_is_left_to_the_os() {
        assert!(!InterfaceInputEvent {
            meta: true,
            ..event()
        }
        .routes_global_hotkey());
    }

    #[test]
    fn text_field_blocks_unless_the_console_owns_focus() {
        assert!(!InterfaceInputEvent {
            in_text_field: true,
            ..event()
        }
        .routes_global_hotkey());
        assert!(InterfaceInputEvent {
            in_text_field: true,
            console_focus: true,
            ..event()
        }
        .routes_global_hotkey());
    }

    #[test]
    fn console_keys_classify_or_pass_through() {
        assert_eq!(classify_console_key("Enter"), Some(ConsoleKey::Submit));
        assert_eq!(classify_console_key("Escape"), Some(ConsoleKey::Dismiss));
        assert_eq!(
            classify_console_key("ArrowUp"),
            Some(ConsoleKey::HistoryPrev)
        );
        assert_eq!(
            classify_console_key("ArrowDown"),
            Some(ConsoleKey::HistoryNext)
        );
        assert_eq!(classify_console_key("a"), None);
    }
}
