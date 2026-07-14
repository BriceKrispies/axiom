//! Persistence: versioned encode/decode round-trips, per-field fallback on
//! corrupt input, the distinct-teams invariant, the store abstraction, and
//! never-panicking loads.

use axiom_end_zone::data::team::LeagueTeamId;
use axiom_end_zone::frontend::bindings::BindableAction;
use axiom_end_zone::frontend::persistence::{
    FrontendProfile, MemoryStore, ProfileStore, PROFILE_VERSION,
};
use axiom_end_zone::frontend::settings::{SettingsCategory, TextSize, UiScale, Volume};
use axiom_end_zone::launch::{Difficulty, FlashIntensity, GameSpeed, ScreenShake};
use axiom_kernel::{LogRecord, LogSink};

#[derive(Default)]
struct TestSink(Vec<LogRecord>);

impl LogSink for TestSink {
    fn record(&mut self, record: LogRecord) {
        self.0.push(record);
    }
}

fn custom_profile() -> FrontendProfile {
    let mut profile = FrontendProfile::default();
    profile.settings.difficulty = Difficulty::AllStar;
    profile.settings.game_speed = GameSpeed::Turbo;
    profile.settings.master_volume = Volume(3);
    profile.settings.mute_when_unfocused = false;
    profile.settings.ui_scale = UiScale::Large;
    profile.settings.screen_shake = ScreenShake::Off;
    profile.settings.reduced_motion = true;
    profile.settings.high_contrast = true;
    profile.settings.flash_intensity = FlashIntensity::Low;
    profile.settings.text_size = TextSize::Large;
    profile.bindings.rebind(BindableAction::Pause, "KeyO");
    profile.last_player_team = LeagueTeamId(4);
    profile.last_opponent_team = LeagueTeamId(2);
    profile.last_category = SettingsCategory::Accessibility;
    profile
}

#[test]
fn encode_decode_round_trips_every_field() {
    let profile = custom_profile();
    let text = profile.encode();
    assert!(text.starts_with(&format!("v={PROFILE_VERSION}\n")));
    let decoded = FrontendProfile::decode(&text);
    assert_eq!(decoded, profile);
}

#[test]
fn unknown_keys_and_garbage_lines_are_ignored() {
    let mut text = custom_profile().encode();
    text.push_str("mystery.key=42\nnot a key value line\n===\n");
    assert_eq!(FrontendProfile::decode(&text), custom_profile());
}

#[test]
fn corrupt_values_fall_back_per_field_not_wholesale() {
    let decoded = FrontendProfile::decode(
        "v=1\ndifficulty=impossible\nspeed=turbo\nvol.master=banana\nvol.menu=99\n\
         shake=off\nteam.player=200\n",
    );
    let defaults = FrontendProfile::default();
    // The corrupt fields fall back...
    assert_eq!(decoded.settings.difficulty, defaults.settings.difficulty);
    assert_eq!(
        decoded.settings.master_volume,
        defaults.settings.master_volume
    );
    assert_eq!(decoded.last_player_team, defaults.last_player_team);
    // ...while the valid neighbors survive.
    assert_eq!(decoded.settings.game_speed, GameSpeed::Turbo);
    assert_eq!(decoded.settings.screen_shake, ScreenShake::Off);
    // Out-of-range volumes clamp rather than reset.
    assert_eq!(decoded.settings.menu_volume, Volume(10));
}

#[test]
fn equal_persisted_teams_fall_back_to_a_legal_pair() {
    let decoded = FrontendProfile::decode("v=1\nteam.player=3\nteam.opponent=3\n");
    assert_ne!(decoded.last_player_team, decoded.last_opponent_team);
}

#[test]
fn empty_or_hostile_text_yields_usable_defaults() {
    for text in ["", "v=999", "\0\0\0", "=====\n=\n=", "v=1\nbind.confirm=\n"] {
        let decoded = FrontendProfile::decode(text);
        assert_eq!(decoded.settings, FrontendProfile::default().settings);
    }
}

#[test]
fn binding_lines_round_trip_and_reject_junk_tokens() {
    let profile = custom_profile();
    let decoded = FrontendProfile::decode(&profile.encode());
    assert_eq!(decoded.bindings.tokens(BindableAction::Pause)[0], "KeyO");
    // Overlong / non-graphic tokens are dropped; the field keeps defaults.
    let hostile = format!("v=1\nbind.pause={}\n", "X".repeat(60));
    let decoded = FrontendProfile::decode(&hostile);
    assert_eq!(decoded.bindings.tokens(BindableAction::Pause)[0], "KeyP");
}

#[test]
fn the_memory_store_load_save_clear_cycle_works() {
    let mut store = MemoryStore::default();
    let mut sink = TestSink::default();
    assert!(store.load().is_none());

    // A missing profile logs and yields defaults — never panics.
    let loaded = FrontendProfile::load_from(&store, &mut sink);
    assert_eq!(loaded, FrontendProfile::default());
    assert!(!sink.0.is_empty(), "the miss is logged");

    let profile = custom_profile();
    profile.save_to(&mut store, &mut sink);
    let loaded = FrontendProfile::load_from(&store, &mut sink);
    assert_eq!(loaded, profile);

    store.clear();
    assert!(store.load().is_none());
}

#[test]
fn a_legacy_versionless_profile_migrates_by_salvage() {
    // Version `0` / missing: every known key is salvaged, the rest defaults.
    let decoded = FrontendProfile::decode("difficulty=rookie\nvol.master=2\n");
    assert_eq!(decoded.settings.difficulty, Difficulty::Rookie);
    assert_eq!(decoded.settings.master_volume, Volume(2));
}
