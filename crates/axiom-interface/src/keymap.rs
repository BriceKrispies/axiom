//! [`Keymap`] — the layer's reusable keybinding primitive.
//!
//! A consumer declares a set of [`KeyBinding`]s (each a key token, a modifier
//! condition, and the `u32` action it selects) and resolves a pressed key against
//! them. The *key token* is whatever the consumer matched — a physical
//! `KeyboardEvent.code` (`"Backquote"`) for a layout-independent hotkey, or a
//! logical `key` (`"w"`, `"ArrowUp"`) for a game — so the layer stays
//! token-agnostic. The action is a neutral `u32`, the same shape the panel's
//! action buttons use ([`crate::InterfaceDrawItem::Button`]); the consumer maps it
//! back to behaviour.
//!
//! [`Keymap::resolve`] is a pure first-match lookup. It deliberately does **not**
//! apply the global-hotkey routing guard
//! ([`InterfaceInputEvent::routes_global_hotkey`]): the consumer composes that
//! where it wants — typically on key-*down* only, so a key-*up* always resolves
//! and clears held state even while a modifier or meta key is down.

use crate::input_event::InterfaceInputEvent;

/// One key binding: a key token, the modifier condition it requires, and the
/// consumer-defined action id it selects. Build with [`KeyBinding::key`]
/// (modifier-insensitive) or [`KeyBinding::chord`] (exact modifiers).
///
/// The modifier condition is stored as a `(care, required)` pair per modifier so
/// matching stays branchless; the two constructors are the only way to set it.
#[derive(Debug, Clone, Copy)]
pub struct KeyBinding {
    key: &'static str,
    care_shift: bool,
    req_shift: bool,
    care_ctrl: bool,
    req_ctrl: bool,
    care_alt: bool,
    req_alt: bool,
    action: u32,
}

impl KeyBinding {
    /// A modifier-insensitive binding: `key` selects `action` whatever modifiers
    /// are held (the movement-key style — and the form that lets a key-up clear
    /// held state even while a modifier is down).
    pub const fn key(key: &'static str, action: u32) -> Self {
        KeyBinding {
            key,
            care_shift: false,
            req_shift: false,
            care_ctrl: false,
            req_ctrl: false,
            care_alt: false,
            req_alt: false,
            action,
        }
    }

    /// An exact-modifier chord: `key` selects `action` only when shift/ctrl/alt
    /// match exactly (the hotkey style). `meta` is intentionally not part of the
    /// match — leave it to the consumer's routing guard.
    pub const fn chord(key: &'static str, shift: bool, ctrl: bool, alt: bool, action: u32) -> Self {
        KeyBinding {
            key,
            care_shift: true,
            req_shift: shift,
            care_ctrl: true,
            req_ctrl: ctrl,
            care_alt: true,
            req_alt: alt,
            action,
        }
    }

    /// Whether this binding matches a pressed `key` token + chord. Branchless:
    /// key-token equality ANDed with each modifier's `!care | (required == actual)`.
    fn matches(self, key: &str, event: InterfaceInputEvent) -> bool {
        (self.key == key)
            & (!self.care_shift | (self.req_shift == event.shift))
            & (!self.care_ctrl | (self.req_ctrl == event.ctrl))
            & (!self.care_alt | (self.req_alt == event.alt))
    }
}

/// An ordered set of [`KeyBinding`]s, resolved against a pressed key + chord.
#[derive(Debug, Clone, Default)]
pub struct Keymap {
    bindings: Vec<KeyBinding>,
}

impl Keymap {
    /// Build a keymap from a binding list. On [`resolve`](Self::resolve) the first
    /// matching binding (in list order) wins.
    pub fn new(bindings: &[KeyBinding]) -> Self {
        Keymap {
            bindings: bindings.to_vec(),
        }
    }

    /// The bound action id for a pressed `key` token + chord, or `None`. A pure
    /// first-match lookup; the global-hotkey routing guard is the caller's to
    /// apply (see the module docs).
    pub fn resolve(&self, key: &str, event: InterfaceInputEvent) -> Option<u32> {
        self.bindings
            .iter()
            .find(|binding| binding.matches(key, event))
            .map(|binding| binding.action)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(shift: bool, ctrl: bool, alt: bool) -> InterfaceInputEvent {
        InterfaceInputEvent {
            shift,
            ctrl,
            alt,
            meta: false,
            in_text_field: false,
            console_focus: false,
        }
    }

    #[test]
    fn empty_and_default_resolve_to_none() {
        assert_eq!(Keymap::new(&[]).resolve("w", ev(false, false, false)), None);
        assert_eq!(
            Keymap::default().resolve("w", InterfaceInputEvent::default()),
            None
        );
    }

    #[test]
    fn modifier_insensitive_key_matches_any_modifiers_and_wrong_key_misses() {
        let map = Keymap::new(&[KeyBinding::key("w", 7)]);
        assert_eq!(map.resolve("w", ev(false, false, false)), Some(7));
        // The same key resolves regardless of held modifiers (the key-up-safe form).
        assert_eq!(map.resolve("w", ev(true, true, true)), Some(7));
        // A different key misses.
        assert_eq!(map.resolve("a", ev(false, false, false)), None);
    }

    #[test]
    fn exact_chord_matches_only_its_modifier_state() {
        // plain / Shift / Ctrl / Alt on the same key -> distinct actions. Resolving
        // each in turn exercises the non-matching modifier arms of the earlier
        // bindings before the matching one is found.
        let map = Keymap::new(&[
            KeyBinding::chord("Backquote", false, false, false, 0),
            KeyBinding::chord("Backquote", true, false, false, 1),
            KeyBinding::chord("Backquote", false, true, false, 2),
            KeyBinding::chord("Backquote", false, false, true, 3),
        ]);
        assert_eq!(map.resolve("Backquote", ev(false, false, false)), Some(0));
        assert_eq!(map.resolve("Backquote", ev(true, false, false)), Some(1));
        assert_eq!(map.resolve("Backquote", ev(false, true, false)), Some(2));
        assert_eq!(map.resolve("Backquote", ev(false, false, true)), Some(3));
        // A multi-modifier combo matches no exact chord.
        assert_eq!(map.resolve("Backquote", ev(true, true, false)), None);
    }

    #[test]
    fn first_match_wins() {
        let map = Keymap::new(&[KeyBinding::key("w", 1), KeyBinding::key("w", 2)]);
        assert_eq!(map.resolve("w", ev(false, false, false)), Some(1));
    }
}
