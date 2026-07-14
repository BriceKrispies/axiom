//! Typed value stepping for the settings fields: the explicit option tables
//! (value ↔ display label), bounded left/right stepping, and confirm-cycling.
//! Everything is table-driven — no per-setting flow code.

use crate::frontend::settings::{
    ColorDistinction, EndZoneSettings, RenderQuality, TextSize, UiScale, Volume,
};
use crate::launch::{
    CameraStyle, Difficulty, EffectsIntensity, FlashIntensity, GameSpeed, ScreenShake,
};

use super::settings_rows::SettingField;

pub(super) const DIFFICULTY: [(Difficulty, &str); 3] = [
    (Difficulty::Rookie, "ROOKIE"),
    (Difficulty::Pro, "PRO"),
    (Difficulty::AllStar, "ALL-STAR"),
];
pub(super) const GAME_SPEED: [(GameSpeed, &str); 3] = [
    (GameSpeed::Normal, "NORMAL"),
    (GameSpeed::Fast, "FAST"),
    (GameSpeed::Turbo, "TURBO"),
];
pub(super) const CAMERA_STYLE: [(CameraStyle, &str); 3] = [
    (CameraStyle::Arcade, "ARCADE"),
    (CameraStyle::Wide, "WIDE"),
    (CameraStyle::Close, "CLOSE"),
];
pub(super) const RENDER_QUALITY: [(RenderQuality, &str); 3] = [
    (RenderQuality::Low, "LOW"),
    (RenderQuality::Medium, "MEDIUM"),
    (RenderQuality::High, "HIGH"),
];
pub(super) const EFFECTS: [(EffectsIntensity, &str); 3] = [
    (EffectsIntensity::Low, "LOW"),
    (EffectsIntensity::Medium, "MEDIUM"),
    (EffectsIntensity::High, "HIGH"),
];
pub(super) const UI_SCALE: [(UiScale, &str); 3] = [
    (UiScale::Small, "SMALL"),
    (UiScale::Normal, "NORMAL"),
    (UiScale::Large, "LARGE"),
];
pub(super) const SCREEN_SHAKE: [(ScreenShake, &str); 3] = [
    (ScreenShake::Off, "OFF"),
    (ScreenShake::Low, "LOW"),
    (ScreenShake::Full, "FULL"),
];
pub(super) const FLASH: [(FlashIntensity, &str); 3] = [
    (FlashIntensity::Off, "OFF"),
    (FlashIntensity::Low, "LOW"),
    (FlashIntensity::Full, "FULL"),
];
pub(super) const TEXT_SIZE: [(TextSize, &str); 2] =
    [(TextSize::Normal, "NORMAL"), (TextSize::Large, "LARGE")];
pub(super) const DISTINCTION: [(ColorDistinction, &str); 2] = [
    (ColorDistinction::Standard, "STANDARD"),
    (ColorDistinction::Enhanced, "ENHANCED"),
];

fn step_option<T: Copy + PartialEq>(options: &[(T, &str)], value: T, dx: i32) -> T {
    let index = options.iter().position(|(v, _)| *v == value).unwrap_or(0) as i32;
    let last = options.len() as i32 - 1;
    options[(index + dx).clamp(0, last) as usize].0
}

fn cycle_option<T: Copy + PartialEq>(options: &[(T, &str)], value: T) -> T {
    let index = options.iter().position(|(v, _)| *v == value).unwrap_or(0);
    options[(index + 1) % options.len()].0
}

