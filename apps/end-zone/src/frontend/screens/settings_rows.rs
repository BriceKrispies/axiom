//! The settings field table: which typed fields each category shows, and the
//! row view each renders as (label, value, control kind, and the small-print
//! detail naming the REAL subsystem the setting drives — see `SETTINGS.md`).
//! Value stepping lives in [`super::settings_values`].

use crate::frontend::bindings::{token_label, BindableAction, ControlBindings};
use crate::frontend::settings::{EndZoneSettings, SettingsCategory, Volume};
use crate::frontend::widgets::{RowControl, SettingRow};

pub use super::settings_values::{activate_field, adjust_field};
use super::settings_values::{
    arrows, label_option, CAMERA_STYLE, DIFFICULTY, DISTINCTION, EFFECTS, FLASH, GAME_SPEED,
    RENDER_QUALITY, SCREEN_SHAKE, TEXT_SIZE, UI_SCALE,
};

/// One editable field (a typed lens into [`EndZoneSettings`] or the bindings).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SettingField {
    Difficulty,
    GameSpeed,
    CameraStyle,
    MasterVolume,
    MusicVolume,
    EffectsVolume,
    CrowdVolume,
    MenuVolume,
    MuteWhenUnfocused,
    RenderQuality,
    EffectsIntensity,
    UiScale,
    ScreenShake,
    ReducedMotion,
    HighContrast,
    FlashIntensity,
    TextSize,
    ColorDistinction,
    Bind(BindableAction),
    RestoreBindings,
}

/// The fields each category tab shows, in order.
pub fn fields_for(category: SettingsCategory) -> Vec<SettingField> {
    match category {
        SettingsCategory::Gameplay => vec![
            SettingField::Difficulty,
            SettingField::GameSpeed,
            SettingField::CameraStyle,
        ],
        SettingsCategory::Audio => vec![
            SettingField::MasterVolume,
            SettingField::MenuVolume,
            SettingField::MusicVolume,
            SettingField::EffectsVolume,
            SettingField::CrowdVolume,
            SettingField::MuteWhenUnfocused,
        ],
        SettingsCategory::Video => vec![
            SettingField::RenderQuality,
            SettingField::EffectsIntensity,
            SettingField::UiScale,
        ],
        SettingsCategory::Controls => BindableAction::ALL
            .into_iter()
            .map(SettingField::Bind)
            .chain([SettingField::RestoreBindings])
            .collect(),
        SettingsCategory::Accessibility => vec![
            SettingField::ScreenShake,
            SettingField::ReducedMotion,
            SettingField::HighContrast,
            SettingField::FlashIntensity,
            SettingField::TextSize,
            SettingField::ColorDistinction,
        ],
    }
}

