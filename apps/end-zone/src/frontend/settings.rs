//! The typed `EndZoneSettings` model. Every exposed setting maps to a real
//! subsystem (documented per-field and in `SETTINGS.md`); nothing here is
//! decorative. Values are bounded enums or clamped steps, so any loaded or
//! edited state is valid by construction.

use crate::launch::{
    CameraStyle, Difficulty, EffectsIntensity, FlashIntensity, GameSpeed, ScreenShake,
};

/// The five settings categories, in tab order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsCategory {
    Gameplay,
    Audio,
    Video,
    Controls,
    Accessibility,
}

impl SettingsCategory {
    pub const ALL: [SettingsCategory; 5] = [
        SettingsCategory::Gameplay,
        SettingsCategory::Audio,
        SettingsCategory::Video,
        SettingsCategory::Controls,
        SettingsCategory::Accessibility,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SettingsCategory::Gameplay => "GAMEPLAY",
            SettingsCategory::Audio => "AUDIO",
            SettingsCategory::Video => "VIDEO",
            SettingsCategory::Controls => "CONTROLS",
            SettingsCategory::Accessibility => "ACCESSIBILITY",
        }
    }

    pub fn index(self) -> usize {
        SettingsCategory::ALL
            .iter()
            .position(|c| *c == self)
            .unwrap_or(0)
    }
}

/// Render quality: real scene detail controls (contact-shadow casters and
/// the fine field-marking mesh) — see `RenderDetail` below.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RenderQuality {
    Low,
    #[default]
    Medium,
    High,
}

/// The resolved scene detail a render quality selects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderDetail {
    /// Player/ball contact shadows: 0 = none, 1 = core body parts, 2 = all.
    pub shadow_tier: u8,
    /// Whether the fine marking mesh (one-yard ticks + hashes) is shown.
    pub fine_markings: bool,
}

impl RenderQuality {
    pub fn detail(self) -> RenderDetail {
        match self {
            RenderQuality::Low => RenderDetail {
                shadow_tier: 0,
                fine_markings: false,
            },
            RenderQuality::Medium => RenderDetail {
                shadow_tier: 1,
                fine_markings: true,
            },
            RenderQuality::High => RenderDetail {
                shadow_tier: 2,
                fine_markings: true,
            },
        }
    }
}

/// UI scale steps (multiplies the whole interface layout).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UiScale {
    Small,
    #[default]
    Normal,
    Large,
}

impl UiScale {
    pub fn factor(self) -> f32 {
        match self {
            UiScale::Small => 0.85,
            UiScale::Normal => 1.0,
            UiScale::Large => 1.18,
        }
    }
}

/// Accessibility text size (scales interface text on top of the UI scale).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextSize {
    #[default]
    Normal,
    Large,
}

impl TextSize {
    pub fn factor(self) -> f32 {
        match self {
            TextSize::Normal => 1.0,
            TextSize::Large => 1.25,
        }
    }
}

/// Color-distinction mode: `Enhanced` adds non-color cues (abbreviations,
/// emblem silhouettes, HOME/AWAY labels, panel patterns) wherever team color
/// is meaningful.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorDistinction {
    #[default]
    Standard,
    Enhanced,
}

/// A bounded 0..=10 volume step (normalized via [`Volume::ratio`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Volume(pub u8);

impl Volume {
    pub const MAX: u8 = 10;

    pub fn clamped(value: u8) -> Self {
        Volume(value.min(Self::MAX))
    }

    pub fn ratio(self) -> f32 {
        f32::from(self.0.min(Self::MAX)) / f32::from(Self::MAX)
    }

    pub fn step(self, up: bool) -> Self {
        Volume(if up {
            (self.0 + 1).min(Self::MAX)
        } else {
            self.0.saturating_sub(1)
        })
    }
}

/// The complete typed settings model.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EndZoneSettings {
    // GAMEPLAY — real named tuning profiles (see `crate::launch`).
    pub difficulty: Difficulty,
    pub game_speed: GameSpeed,
    pub camera_style: CameraStyle,
    // AUDIO — master/menu are audible today (procedural tones + master
    // gain); music/sfx/crowd are typed but have no audible path yet (the
    // engine's sample/music playback arm is a stub) — documented in
    // SETTINGS.md.
    pub master_volume: Volume,
    pub music_volume: Volume,
    pub effects_volume: Volume,
    pub crowd_volume: Volume,
    pub menu_volume: Volume,
    pub mute_when_unfocused: bool,
    // VIDEO — real scene/presentation controls.
    pub render_quality: RenderQuality,
    pub effects_intensity: EffectsIntensity,
    pub ui_scale: UiScale,
    // ACCESSIBILITY — real presentation/interface controls.
    pub screen_shake: ScreenShake,
    pub reduced_motion: bool,
    pub high_contrast: bool,
    pub flash_intensity: FlashIntensity,
    pub text_size: TextSize,
    pub color_distinction: ColorDistinction,
}

impl Default for EndZoneSettings {
    fn default() -> Self {
        EndZoneSettings {
            difficulty: Difficulty::Pro,
            game_speed: GameSpeed::Normal,
            camera_style: CameraStyle::Arcade,
            master_volume: Volume(8),
            music_volume: Volume(6),
            effects_volume: Volume(8),
            crowd_volume: Volume(6),
            menu_volume: Volume(7),
            mute_when_unfocused: true,
            render_quality: RenderQuality::Medium,
            effects_intensity: EffectsIntensity::Medium,
            ui_scale: UiScale::Normal,
            screen_shake: ScreenShake::Full,
            reduced_motion: false,
            high_contrast: false,
            flash_intensity: FlashIntensity::Full,
            text_size: TextSize::Normal,
            color_distinction: ColorDistinction::Standard,
        }
    }
}

impl EndZoneSettings {
    /// Every value is bounded by type; volumes additionally clamp here.
    pub fn sanitized(mut self) -> Self {
        self.master_volume = Volume::clamped(self.master_volume.0);
        self.music_volume = Volume::clamped(self.music_volume.0);
        self.effects_volume = Volume::clamped(self.effects_volume.0);
        self.crowd_volume = Volume::clamped(self.crowd_volume.0);
        self.menu_volume = Volume::clamped(self.menu_volume.0);
        self
    }
}