pub(super) fn label_option<T: Copy + PartialEq>(
    options: &[(T, &'static str)],
    value: T,
) -> &'static str {
    options
        .iter()
        .find(|(v, _)| *v == value)
        .map(|(_, l)| *l)
        .unwrap_or("?")
}

pub(super) fn arrows<T: Copy + PartialEq>(options: &[(T, &str)], value: T) -> (bool, bool) {
    let index = options.iter().position(|(v, _)| *v == value).unwrap_or(0);
    (index > 0, index + 1 < options.len())
}

fn step_volume(volume: &mut Volume, dx: i32) -> bool {
    let next = volume.step(dx > 0);
    let changed = next != *volume;
    *volume = next;
    changed
}

fn step_assign<T: Copy + PartialEq>(slot: &mut T, options: &[(T, &str)], dx: i32) -> bool {
    let next = step_option(options, *slot, dx);
    let changed = next != *slot;
    *slot = next;
    changed
}

fn cycle_assign<T: Copy + PartialEq>(slot: &mut T, options: &[(T, &str)]) -> bool {
    *slot = cycle_option(options, *slot);
    true
}

/// Apply a left/right step to `field`. Returns whether the value changed.
pub fn adjust_field(field: SettingField, s: &mut EndZoneSettings, dx: i32) -> bool {
    let set = |slot: &mut bool, dx: i32| -> bool {
        let next = dx > 0;
        let changed = *slot != next;
        *slot = next;
        changed
    };
    match field {
        SettingField::Difficulty => step_assign(&mut s.difficulty, &DIFFICULTY, dx),
        SettingField::GameSpeed => step_assign(&mut s.game_speed, &GAME_SPEED, dx),
        SettingField::CameraStyle => step_assign(&mut s.camera_style, &CAMERA_STYLE, dx),
        SettingField::MasterVolume => step_volume(&mut s.master_volume, dx),
        SettingField::MusicVolume => step_volume(&mut s.music_volume, dx),
        SettingField::EffectsVolume => step_volume(&mut s.effects_volume, dx),
        SettingField::CrowdVolume => step_volume(&mut s.crowd_volume, dx),
        SettingField::MenuVolume => step_volume(&mut s.menu_volume, dx),
        SettingField::MuteWhenUnfocused => set(&mut s.mute_when_unfocused, dx),
        SettingField::RenderQuality => step_assign(&mut s.render_quality, &RENDER_QUALITY, dx),
        SettingField::EffectsIntensity => step_assign(&mut s.effects_intensity, &EFFECTS, dx),
        SettingField::UiScale => step_assign(&mut s.ui_scale, &UI_SCALE, dx),
        SettingField::ScreenShake => step_assign(&mut s.screen_shake, &SCREEN_SHAKE, dx),
        SettingField::ReducedMotion => set(&mut s.reduced_motion, dx),
        SettingField::HighContrast => set(&mut s.high_contrast, dx),
        SettingField::FlashIntensity => step_assign(&mut s.flash_intensity, &FLASH, dx),
        SettingField::TextSize => step_assign(&mut s.text_size, &TEXT_SIZE, dx),
        SettingField::ColorDistinction => step_assign(&mut s.color_distinction, &DISTINCTION, dx),
        SettingField::Bind(_) | SettingField::RestoreBindings => false,
    }
}

/// Confirm on a value row: toggles flip, selectors cycle forward.
pub fn activate_field(field: SettingField, s: &mut EndZoneSettings) -> bool {
    let flip = |slot: &mut bool| -> bool {
        *slot = !*slot;
        true
    };
    match field {
        SettingField::MuteWhenUnfocused => flip(&mut s.mute_when_unfocused),
        SettingField::ReducedMotion => flip(&mut s.reduced_motion),
        SettingField::HighContrast => flip(&mut s.high_contrast),
        SettingField::Difficulty => cycle_assign(&mut s.difficulty, &DIFFICULTY),
        SettingField::GameSpeed => cycle_assign(&mut s.game_speed, &GAME_SPEED),
        SettingField::CameraStyle => cycle_assign(&mut s.camera_style, &CAMERA_STYLE),
        SettingField::RenderQuality => cycle_assign(&mut s.render_quality, &RENDER_QUALITY),
        SettingField::EffectsIntensity => cycle_assign(&mut s.effects_intensity, &EFFECTS),
        SettingField::UiScale => cycle_assign(&mut s.ui_scale, &UI_SCALE),
        SettingField::ScreenShake => cycle_assign(&mut s.screen_shake, &SCREEN_SHAKE),
        SettingField::FlashIntensity => cycle_assign(&mut s.flash_intensity, &FLASH),
        SettingField::TextSize => cycle_assign(&mut s.text_size, &TEXT_SIZE),
        SettingField::ColorDistinction => cycle_assign(&mut s.color_distinction, &DISTINCTION),
        SettingField::MasterVolume
        | SettingField::MusicVolume
        | SettingField::EffectsVolume
        | SettingField::CrowdVolume
        | SettingField::MenuVolume => false,
        SettingField::Bind(_) | SettingField::RestoreBindings => false,
    }
}