fn selector<T: Copy + PartialEq>(
    label: &str,
    options: &[(T, &'static str)],
    value: T,
    detail: &str,
) -> SettingRow {
    let (has_prev, has_next) = arrows(options, value);
    SettingRow::selector(label, label_option(options, value), has_prev, has_next)
        .with_detail(detail)
}

fn volume_row(label: &str, volume: Volume, detail: &str) -> SettingRow {
    SettingRow::volume(label, volume.0, Volume::MAX).with_detail(detail)
}

fn binding_row(
    action: BindableAction,
    bindings: &ControlBindings,
    capture: Option<(BindableAction, u32)>,
) -> SettingRow {
    let capturing = capture.map(|(a, _)| a == action).unwrap_or(false);
    let tokens: Vec<String> = bindings
        .tokens(action)
        .iter()
        .map(|t| token_label(t))
        .collect();
    // Menu and gameplay actions may legitimately share a token (ENTER is
    // both CONFIRM and SNAP/THROW); only same-group overlaps are conflicts.
    let game_group = |a: BindableAction| {
        matches!(
            a,
            BindableAction::GamePrimary
                | BindableAction::GameSecondary
                | BindableAction::GameSwitchPlayer
        )
    };
    let conflict = bindings
        .tokens(action)
        .first()
        .map(|t| {
            bindings
                .conflicts(action, t)
                .into_iter()
                .any(|other| game_group(other) == game_group(action))
        })
        .unwrap_or(false);
    let value = if capturing {
        "PRESS A KEY\u{2026}".to_string()
    } else {
        tokens.join(" / ")
    };
    SettingRow {
        label: action.label().to_string(),
        value,
        control: RowControl::Binding {
            tokens,
            capturing,
            conflict,
        },
        detail: None,
    }
}

/// Build the row view for `field` against the working state.
pub fn row_view(
    field: SettingField,
    s: &EndZoneSettings,
    bindings: &ControlBindings,
    capture: Option<(BindableAction, u32)>,
) -> SettingRow {
    match field {
        SettingField::Difficulty => selector(
            "DIFFICULTY",
            &DIFFICULTY,
            s.difficulty,
            "Default for new matches: opponent reaction, pursuit, tackle range",
        ),
        SettingField::GameSpeed => selector(
            "GAME SPEED",
            &GAME_SPEED,
            s.game_speed,
            "Default for new matches: simulation steps per frame",
        ),
        SettingField::CameraStyle => selector(
            "CAMERA",
            &CAMERA_STYLE,
            s.camera_style,
            "Follow distance and field of view of the match camera",
        ),
        SettingField::MasterVolume => volume_row(
            "MASTER VOLUME",
            s.master_volume,
            "Overall output level (engine master gain)",
        ),
        SettingField::MenuVolume => volume_row(
            "MENU VOLUME",
            s.menu_volume,
            "Interface tone level (navigate / confirm / impact stingers)",
        ),
        SettingField::MusicVolume => volume_row(
            "MUSIC VOLUME",
            s.music_volume,
            "Reserved: the engine's music playback arm is not built yet",
        ),
        SettingField::EffectsVolume => volume_row(
            "EFFECTS VOLUME",
            s.effects_volume,
            "Reserved: match sound effects await engine sample playback",
        ),
        SettingField::CrowdVolume => volume_row(
            "CROWD VOLUME",
            s.crowd_volume,
            "Reserved: crowd bed awaits engine sample playback",
        ),
        SettingField::MuteWhenUnfocused => {
            SettingRow::toggle("MUTE WHEN UNFOCUSED", s.mute_when_unfocused)
                .with_detail("Silence all audio while the game tab is in the background")
        }
        SettingField::RenderQuality => selector(
            "RENDER QUALITY",
            &RENDER_QUALITY,
            s.render_quality,
            "Contact-shadow casters and the fine field-marking mesh",
        ),
        SettingField::EffectsIntensity => selector(
            "EFFECTS INTENSITY",
            &EFFECTS,
            s.effects_intensity,
            "Dust, rings, streaks and other impact effect density",
        ),
        SettingField::UiScale => selector(
            "UI SCALE",
            &UI_SCALE,
            s.ui_scale,
            "Scales the whole interface (live preview)",
        ),
        SettingField::ScreenShake => selector(
            "SCREEN SHAKE",
            &SCREEN_SHAKE,
            s.screen_shake,
            "Camera impulse strength on hits, throws and landings",
        ),
        SettingField::ReducedMotion => SettingRow::toggle("REDUCED MOTION", s.reduced_motion)
            .with_detail("Replaces sweeps/zooms with short fades; stills decorative motion"),
        SettingField::HighContrast => SettingRow::toggle("HIGH CONTRAST", s.high_contrast)
            .with_detail("Stronger text/background separation across the interface"),
        SettingField::FlashIntensity => selector(
            "FLASH INTENSITY",
            &FLASH,
            s.flash_intensity,
            "Full-screen flash strength on throws and catches",
        ),
        SettingField::TextSize => selector(
            "TEXT SIZE",
            &TEXT_SIZE,
            s.text_size,
            "Scales interface text on top of the UI scale",
        ),
        SettingField::ColorDistinction => selector(
            "COLOR DISTINCTION",
            &DISTINCTION,
            s.color_distinction,
            "Enhanced adds non-color team cues (labels, patterns)",
        ),
        SettingField::Bind(action) => binding_row(action, bindings, capture),
        SettingField::RestoreBindings => SettingRow {
            label: "RESTORE DEFAULT CONTROLS".to_string(),
            value: String::new(),
            control: RowControl::Action,
            detail: Some("Resets every binding in the working copy".to_string()),
        },
    }
}
