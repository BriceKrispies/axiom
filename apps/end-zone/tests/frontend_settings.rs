//! The four settings and their compact persistence: valid defaults, a bounded
//! volume, screen-shake driving real camera amplitude, reduced motion
//! suppressing nonessential movement, a persistence round-trip, safe fallback
//! on malformed input, and the absence of every removed setting from the shape.

use axiom_end_zone::frontend::persistence::{decode, encode};
use axiom_end_zone::frontend::settings::{EndZoneSettings, Volume};
use axiom_end_zone::launch::{camera_tuning, juice_tuning, RunConfig, ScreenShake};

#[test]
fn defaults_are_valid() {
    let d = EndZoneSettings::default();
    assert_eq!(d.sanitized(), d);
    assert!(d.master_volume.0 <= Volume::MAX);
    assert!((0.0..=1.0).contains(&d.master_volume.ratio()));
}

#[test]
fn master_volume_is_bounded() {
    assert_eq!(Volume::clamped(255).0, Volume::MAX);
    assert_eq!(Volume(Volume::MAX).step(true).0, Volume::MAX);
    assert_eq!(Volume(0).step(false).0, 0);
    assert!((Volume::clamped(255).ratio() - 1.0).abs() < 1e-6);
}

#[test]
fn screen_shake_off_produces_zero_camera_amplitude() {
    assert_eq!(ScreenShake::Off.scale(), 0.0);
    let config = RunConfig::new(1).with_presentation(ScreenShake::Off, false);
    assert_eq!(camera_tuning(&config).shake_scale, 0.0);
}

#[test]
fn screen_shake_low_scales_camera_amplitude() {
    assert_eq!(ScreenShake::Low.scale(), 0.5);
    let config = RunConfig::new(1).with_presentation(ScreenShake::Low, false);
    assert_eq!(camera_tuning(&config).shake_scale, 0.5);
    let full = RunConfig::new(1).with_presentation(ScreenShake::Full, false);
    assert!(camera_tuning(&full).shake_scale > camera_tuning(&config).shake_scale);
}

#[test]
fn reduced_motion_suppresses_nonessential_movement() {
    // Reduced motion drops the nonessential flash juice to zero.
    let config = RunConfig::new(1).with_presentation(ScreenShake::Full, true);
    assert_eq!(juice_tuning(&config).flash_scale, 0.0);
}

#[test]
fn settings_round_trip_through_compact_persistence() {
    let settings = EndZoneSettings {
        master_volume: Volume(3),
        music_volume: Volume(6),
        screen_shake: ScreenShake::Off,
        reduced_motion: true,
    };
    assert_eq!(decode(&encode(&settings)), settings);
}

#[test]
fn music_volume_is_bounded_and_defaults_reasonably() {
    let d = EndZoneSettings::default();
    assert!(d.music_volume.0 <= Volume::MAX);
    assert!((0.0..=1.0).contains(&d.music_volume.ratio()));
    assert_eq!(Volume(Volume::MAX).step(true).0, Volume::MAX);
}

#[test]
fn an_old_profile_without_music_volume_loads_the_default() {
    // A profile persisted before MUSIC VOLUME existed: it must load with the
    // default music volume rather than failing or zeroing it.
    let legacy = "endzone.v=1\nmaster_volume=4\nscreen_shake=low\nreduced_motion=0\n";
    let decoded = decode(legacy);
    assert_eq!(decoded.music_volume, EndZoneSettings::default().music_volume);
    assert_eq!(decoded.master_volume, Volume(4));
}

#[test]
fn malformed_persisted_settings_fall_back_safely() {
    let text = "\u{0}garbage\nmaster_volume=abc\nscreen_shake=purple\nreduced_motion=maybe\n";
    assert_eq!(decode(text), EndZoneSettings::default());
    // An empty blob is safe too.
    assert_eq!(decode(""), EndZoneSettings::default());
}

#[test]
fn no_removed_setting_appears_in_the_persisted_shape() {
    let text = encode(&EndZoneSettings::default());
    for key in &["master_volume", "music_volume", "screen_shake", "reduced_motion"] {
        assert!(text.contains(key), "keeps {key}");
    }
    for removed in &[
        "difficulty",
        "camera",
        "game_speed",
        "crowd",
        "quality",
        "contrast",
        "ui_scale",
        "text_size",
        "team",
        "bind",
    ] {
        assert!(!text.contains(removed), "drops {removed}");
    }
}
