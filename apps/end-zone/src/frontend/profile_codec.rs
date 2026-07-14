//! The versioned text codec for [`FrontendProfile`]: line-oriented
//! `key=value`, every decoded value validated against an explicit keyword
//! table with per-field fallback — a corrupt or foreign profile can never
//! panic or produce an out-of-range setting.

use crate::data::team::{LeagueTeamId, LEAGUE_SIZE};
use crate::launch::{
    CameraStyle, ControlProfileId, Difficulty, EffectsIntensity, FlashIntensity, GameSpeed,
    ScreenShake, CONTROL_PROFILE_COUNT,
};

use super::persistence::{FrontendProfile, PROFILE_VERSION};
use super::settings::{
    ColorDistinction, RenderQuality, SettingsCategory, TextSize, UiScale, Volume,
};

/// Pick a value from an explicit keyword table; unknown keywords fall back.
fn pick<T: Copy>(value: &str, table: &[(&str, T)], fallback: T) -> T {
    table
        .iter()
        .find(|(key, _)| *key == value)
        .map(|(_, v)| *v)
        .unwrap_or(fallback)
}

impl FrontendProfile {
    /// Encode as the versioned text profile.
    pub fn encode(&self) -> String {
        let s = &self.settings;
        let mut out = String::with_capacity(640);
        let mut line = |k: &str, v: String| {
            out.push_str(k);
            out.push('=');
            out.push_str(&v);
            out.push('\n');
        };
        line("v", PROFILE_VERSION.to_string());
        line(
            "difficulty",
            enum_key(s.difficulty as u8, &["rookie", "pro", "allstar"]),
        );
        line(
            "speed",
            enum_key(s.game_speed as u8, &["normal", "fast", "turbo"]),
        );
        line(
            "camera",
            enum_key(s.camera_style as u8, &["arcade", "wide", "close"]),
        );
        line("vol.master", s.master_volume.0.to_string());
        line("vol.music", s.music_volume.0.to_string());
        line("vol.effects", s.effects_volume.0.to_string());
        line("vol.crowd", s.crowd_volume.0.to_string());
        line("vol.menu", s.menu_volume.0.to_string());
        line("mute.unfocused", (s.mute_when_unfocused as u8).to_string());
        line(
            "quality",
            enum_key(s.render_quality as u8, &["low", "medium", "high"]),
        );
        line(
            "fx",
            enum_key(s.effects_intensity as u8, &["low", "medium", "high"]),
        );
        line(
            "uiscale",
            enum_key(s.ui_scale as u8, &["small", "normal", "large"]),
        );
        line(
            "shake",
            enum_key(s.screen_shake as u8, &["off", "low", "full"]),
        );
        line("motion.reduced", (s.reduced_motion as u8).to_string());
        line("contrast.high", (s.high_contrast as u8).to_string());
        line(
            "flash",
            enum_key(s.flash_intensity as u8, &["off", "low", "full"]),
        );
        line("text", enum_key(s.text_size as u8, &["normal", "large"]));
        line(
            "colors",
            enum_key(s.color_distinction as u8, &["standard", "enhanced"]),
        );
        line("team.player", self.last_player_team.0.to_string());
        line("team.opponent", self.last_opponent_team.0.to_string());
        line("settings.category", self.last_category.index().to_string());
        line("controls.profile", self.control_profile.0.to_string());
        self.bindings.encode_lines(&mut out);
        out
    }

