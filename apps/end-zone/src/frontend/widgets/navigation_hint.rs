//! Navigation hints: device-aware action/label pairs shown in the footer.
//! The labels track the stable last-active device so keyboard, gamepad,
//! pointer, and touch users each see their own controls.

use crate::frontend::actions::InputDevice;

/// One hint chip: the action name plus the device-specific control label.
#[derive(Debug, Clone, PartialEq)]
pub struct Hint {
    pub action: String,
    pub control: String,
}

fn hint(action: &str, control: &str) -> Hint {
    Hint {
        action: action.to_string(),
        control: control.to_string(),
    }
}

/// What one screen's footer offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HintSet {
    pub navigate: bool,
    pub adjust: bool,
    pub confirm: Option<&'static str>,
    pub cancel: Option<&'static str>,
    pub pause: Option<&'static str>,
}

impl HintSet {
    pub const fn menu() -> Self {
        HintSet {
            navigate: true,
            adjust: false,
            confirm: Some("SELECT"),
            cancel: Some("BACK"),
            pause: None,
        }
    }
}

/// Build the footer hints for the active device.
pub fn hints_for(device: InputDevice, set: HintSet) -> Vec<Hint> {
    let (nav, adjust, confirm, cancel, pause) = match device {
        InputDevice::Keyboard => ("W/S · ↑/↓", "A/D · ←/→", "ENTER", "ESC", "P"),
        InputDevice::Gamepad => ("D-PAD ↑/↓", "D-PAD ←/→", "PAD A", "PAD B", "START"),
        InputDevice::Pointer => ("HOVER", "CLICK ◀ ▶", "CLICK", "RIGHT-CLICK / ESC", "P"),
        InputDevice::Touch => ("TAP", "TAP ◀ ▶", "TAP", "BACK BUTTON", "PAUSE BUTTON"),
    };
    let mut hints = Vec::new();
    if set.navigate {
        hints.push(hint("MOVE", nav));
    }
    if set.adjust {
        hints.push(hint("ADJUST", adjust));
    }
    if let Some(label) = set.confirm {
        hints.push(hint(label, confirm));
    }
    if let Some(label) = set.cancel {
        hints.push(hint(label, cancel));
    }
    if let Some(label) = set.pause {
        hints.push(hint(label, pause));
    }
    hints
}
