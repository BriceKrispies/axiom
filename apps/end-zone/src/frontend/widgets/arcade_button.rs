//! The arcade button: an angled metallic plate with a chrome bevel, a
//! procedural glow when focused, and a squash-and-snap press animation —
//! all styling is derived from these typed fields by the presenter.

/// Visual weight of a button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonStyle {
    /// The big confirm plates (START MATCH, main menu entries).
    Primary,
    /// Destructive / exit actions (RETURN TO MENU, DISCARD).
    Danger,
    /// Quiet secondary actions (BACK).
    Flat,
}

/// One arcade button.
#[derive(Debug, Clone, PartialEq)]
pub struct ArcadeButton {
    pub label: String,
    pub style: ButtonStyle,
    /// Angled (trapezoidal) plate silhouette.
    pub angled: bool,
    /// Team-color plate tint (CSS), when team-scoped.
    pub tint: Option<String>,
}

impl ArcadeButton {
    pub fn primary(label: &str) -> Self {
        ArcadeButton {
            label: label.to_string(),
            style: ButtonStyle::Primary,
            angled: true,
            tint: None,
        }
    }

    pub fn danger(label: &str) -> Self {
        ArcadeButton {
            label: label.to_string(),
            style: ButtonStyle::Danger,
            angled: true,
            tint: None,
        }
    }

    pub fn flat(label: &str) -> Self {
        ArcadeButton {
            label: label.to_string(),
            style: ButtonStyle::Flat,
            angled: false,
            tint: None,
        }
    }
}
