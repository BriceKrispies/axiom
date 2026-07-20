//! The typed `EndZoneSettings` model: four settings, each mapping to a real
//! subsystem (documented per-field and in `SETTINGS.md`). Values are bounded by
//! construction, so any loaded or edited state is valid.

use crate::launch::ScreenShake;

/// A bounded `0..=10` volume step (normalized via [`Volume::ratio`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Volume(pub u8);

impl Volume {
    pub const MAX: u8 = 10;

    pub fn clamped(value: u8) -> Self {
        Volume(value.min(Self::MAX))
    }

    /// Normalized `0.0..=1.0` gain.
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

/// The complete typed settings model — master volume, music volume, screen
/// shake, reduced motion, and nothing else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EndZoneSettings {
    /// Scales all current audio output (the master gain the tone path reads).
    pub master_volume: Volume,
    /// Scales the menu music beneath the master gain (menu music only, so it can
    /// be lowered without muting UI sound effects).
    pub music_volume: Volume,
    /// Scales actual gameplay camera impulses (`Off` is exactly zero shake).
    pub screen_shake: ScreenShake,
    /// Suppresses large menu sweeps, UI scaling, and nonessential camera
    /// presentation while preserving gameplay clarity.
    pub reduced_motion: bool,
}

impl Default for EndZoneSettings {
    fn default() -> Self {
        EndZoneSettings {
            master_volume: Volume(8),
            music_volume: Volume(7),
            screen_shake: ScreenShake::Full,
            reduced_motion: false,
        }
    }
}

impl EndZoneSettings {
    /// Clamp every value (volumes) into range.
    pub fn sanitized(mut self) -> Self {
        self.master_volume = Volume::clamped(self.master_volume.0);
        self.music_volume = Volume::clamped(self.music_volume.0);
        self
    }
}
