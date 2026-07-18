//! Compact, versioned frontend persistence behind an app-local store
//! abstraction. The profile is exactly the three retained settings, encoded as
//! a small `key=value` text stamped with a version. Every loaded value is
//! validated and anything invalid or unknown falls back to its default without
//! panicking; a persistence failure never blocks the title or gameplay — it
//! logs through the kernel's structured logging and continues with defaults.

use axiom_kernel::{LogLevel, LogRecord, LogSink};

use crate::launch::ScreenShake;

use super::settings::{EndZoneSettings, Volume};

/// The current persisted profile version.
pub const PROFILE_VERSION: u32 = 1;

/// The storage abstraction: the wasm edge adapts it onto the browser's
/// sanctioned storage; tests use [`MemoryStore`]. String-in / string-out only.
pub trait ProfileStore {
    fn load(&self) -> Option<String>;
    fn save(&mut self, profile: &str) -> bool;
    fn clear(&mut self);
}

/// The deterministic in-memory store used by tests (and as the native fallback
/// when no browser storage exists).
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

/// Everything the frontend persists: only the three retained settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FrontendProfile {
    pub settings: EndZoneSettings,
}

impl FrontendProfile {
    /// Load through the store; any failure logs and yields defaults.
    pub fn load_from(store: &dyn ProfileStore, sink: &mut dyn LogSink) -> FrontendProfile {
        match store.load() {
            Some(text) => FrontendProfile {
                settings: decode(&text),
            },
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
        if !store.save(&encode(&self.settings)) {
            sink.record(LogRecord::new(
                LogLevel::Warn,
                "frontend.persistence",
                2,
                "profile save failed; continuing without persistence",
            ));
        }
    }
}

/// Encode the three settings as a compact versioned text.
pub fn encode(settings: &EndZoneSettings) -> String {
    format!(
        "endzone.v={PROFILE_VERSION}\nmaster_volume={}\nscreen_shake={}\nreduced_motion={}\n",
        settings.master_volume.0,
        shake_key(settings.screen_shake),
        u8::from(settings.reduced_motion),
    )
}

/// Decode a persisted text into validated settings, falling back per field.
pub fn decode(text: &str) -> EndZoneSettings {
    let mut settings = EndZoneSettings::default();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "master_volume" => {
                if let Ok(v) = value.trim().parse::<u8>() {
                    settings.master_volume = Volume::clamped(v);
                }
            }
            "screen_shake" => {
                if let Some(shake) = parse_shake(value.trim()) {
                    settings.screen_shake = shake;
                }
            }
            "reduced_motion" => {
                if let Ok(v) = value.trim().parse::<u8>() {
                    settings.reduced_motion = v != 0;
                }
            }
            _ => {}
        }
    }
    settings.sanitized()
}

fn shake_key(shake: ScreenShake) -> &'static str {
    match shake {
        ScreenShake::Off => "off",
        ScreenShake::Low => "low",
        ScreenShake::Full => "full",
    }
}

fn parse_shake(value: &str) -> Option<ScreenShake> {
    match value {
        "off" => Some(ScreenShake::Off),
        "low" => Some(ScreenShake::Low),
        "full" => Some(ScreenShake::Full),
        _ => None,
    }
}
