//! The launch boundary: validation, difficulty/camera/effects profiles as
//! real data, deterministic game-speed pacing, and byte-equal match
//! reproduction from one frozen `MatchLaunchConfig`.

use axiom_end_zone::data::team::LeagueTeamId;
use axiom_end_zone::launch::{
    camera_profile, difficulty_profile, juice_profile, resolve_launch, CameraStyle, Difficulty,
    EffectsIntensity, FlashIntensity, GameSpeed, LaunchError, MatchLaunchConfig, ScreenShake,
};
use axiom_end_zone::showcase::ShowcaseRun;
use axiom_end_zone::state::SimState;

fn launch(player: u8, opponent: u8) -> MatchLaunchConfig {
    MatchLaunchConfig {
        player_team: LeagueTeamId(player),
        opponent_team: LeagueTeamId(opponent),
        player_is_home: true,
        field: Default::default(),
        difficulty: Difficulty::Pro,
        game_speed: GameSpeed::Normal,
        camera_style: CameraStyle::Arcade,
        seed: 0xA11CE,
        presentation: Default::default(),
        control_profile: Default::default(),
    }
}

#[test]
fn validation_rejects_every_illegal_configuration() {
    assert_eq!(launch(2, 2).validate(), Err(LaunchError::SameTeams));
    assert_eq!(launch(9, 1).validate(), Err(LaunchError::UnknownPlayerTeam));
    assert_eq!(
        launch(0, 9).validate(),
        Err(LaunchError::UnknownOpponentTeam)
    );
    let mut bad_profile = launch(0, 1);
    bad_profile.control_profile = axiom_end_zone::launch::ControlProfileId(7);
    assert_eq!(
        bad_profile.validate(),
        Err(LaunchError::UnknownControlProfile)
    );
    assert_eq!(launch(0, 1).validate(), Ok(()));
}

#[test]
fn game_speed_pacing_is_a_pure_function_of_the_frame() {
    for frame in 0..12 {
        assert_eq!(GameSpeed::Normal.steps_for_frame(frame), 1);
        assert_eq!(GameSpeed::Turbo.steps_for_frame(frame), 2);
        let fast = GameSpeed::Fast.steps_for_frame(frame);
        assert_eq!(fast, if frame % 2 == 0 { 2 } else { 1 });
    }
}

#[test]
fn difficulty_profiles_order_correctly() {
    let rookie = difficulty_profile(Difficulty::Rookie);
    let pro = difficulty_profile(Difficulty::Pro);
    let allstar = difficulty_profile(Difficulty::AllStar);
    assert!(rookie.reaction_delay_scale > pro.reaction_delay_scale);
    assert!(pro.reaction_delay_scale > allstar.reaction_delay_scale);
    assert!(rookie.pursuit_scale < allstar.pursuit_scale);
    assert!(rookie.tackle_range_scale < allstar.tackle_range_scale);
    // Pro IS the showcase default (scale 1.0 everywhere).
    assert_eq!(pro.reaction_delay_scale, 1.0);
    assert_eq!(pro.pursuit_scale, 1.0);
    assert_eq!(pro.tackle_range_scale, 1.0);
}

#[test]
fn difficulty_reshapes_the_opponent_data_not_code() {
    let easy = resolve_launch(&MatchLaunchConfig {
        difficulty: Difficulty::Rookie,
        ..launch(0, 1)
    });
    let hard = resolve_launch(&MatchLaunchConfig {
        difficulty: Difficulty::AllStar,
        ..launch(0, 1)
    });
    // Same teams, same archetypes — only the numbers differ.
    for slot in 0..7 {
        let e = easy.rosters.1.players[slot].archetype;
        let h = hard.rosters.1.players[slot].archetype;
        assert!(e.reaction_delay_ticks >= h.reaction_delay_ticks);
        assert!(e.pursuit_aggressiveness <= h.pursuit_aggressiveness);
    }
    assert!(easy.tuning.tackle_range < hard.tuning.tackle_range);
    // The player's own offense is untouched by difficulty.
    assert_eq!(easy.rosters.0, hard.rosters.0);
}

#[test]
fn accessibility_scales_are_real_presentation_data() {
    assert_eq!(
        camera_profile(CameraStyle::Arcade, ScreenShake::Off).shake_scale,
        0.0
    );
    assert_eq!(
        camera_profile(CameraStyle::Arcade, ScreenShake::Full).shake_scale,
        1.0
    );
    assert_eq!(
        juice_profile(EffectsIntensity::Medium, FlashIntensity::Off).flash_scale,
        0.0
    );
    let low = juice_profile(EffectsIntensity::Low, FlashIntensity::Full);
    let high = juice_profile(EffectsIntensity::High, FlashIntensity::Full);
    assert!(low.dust_particles < high.dust_particles);
    assert!(low.streak_count < high.streak_count);
}

#[test]
fn camera_styles_are_distinct_named_tunings() {
    let arcade = camera_profile(CameraStyle::Arcade, ScreenShake::Full);
    let wide = camera_profile(CameraStyle::Wide, ScreenShake::Full);
    let close = camera_profile(CameraStyle::Close, ScreenShake::Full);
    assert!(wide.follow_distance > arcade.follow_distance);
    assert!(close.follow_distance < arcade.follow_distance);
    assert!(wide.base_fov_degrees > close.base_fov_degrees);
}

#[test]
fn the_same_launch_config_reproduces_the_same_match() {
    let config = launch(3, 5);
    let digest_of = |mut run: ShowcaseRun| {
        for _ in 0..240 {
            let _ = run.step(&[]);
        }
        run.sim.digest()
    };
    let a = digest_of(ShowcaseRun::new_match(&config));
    let b = digest_of(ShowcaseRun::new_match(&config));
    assert_eq!(a, b, "same config, same seed, same authoritative state");
}

#[test]
fn different_seeds_produce_valid_but_different_presentation_seeds() {
    let mut a = launch(3, 5);
    let mut b = launch(3, 5);
    a.seed = 1;
    b.seed = 2;
    // Both boot fine; initial authoritative formation is seed-independent.
    let run_a = ShowcaseRun::new_match(&a);
    let run_b = ShowcaseRun::new_match(&b);
    assert_eq!(run_a.sim.digest(), run_b.sim.digest());
}

#[test]
fn restart_reproduces_the_initial_state_exactly() {
    let config = launch(2, 4);
    let fresh = SimState::new_match(&resolve_launch(&config)).digest();
    let mut run = ShowcaseRun::new_match(&config);
    for _ in 0..400 {
        let _ = run.step(&[]);
    }
    assert_ne!(run.sim.digest(), fresh, "the match actually advanced");
    let restarted = ShowcaseRun::new_match(&config);
    assert_eq!(restarted.sim.digest(), fresh);
}

#[test]
fn selected_teams_shape_the_rosters() {
    let setup = resolve_launch(&launch(3, 5));
    assert_eq!(setup.rosters.0.team.league_id, LeagueTeamId(3));
    assert_eq!(setup.rosters.1.team.league_id, LeagueTeamId(5));
    // Sim side slots are the possession slots, not league ids.
    assert_eq!(setup.rosters.0.team.id.0, 0);
    assert_eq!(setup.rosters.1.team.id.0, 1);
}
