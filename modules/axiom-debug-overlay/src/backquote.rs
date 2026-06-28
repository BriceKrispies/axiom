//! The overlay's **Backquote keymap** — the debug-specific mapping of `` ` ``
//! chords to overlay actions, expressed as data over the interface layer's
//! [`Keymap`] primitive (matched on the physical `KeyboardEvent.code == "Backquote"`).
//!
//! The neutral "should this chord route as a global hotkey?" rule
//! ([`axiom_interface::InterfaceInputEvent::routes_global_hotkey`]) and the
//! key→action lookup ([`Keymap::resolve`]) both live in `axiom-interface`; this
//! file only declares the overlay's specific bindings: plain → toggle,
//! Shift → cycle density, Ctrl → pin, Alt → focus console. Each resolves to the
//! matching [`OverlayShortcut`] discriminant, which doubles as the keymap action
//! id and the op-table index in
//! [`crate::overlay_state::OverlayState::apply_shortcut`].

use axiom_interface::{KeyBinding, Keymap};

/// An overlay action selected by a Backquote chord. Discriminants are explicit so
/// the value is both the keymap action id and the index of the state op the
/// overlay runs for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OverlayShortcut {
    ToggleOverlay = 0,
    CycleDensity = 1,
    TogglePinned = 2,
    FocusConsole = 3,
}

/// The overlay's Backquote bindings as a [`Keymap`]: each exact modifier chord on
/// the physical `Backquote` key selects one [`OverlayShortcut`] (by discriminant).
/// Priority is gone — a multi-modifier combo simply matches no chord.
pub(crate) fn backquote_keymap() -> Keymap {
    Keymap::new(&[
        KeyBinding::chord(
            "Backquote",
            false,
            false,
            false,
            OverlayShortcut::ToggleOverlay as u32,
        ),
        KeyBinding::chord(
            "Backquote",
            true,
            false,
            false,
            OverlayShortcut::CycleDensity as u32,
        ),
        KeyBinding::chord(
            "Backquote",
            false,
            true,
            false,
            OverlayShortcut::TogglePinned as u32,
        ),
        KeyBinding::chord(
            "Backquote",
            false,
            false,
            true,
            OverlayShortcut::FocusConsole as u32,
        ),
    ])
}
