//! The arcade theme: the fixed early-2000s arcade-sports palette (charcoal
//! steel, electric blue, hot red, silver chrome, volt accent) plus the one
//! motion flag the presenter honors. Colors use the interface layer's packed
//! [`UiColor`] vocabulary.

use axiom_interface::UiColor;

use super::settings::EndZoneSettings;

/// Pack a display color for the view model.
pub const fn color(rgb: u32) -> UiColor {
    UiColor::new((rgb << 8) | 0xFF)
}

/// The computed interface theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    pub backdrop: UiColor,
    pub steel_dark: UiColor,
    pub steel_light: UiColor,
    pub chrome: UiColor,
    pub electric: UiColor,
    pub hot: UiColor,
    pub volt: UiColor,
    pub text: UiColor,
    pub text_dim: UiColor,
    pub focus_ring: UiColor,
    /// Whether large sweeps/zooms/continuous background motion are replaced
    /// with short fades.
    pub reduced_motion: bool,
}

impl Theme {
    /// Compute the theme from settings (only reduced motion varies).
    pub fn from_settings(settings: &EndZoneSettings) -> Self {
        Theme {
            backdrop: color(0x0A0D12),
            steel_dark: color(0x1A2029),
            steel_light: color(0x39424F),
            chrome: color(0xC7CFDA),
            electric: color(0x1E86FF),
            hot: color(0xE33E30),
            volt: color(0xB4F03C),
            text: color(0xEDF1F6),
            text_dim: color(0x9AA6B5),
            focus_ring: color(0x53C8FF),
            reduced_motion: settings.reduced_motion,
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Theme::from_settings(&EndZoneSettings::default())
    }
}
