//! Versioned frontend persistence behind an app-local store abstraction:
//! `load / save / clear / migrate`. The profile is a line-oriented
//! `key=value` text (version-stamped; codec in [`super::profile_codec`]);
//! every loaded value is validated and anything invalid or unknown falls
//! back to defaults without panicking. A persistence failure never blocks
//! the frontend — it logs through the kernel's structured logging and
//! continues with defaults.

use axiom_kernel::{LogLevel, LogRecord, LogSink};

use crate::data::team::LeagueTeamId;
use crate::launch::ControlProfileId;

use super::bindings::ControlBindings;
use super::settings::{EndZoneSettings, SettingsCategory};

/// The current persisted profile version.
pub const PROFILE_VERSION: u32 = 1;

/// The storage abstraction: the wasm edge adapts it onto the browser's
/// sanctioned storage; tests use [`MemoryStore`]. String-in/string-out only.
pub trait ProfileStore {
    fn load(&self) -> Option<String>;
    fn save(&mut self, profile: &str) -> bool;
    fn clear(&mut self);
}

/// The deterministic in-memory store used by tests (and as the native
/// fallback when no browser storage exists).
#[derive(Debug, Default, Clone)]
pub struct MemoryStore {
    slot: Option<String>,
}

impl ProfileStore for MemoryStore {
    fn load(&self) -> Option<String> {
        self.slot.clone()
    }

    fn save(&mut self, profile: &str) -> bool {
        self.slot = Some(profile.to_string());
        true
    }

    fn clear(&mut self) {
        self.slot = None;
    }
}

/// Everything the frontend persists.
#[derive(Debug, Clone, PartialEq)]
pub struct FrontendProfile {
    pub settings: EndZoneSettings,
    pub bindings: ControlBindings,
    pub last_player_team: LeagueTeamId,
    pub last_opponent_team: LeagueTeamId,
    pub last_category: SettingsCategory,
    pub control_profile: ControlProfileId,
}

impl Default for FrontendProfile {
    fn default() -> Self {
        FrontendProfile {
            settings: EndZoneSettings::default(),
            bindings: ControlBindings::default(),
            last_player_team: LeagueTeamId(0),
            last_opponent_team: LeagueTeamId(1),
            last_category: SettingsCategory::Gameplay,
            control_profile: ControlProfileId(0),
        }
    }
}

impl FrontendProfile {
    /// Load through the store; any failure logs and yields defaults.
    pub fn load_from(store: &dyn ProfileStore, sink: &mut dyn LogSink) -> FrontendProfile {
        match store.load() {
            Some(text) => FrontendProfile::decode(&text),
            None => {
                sink.record(LogRecord::new(
                    LogLevel::Info,
                    "frontend.persistence",
                    1,
                    "no stored profile; using defaults",
                ));
                FrontendProfile::default()
            }
        }
    }

    /// Save through the store; failure logs and continues.
    pub fn save_to(&self, store: &mut dyn ProfileStore, sink: &mut dyn LogSink) {
        if !store.save(&self.encode()) {
            sink.record(LogRecord::new(
                LogLevel::Warn,
                "frontend.persistence",
                2,
                "profile save failed; continuing without persistence",
            ));
        }
    }
}
