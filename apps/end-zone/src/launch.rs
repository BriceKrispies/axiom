//! The frontend → simulation launch boundary: one immutable
//! [`MatchLaunchConfig`] produced by the frontend when a match is confirmed,
//! validated here, and resolved into the sim-facing [`MatchSetup`] bundle the
//! existing showcase bootstrap consumes. The simulation never reads frontend
//! state; the frontend never mutates a running simulation.

use crate::config::PLAYERS_PER_TEAM;
use crate::data::player::{roster_for, RosterDefinition, RosterSide};
use crate::data::team::{league_team, LeagueTeamId, LEAGUE_SIZE};
use crate::data::{BehaviorTuning, CameraTuning, JuiceTuning};
use crate::identity::TeamId;

/// Opponent strength profile (named AI tuning profiles).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Difficulty {
    Rookie,
    #[default]
    Pro,
    AllStar,
}

/// Deterministic simulation pacing: whole sim steps per animation frame in a
/// fixed repeating pattern — the fixed-step duration itself never changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GameSpeed {
    #[default]
    Normal,
    Fast,
    Turbo,
}

impl GameSpeed {
    /// Sim steps to run on `frame` (Normal `1`, Fast alternates `2,1` for
    /// 1.5×, Turbo `2`). Pure function of the frame index — replayable.
    pub fn steps_for_frame(self, frame: u64) -> u32 {
        match self {
            GameSpeed::Normal => 1,
            GameSpeed::Fast => 1 + u32::from(frame % 2 == 0),
            GameSpeed::Turbo => 2,
        }
    }
}

/// Named camera tuning profiles (one implementation, three tunings).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CameraStyle {
    #[default]
    Arcade,
    Wide,
    Close,
}

/// Presentation-effect intensity (particles, trails, rings, wobble, squash).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EffectsIntensity {
    Low,
    #[default]
    Medium,
    High,
}

/// Camera impulse (shake) scaling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScreenShake {
    Off,
    Low,
    #[default]
    Full,
}

impl ScreenShake {
    pub fn scale(self) -> f32 {
        match self {
            ScreenShake::Off => 0.0,
            ScreenShake::Low => 0.5,
            ScreenShake::Full => 1.0,
        }
    }
}

/// Flash / bright-transition intensity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FlashIntensity {
    Off,
    Low,
    #[default]
    Full,
}

impl FlashIntensity {
    pub fn scale(self) -> f32 {
        match self {
            FlashIntensity::Off => 0.0,
            FlashIntensity::Low => 0.5,
            FlashIntensity::Full => 1.0,
        }
    }
}

/// The presentation slice of a launch (never touches authoritative state).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PresentationProfile {
    pub effects: EffectsIntensity,
    pub screen_shake: ScreenShake,
    pub flash: FlashIntensity,
}

/// The arena/field presentation identifier (one arena exists today; the
/// identifier is the boundary future arenas plug into).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FieldPresentation {
    #[default]
    Standard,
}

/// A control profile identity (profile 0 is the built-in default).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ControlProfileId(pub u8);

/// How many control profiles exist.
pub const CONTROL_PROFILE_COUNT: u8 = 1;

/// The immutable match launch configuration — frozen by the frontend at
/// `START MATCH`, consumed once by the bootstrap.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MatchLaunchConfig {
    pub player_team: LeagueTeamId,
    pub opponent_team: LeagueTeamId,
    pub player_is_home: bool,
    pub field: FieldPresentation,
    pub difficulty: Difficulty,
    pub game_speed: GameSpeed,
    pub camera_style: CameraStyle,
    pub seed: u64,
    pub presentation: PresentationProfile,
    pub control_profile: ControlProfileId,
}

/// Why a launch configuration was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchError {
    SameTeams,
    UnknownPlayerTeam,
    UnknownOpponentTeam,
    UnknownControlProfile,
}

impl MatchLaunchConfig {
    /// Validate every cross-field rule the frontend must have enforced.
    pub fn validate(&self) -> Result<(), LaunchError> {
        if usize::from(self.player_team.0) >= LEAGUE_SIZE {
            return Err(LaunchError::UnknownPlayerTeam);
        }
        if usize::from(self.opponent_team.0) >= LEAGUE_SIZE {
            return Err(LaunchError::UnknownOpponentTeam);
        }
        if self.player_team == self.opponent_team {
            return Err(LaunchError::SameTeams);
        }
        if self.control_profile.0 >= CONTROL_PROFILE_COUNT {
            return Err(LaunchError::UnknownControlProfile);
        }
        Ok(())
    }
}

