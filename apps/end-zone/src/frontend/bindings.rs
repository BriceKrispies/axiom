//! The fixed control map: neutral token bindings per action. There is no
//! rebinding, conflict detection, or control profile — the bindings are
//! constant, and the Controls screen renders them read-only. Tokens are the
//! app's neutral input vocabulary — `KeyboardEvent.code` strings and `Pad*`
//! gamepad tokens.

/// Every bound action. Menu navigation, confirm/cancel/pause, and the one
/// gameplay action (SNAP / THROW).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindableAction {
    NavUp,
    NavDown,
    NavLeft,
    NavRight,
    Confirm,
    Cancel,
    Pause,
    GamePrimary,
}

impl BindableAction {
    pub const ALL: [BindableAction; 8] = [
        BindableAction::NavUp,
        BindableAction::NavDown,
        BindableAction::NavLeft,
        BindableAction::NavRight,
        BindableAction::Confirm,
        BindableAction::Cancel,
        BindableAction::Pause,
        BindableAction::GamePrimary,
    ];

    pub fn label(self) -> &'static str {
        match self {
            BindableAction::NavUp => "MOVE UP",
            BindableAction::NavDown => "MOVE DOWN",
            BindableAction::NavLeft => "MOVE LEFT",
            BindableAction::NavRight => "MOVE RIGHT",
            BindableAction::Confirm => "CONFIRM",
            BindableAction::Cancel => "CANCEL / BACK",
            BindableAction::Pause => "PAUSE",
            BindableAction::GamePrimary => "SNAP / THROW",
        }
    }

    pub fn index(self) -> usize {
        BindableAction::ALL
            .iter()
            .position(|a| *a == self)
            .unwrap_or(0)
    }
}

/// The fixed control map: an ordered token list per action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlBindings {
    tokens: [&'static [&'static str]; BindableAction::ALL.len()],
}

fn defaults_for(action: BindableAction) -> &'static [&'static str] {
    match action {
        BindableAction::NavUp => &["KeyW", "ArrowUp", "PadUp"],
        BindableAction::NavDown => &["KeyS", "ArrowDown", "PadDown"],
        BindableAction::NavLeft => &["KeyA", "ArrowLeft", "PadLeft"],
        BindableAction::NavRight => &["KeyD", "ArrowRight", "PadRight"],
        BindableAction::Confirm => &["Enter", "PadA"],
        BindableAction::Cancel => &["Escape", "PadB"],
        BindableAction::Pause => &["KeyP", "PadStart"],
        BindableAction::GamePrimary => &["Enter", "PadA"],
    }
}

impl Default for ControlBindings {
    fn default() -> Self {
        ControlBindings {
            tokens: BindableAction::ALL.map(defaults_for),
        }
    }
}

impl ControlBindings {
    /// The tokens bound to `action`.
    pub fn tokens(&self, action: BindableAction) -> &[&'static str] {
        self.tokens[action.index()]
    }
}

/// A friendly display label for a token (gamepad labels included).
pub fn token_label(token: &str) -> String {
    match token {
        "ArrowUp" => "↑".to_string(),
        "ArrowDown" => "↓".to_string(),
        "ArrowLeft" => "←".to_string(),
        "ArrowRight" => "→".to_string(),
        "Enter" => "ENTER".to_string(),
        "Escape" => "ESC".to_string(),
        "Space" => "SPACE".to_string(),
        "PadA" => "PAD A".to_string(),
        "PadB" => "PAD B".to_string(),
        "PadStart" => "PAD START".to_string(),
        "PadUp" => "PAD ↑".to_string(),
        "PadDown" => "PAD ↓".to_string(),
        "PadLeft" => "PAD ←".to_string(),
        "PadRight" => "PAD →".to_string(),
        other => other.strip_prefix("Key").unwrap_or(other).to_uppercase(),
    }
}
