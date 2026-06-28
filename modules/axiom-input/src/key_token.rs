//! [`KeyToken`] — a layout-stable, neutral name for one key, button, or gesture
//! input the platform edge decodes a raw device event into.

/// A neutral input token: the layout-stable string the host decodes a raw
/// `KeyboardEvent.code`/pointer-button/gesture into before it reaches this
/// module (`"KeyW"`, `"ArrowUp"`, `"Mouse0"`). It carries no behaviour — it is
/// the noun a [`crate::DeviceFrame`] reports as down and that
/// [`crate::InputState::bind_action`] maps to an action. The shape mirrors the
/// interface layer's keymap *token*, but this module owns its own guard-free
/// table and never depends on that UI layer.
#[derive(Debug, Clone, PartialEq)]
pub struct KeyToken(String);

impl KeyToken {
    /// Intern a token from its layout-stable name. Two tokens are equal iff their
    /// names are.
    pub fn new(name: &str) -> Self {
        KeyToken(name.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_names_compare_equal_distinct_names_do_not() {
        assert_eq!(KeyToken::new("KeyW"), KeyToken::new("KeyW"));
        assert_ne!(KeyToken::new("KeyW"), KeyToken::new("KeyA"));
    }

    #[test]
    fn token_clones_equal() {
        let token = KeyToken::new("ArrowUp");
        assert_eq!(token.clone(), token);
    }
}