/// The named difficulty profiles: pure scaling of the opponent's defensive
/// data + the shared contact tuning. `Pro` is exactly the showcase default.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DifficultyProfile {
    pub reaction_delay_scale: f32,
    pub pursuit_scale: f32,
    pub tackle_range_scale: f32,
}

pub fn difficulty_profile(difficulty: Difficulty) -> DifficultyProfile {
    match difficulty {
        Difficulty::Rookie => DifficultyProfile {
            reaction_delay_scale: 1.6,
            pursuit_scale: 0.8,
            tackle_range_scale: 0.9,
        },
        Difficulty::Pro => DifficultyProfile {
            reaction_delay_scale: 1.0,
            pursuit_scale: 1.0,
            tackle_range_scale: 1.0,
        },
        Difficulty::AllStar => DifficultyProfile {
            reaction_delay_scale: 0.7,
            pursuit_scale: 1.15,
            tackle_range_scale: 1.1,
        },
    }
}

/// The named camera profiles. `Arcade` is exactly the showcase default.
pub fn camera_profile(style: CameraStyle, shake: ScreenShake) -> CameraTuning {
    let mut tuning = CameraTuning::default();
    match style {
        CameraStyle::Arcade => {}
        CameraStyle::Wide => {
            tuning.follow_distance = 13.0;
            tuning.follow_height = 6.8;
            tuning.base_fov_degrees = 66.0;
            tuning.formation_distance = 22.0;
            tuning.formation_height = 12.0;
        }
        CameraStyle::Close => {
            tuning.follow_distance = 6.2;
            tuning.follow_height = 3.0;
            tuning.base_fov_degrees = 52.0;
            tuning.formation_distance = 13.0;
            tuning.formation_height = 6.5;
        }
    }
    tuning.shake_scale = shake.scale();
    tuning
}

/// The named presentation-effect profiles. `Medium`/`Full` is exactly the
/// showcase default.
pub fn juice_profile(effects: EffectsIntensity, flash: FlashIntensity) -> JuiceTuning {
    let mut tuning = JuiceTuning::default();
    match effects {
        EffectsIntensity::Low => {
            tuning.dust_particles = 5;
            tuning.streak_count = 3;
            tuning.trail_points = 8;
            tuning.dust_radius *= 0.7;
            tuning.ring_radius *= 0.7;
            tuning.squash_amplitude *= 0.7;
            tuning.field_wobble_amplitude *= 0.6;
        }
        EffectsIntensity::Medium => {}
        EffectsIntensity::High => {
            tuning.dust_particles = 14;
            tuning.streak_count = 8;
            tuning.trail_points = 18;
            tuning.dust_radius *= 1.25;
            tuning.ring_radius *= 1.25;
            tuning.squash_amplitude *= 1.15;
            tuning.field_wobble_amplitude *= 1.2;
        }
    }
    tuning.flash_scale = flash.scale();
    tuning
}

/// The sim-facing resolved bundle: rosters in sim slots (player = possession
/// slot 0, opponent = slot 1), contact tuning, and the deterministic seed.
#[derive(Debug, Clone, PartialEq)]
pub struct MatchSetup {
    pub rosters: (RosterDefinition, RosterDefinition),
    pub tuning: BehaviorTuning,
    pub seed: u64,
}

/// Resolve a validated launch into the sim bundle: build both rosters from
/// the league data and apply the difficulty profile to the OPPONENT's
/// defensive archetypes and the shared tackle range.
pub fn resolve_launch(launch: &MatchLaunchConfig) -> MatchSetup {
    let player = league_team(launch.player_team).with_sim_slot(TeamId(0));
    let opponent = league_team(launch.opponent_team).with_sim_slot(TeamId(1));
    let offense = roster_for(player, 0, RosterSide::Offense);
    let mut defense = roster_for(opponent, PLAYERS_PER_TEAM as u8, RosterSide::Defense);

    let profile = difficulty_profile(launch.difficulty);
    for player in defense.players.iter_mut() {
        let a = &mut player.archetype;
        a.reaction_delay_ticks =
            ((a.reaction_delay_ticks as f32 * profile.reaction_delay_scale).round() as u32).max(1);
        a.pursuit_aggressiveness = (a.pursuit_aggressiveness * profile.pursuit_scale).min(1.0);
    }
    let mut tuning = BehaviorTuning::default();
    tuning.tackle_range *= profile.tackle_range_scale;

    MatchSetup {
        rosters: (offense, defense),
        tuning,
        seed: launch.seed,
    }
}
