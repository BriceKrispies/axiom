//! The arcade theme: every color, bevel, glow, and motion flag the presenter
//! styles from — computed HERE from the committed/working settings so
//! accessibility (high contrast, text size, reduced motion, color
//! distinction) changes the actual interface palette and behavior. Colors use
//! the interface layer's packed [`UiColor`] vocabulary.

use axiom_interface::UiColor;

use super::settings::{ColorDistinction, EndZoneSettings, TextSize, UiScale};

/// Convert a linear-RGB team color (the sim palette space) to a display sRGB
/// hex string for the presenter (gamma ≈ 1/2.2).
pub fn css_color(linear: [f32; 3]) -> String {
    let to8 = |c: f32| -> u8 {
        let clamped = c.clamp(0.0, 1.0);
        (clamped.powf(1.0 / 2.2) * 255.0).round() as u8
    };
    format!(
        "#{:02x}{:02x}{:02x}",
        to8(linear[0]),
        to8(linear[1]),
        to8(linear[2])
    )
}

/// Pack a display color for the view model.
pub const fn color(rgb: u32) -> UiColor {
    UiColor::new((rgb << 8) | 0xFF)
}

/// The computed interface theme.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Theme {
    // Core early-2000s arcade-sports palette: charcoal steel, electric blue,
    // hot red, silver chrome, volt accent.
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
    /// Whether panels use the high-contrast variant (solid fills, thicker
    /// borders, brighter text).
    pub high_contrast: bool,
    /// Whether large sweeps/zooms/continuous background motion are replaced
    /// with short fades.
    pub reduced_motion: bool,
    /// Whether non-color team cues (abbreviations, silhouettes, patterns,
    /// HOME/AWAY tags) are forced on.
    pub enhanced_distinction: bool,
    /// Interface scale multiplier (UI scale × 1).
    pub ui_scale: f32,
    /// Text scale multiplier on top of the UI scale.
    pub text_scale: f32,
}

impl Theme {
    /// Compute the theme from settings.
    pub fn from_settings(settings: &EndZoneSettings) -> Self {
        let high_contrast = settings.high_contrast;
        let (text, text_dim) = if high_contrast {
            (color(0xFFFFFF), color(0xE8ECF2))
        } else {
            (color(0xEDF1F6), color(0x9AA6B5))
        };
        Theme {
            backdrop: color(0x0A0D12),
            steel_dark: if high_contrast {
                color(0x11151C)
            } else {
                color(0x1A2029)
            },
            steel_light: if high_contrast {
                color(0x2A313D)
            } else {
                color(0x39424F)
            },
            chrome: color(0xC7CFDA),
            electric: color(0x1E86FF),
            hot: color(0xE33E30),
            volt: color(0xB4F03C),
            text,
            text_dim,
            focus_ring: if high_contrast {
                color(0xFFE24A)
            } else {
                color(0x53C8FF)
            },
            high_contrast,
            reduced_motion: settings.reduced_motion,
            enhanced_distinction: settings.color_distinction == ColorDistinction::Enhanced,
            ui_scale: settings.ui_scale.factor(),
            text_scale: settings.text_size.factor(),
        }
    }

    /// A compact fingerprint (used by tests to prove settings change the
    /// computed palette, and by the presenter as a style-dirty key).
    pub fn fingerprint(&self) -> u64 {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        let mut fold = |v: u64| {
            hash ^= v;
            hash = hash.wrapping_mul(0x1000_0000_01b3);
        };
        for c in [
            self.backdrop,
            self.steel_dark,
            self.steel_light,
            self.chrome,
            self.electric,
            self.hot,
            self.volt,
            self.text,
            self.text_dim,
            self.focus_ring,
        ] {
            fold(u64::from(c.rgba()));
        }
        fold(u64::from(self.high_contrast));
        fold(u64::from(self.reduced_motion));
        fold(u64::from(self.enhanced_distinction));
        fold((self.ui_scale * 100.0) as u64);
        fold((self.text_scale * 100.0) as u64);
        hash
    }
}

impl Default for Theme {
    fn default() -> Self {
        Theme::from_settings(&EndZoneSettings::default())
    }
}

/// Convenience: the settings that shape layout scale.
pub fn scale_of(ui: UiScale, text: TextSize) -> (f32, f32) {
    (ui.factor(), text.factor())
}