    /// Decode any supported profile text. Version `1` is current; a missing
    /// or `0` version is the legacy shape and migrates by salvaging every
    /// known key. Unknown keys and invalid values fall back per-field.
    pub fn decode(text: &str) -> FrontendProfile {
        let mut profile = FrontendProfile::default();
        let mut teams = (profile.last_player_team, profile.last_opponent_team);
        for raw in text.lines().take(256) {
            let Some((key, value)) = raw.split_once('=') else {
                continue;
            };
            let (key, value) = (key.trim(), value.trim());
            let s = &mut profile.settings;
            match key {
                "v" => {}
                "difficulty" => {
                    s.difficulty = pick(
                        value,
                        &[
                            ("rookie", Difficulty::Rookie),
                            ("pro", Difficulty::Pro),
                            ("allstar", Difficulty::AllStar),
                        ],
                        s.difficulty,
                    )
                }
                "speed" => {
                    s.game_speed = pick(
                        value,
                        &[
                            ("normal", GameSpeed::Normal),
                            ("fast", GameSpeed::Fast),
                            ("turbo", GameSpeed::Turbo),
                        ],
                        s.game_speed,
                    )
                }
                "camera" => {
                    s.camera_style = pick(
                        value,
                        &[
                            ("arcade", CameraStyle::Arcade),
                            ("wide", CameraStyle::Wide),
                            ("close", CameraStyle::Close),
                        ],
                        s.camera_style,
                    )
                }
                "vol.master" => s.master_volume = parse_volume(value, s.master_volume),
                "vol.music" => s.music_volume = parse_volume(value, s.music_volume),
                "vol.effects" => s.effects_volume = parse_volume(value, s.effects_volume),
                "vol.crowd" => s.crowd_volume = parse_volume(value, s.crowd_volume),
                "vol.menu" => s.menu_volume = parse_volume(value, s.menu_volume),
                "mute.unfocused" => s.mute_when_unfocused = value == "1",
                "quality" => {
                    s.render_quality = pick(
                        value,
                        &[
                            ("low", RenderQuality::Low),
                            ("medium", RenderQuality::Medium),
                            ("high", RenderQuality::High),
                        ],
                        s.render_quality,
                    )
                }
                "fx" => {
                    s.effects_intensity = pick(
                        value,
                        &[
                            ("low", EffectsIntensity::Low),
                            ("medium", EffectsIntensity::Medium),
                            ("high", EffectsIntensity::High),
                        ],
                        s.effects_intensity,
                    )
                }
                "uiscale" => {
                    s.ui_scale = pick(
                        value,
                        &[
                            ("small", UiScale::Small),
                            ("normal", UiScale::Normal),
                            ("large", UiScale::Large),
                        ],
                        s.ui_scale,
                    )
                }
                "shake" => {
                    s.screen_shake = pick(
                        value,
                        &[
                            ("off", ScreenShake::Off),
                            ("low", ScreenShake::Low),
                            ("full", ScreenShake::Full),
                        ],
                        s.screen_shake,
                    )
                }
                "motion.reduced" => s.reduced_motion = value == "1",
                "contrast.high" => s.high_contrast = value == "1",
                "flash" => {
                    s.flash_intensity = pick(
                        value,
                        &[
                            ("off", FlashIntensity::Off),
                            ("low", FlashIntensity::Low),
                            ("full", FlashIntensity::Full),
                        ],
                        s.flash_intensity,
                    )
                }
                "text" => {
                    s.text_size = pick(
                        value,
                        &[("normal", TextSize::Normal), ("large", TextSize::Large)],
                        s.text_size,
                    )
                }
                "colors" => {
                    s.color_distinction = pick(
                        value,
                        &[
                            ("standard", ColorDistinction::Standard),
                            ("enhanced", ColorDistinction::Enhanced),
                        ],
                        s.color_distinction,
                    )
                }
                "team.player" => teams.0 = parse_team(value, teams.0),
                "team.opponent" => teams.1 = parse_team(value, teams.1),
                "settings.category" => {
                    profile.last_category = value
                        .parse::<usize>()
                        .ok()
                        .and_then(|i| SettingsCategory::ALL.get(i).copied())
                        .unwrap_or(profile.last_category)
                }
                "controls.profile" => {
                    profile.control_profile = value
                        .parse::<u8>()
                        .ok()
                        .filter(|p| *p < CONTROL_PROFILE_COUNT)
                        .map(ControlProfileId)
                        .unwrap_or(profile.control_profile)
                }
                _ => {
                    if let Some(bind_key) = key.strip_prefix("bind.") {
                        profile.bindings.decode_line(bind_key, value);
                    }
                }
            }
        }
        // Selected teams must differ; a corrupt pair falls back to defaults.
        if teams.0 == teams.1 {
            teams = (LeagueTeamId(0), LeagueTeamId(1));
        }
        profile.last_player_team = teams.0;
        profile.last_opponent_team = teams.1;
        profile.settings = profile.settings.sanitized();
        profile
    }
}

fn enum_key(index: u8, keys: &[&str]) -> String {
    keys.get(usize::from(index)).unwrap_or(&keys[0]).to_string()
}

fn parse_volume(value: &str, fallback: Volume) -> Volume {
    value
        .parse::<u8>()
        .ok()
        .map(Volume::clamped)
        .unwrap_or(fallback)
}

fn parse_team(value: &str, fallback: LeagueTeamId) -> LeagueTeamId {
    value
        .parse::<u8>()
        .ok()
        .filter(|t| usize::from(*t) < LEAGUE_SIZE)
        .map(LeagueTeamId)
        .unwrap_or(fallback)
}
