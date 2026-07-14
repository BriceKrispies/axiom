//! The typed control profile: neutral token bindings per bindable action,
//! defaults, conflict detection, display labels, and the emergency keyboard
//! path (Enter/Escape/arrows always work in menus, so bindings can never make
//! the interface unusable). Tokens are the app's neutral input vocabulary —
//! `KeyboardEvent.code` strings and `Pad*` gamepad tokens.

/// Every rebindable action. Gameplay secondary / switch-player are RESERVED
/// bindings: the current showcase has no such gameplay actions yet (see
/// `SETTINGS.md`), but the profile carries them for the future game.
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
    GameSecondary,
    GameSwitchPlayer,
}

impl BindableAction {
    pub const ALL: [BindableAction; 10] = [
        BindableAction::NavUp,
        BindableAction::NavDown,
        BindableAction::NavLeft,
        BindableAction::NavRight,
        BindableAction::Confirm,
        BindableAction::Cancel,
        BindableAction::Pause,
        BindableAction::GamePrimary,
        BindableAction::GameSecondary,
        BindableAction::GameSwitchPlayer,
    ];

    pub fn label(self) -> &'static str {
        match self {
            BindableAction::NavUp => "MOVE UP",
            BindableAction::NavDown => "MOVE DOWN",
            BindableAction::NavLeft => "MOVE LEFT",
            BindableAction::NavRight => "MOVE RIGHT",
            BindableAction::Confirm => "CONFIRM",
            BindableAction::Cancel => "CANCEL",
            BindableAction::Pause => "PAUSE",
            BindableAction::GamePrimary => "SNAP / THROW",
            BindableAction::GameSecondary => "SECONDARY (RESERVED)",
            BindableAction::GameSwitchPlayer => "SWITCH PLAYER (RESERVED)",
        }
    }

    pub fn index(self) -> usize {
        BindableAction::ALL
            .iter()
            .position(|a| *a == self)
            .unwrap_or(0)
    }
}

/// Max tokens per action (keyboard slot + gamepad slot).
pub const MAX_TOKENS_PER_ACTION: usize = 3;

/// The control profile: an ordered token list per action.
#[derive(Debug, Clone, PartialEq)]
pub struct ControlBindings {
    tokens: [Vec<String>; BindableAction::ALL.len()],
}

fn defaults_for(action: BindableAction) -> Vec<String> {
    let list: &[&str] = match action {
        BindableAction::NavUp => &["KeyW", "ArrowUp", "PadUp"],
        BindableAction::NavDown => &["KeyS", "ArrowDown", "PadDown"],
        BindableAction::NavLeft => &["KeyA", "ArrowLeft", "PadLeft"],
        BindableAction::NavRight => &["KeyD", "ArrowRight", "PadRight"],
        BindableAction::Confirm => &["Enter", "PadA"],
        BindableAction::Cancel => &["Escape", "PadB"],
        BindableAction::Pause => &["KeyP", "PadStart"],
        BindableAction::GamePrimary => &["Enter", "PadA"],
        BindableAction::GameSecondary => &["ShiftLeft", "PadX"],
        BindableAction::GameSwitchPlayer => &["KeyQ", "PadY"],
    };
    list.iter().map(|t| t.to_string()).collect()
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
    pub fn tokens(&self, action: BindableAction) -> &[String] {
        &self.tokens[action.index()]
    }

    /// Whether `token` currently triggers `action`. Menu confirm/cancel and
    /// the arrows keep an EMERGENCY keyboard path in addition to bindings.
    pub fn matches(&self, action: BindableAction, token: &str) -> bool {
        let emergency: &[&str] = match action {
            BindableAction::Confirm => &["Enter"],
            BindableAction::Cancel => &["Escape"],
            BindableAction::NavUp => &["ArrowUp"],
            BindableAction::NavDown => &["ArrowDown"],
            BindableAction::NavLeft => &["ArrowLeft"],
            BindableAction::NavRight => &["ArrowRight"],
            _ => &[],
        };
        emergency.contains(&token) || self.tokens(action).iter().any(|t| t == token)
    }

    /// Rebind `action`'s PRIMARY slot to `token` (keeps the remaining slots).
    pub fn rebind(&mut self, action: BindableAction, token: &str) {
        let slot = &mut self.tokens[action.index()];
        slot.retain(|t| t != token);
        slot.insert(0, token.to_string());
        slot.truncate(MAX_TOKENS_PER_ACTION);
    }

    /// Restore one action to its defaults.
    pub fn restore(&mut self, action: BindableAction) {
        self.tokens[action.index()] = defaults_for(action);
    }

    /// Restore everything to defaults.
    pub fn restore_all(&mut self) {
        *self = ControlBindings::default();
    }

    /// Actions (other than `action`) already using `token`.
    pub fn conflicts(&self, action: BindableAction, token: &str) -> Vec<BindableAction> {
        BindableAction::ALL
            .into_iter()
            .filter(|a| *a != action && self.tokens(*a).iter().any(|t| t == token))
            .collect()
    }

    /// Serialize as `action=token,token` lines (persistence format).
    pub fn encode_lines(&self, out: &mut String) {
        for action in BindableAction::ALL {
            out.push_str("bind.");
            out.push_str(key_of(action));
            out.push('=');
            out.push_str(&self.tokens(action).join(","));
            out.push('\n');
        }
    }

    /// Apply one persisted `bind.<key>` line (unknown keys/tokens ignored).
    pub fn decode_line(&mut self, key: &str, value: &str) {
        let Some(action) = BindableAction::ALL.into_iter().find(|a| key_of(*a) == key) else {
            return;
        };
        let tokens: Vec<String> = value
            .split(',')
            .map(str::trim)
            .filter(|t| !t.is_empty() && t.len() <= 24 && t.chars().all(|c| c.is_ascii_graphic()))
            .take(MAX_TOKENS_PER_ACTION)
            .map(|t| t.to_string())
            .collect();
        if !tokens.is_empty() {
            self.tokens[action.index()] = tokens;
        }
    }
}

fn key_of(action: BindableAction) -> &'static str {
    match action {
        BindableAction::NavUp => "up",
        BindableAction::NavDown => "down",
        BindableAction::NavLeft => "left",
        BindableAction::NavRight => "right",
        BindableAction::Confirm => "confirm",
        BindableAction::Cancel => "cancel",
        BindableAction::Pause => "pause",
        BindableAction::GamePrimary => "primary",
        BindableAction::GameSecondary => "secondary",
        BindableAction::GameSwitchPlayer => "switch",
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
        "ShiftLeft" => "L-SHIFT".to_string(),
        "PadA" => "PAD A".to_string(),
        "PadB" => "PAD B".to_string(),
        "PadX" => "PAD X".to_string(),
        "PadY" => "PAD Y".to_string(),
        "PadStart" => "PAD START".to_string(),
        "PadUp" => "PAD ↑".to_string(),
        "PadDown" => "PAD ↓".to_string(),
        "PadLeft" => "PAD ←".to_string(),
        "PadRight" => "PAD →".to_string(),
        other => other.strip_prefix("Key").unwrap_or(other).to_uppercase(),
    }
}
