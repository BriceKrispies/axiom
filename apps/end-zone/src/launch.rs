//! The run boundary: one immutable [`RunConfig`] the frontend freezes when the
//! player presses START, resolved here into the sim-facing [`RunSetup`] the
//! drive bootstrap consumes — once at launch and again at every heat step. The
//! simulation never reads frontend state; the frontend never mutates a running
//! run. There is no team selection, difficulty menu, or match setup: the
//! matchup is fixed (see [`crate::data::team`]) and the only escalating dial is
//! the automatic defensive heat.

use crate::config::PLAYERS_PER_TEAM;
use crate::data::player::{roster_for, RosterDefinition, RosterSide};
use crate::data::team::{league_team, LeagueTeamId, DEFENSE_TEAM, OFFENSE_TEAM};
use crate::data::{BehaviorTuning, CameraTuning, JuiceTuning};
use crate::identity::TeamId;

/// Camera-impulse (screen-shake) scaling — the one presentation preference
/// gameplay reads directly: it scales the actual gameplay camera impulses
/// (`Off` is exactly zero shake).
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

/// The highest defensive heat level the run escalates to.
pub const MAX_HEAT: u8 = 6;

/// The immutable run configuration — everything a deterministic score-attack
/// run needs, and nothing else. Frozen by the frontend at START, consumed once
/// by the drive bootstrap; restarting a run rebuilds from the same value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RunConfig {
    /// The explicit deterministic seed for this run.
    pub seed: u64,
    /// The fixed offensive team (always [`OFFENSE_TEAM`]).
    pub offense: LeagueTeamId,
    /// The fixed defensive team (always [`DEFENSE_TEAM`]).
    pub defense: LeagueTeamId,
    /// The heat the run begins at (`1..=MAX_HEAT`).
    pub initial_heat: u8,
    /// Screen-shake preference (scales gameplay camera impulses).
    pub screen_shake: ScreenShake,
    /// Reduced-motion preference (suppresses nonessential presentation motion).
    pub reduced_motion: bool,
}

impl RunConfig {
    /// The canonical run from `seed`: the two fixed teams, heat 1, full shake.
    pub fn new(seed: u64) -> Self {
        RunConfig {
            seed,
            offense: OFFENSE_TEAM,
            defense: DEFENSE_TEAM,
            initial_heat: 1,
            screen_shake: ScreenShake::Full,
            reduced_motion: false,
        }
    }

    /// The same run with the platform's presentation preferences applied.
    pub fn with_presentation(mut self, screen_shake: ScreenShake, reduced_motion: bool) -> Self {
        self.screen_shake = screen_shake;
        self.reduced_motion = reduced_motion;
        self
    }
}

impl Default for RunConfig {
    fn default() -> Self {
        RunConfig::new(crate::config::DEFAULT_SEED)
    }
}

/// The defensive tuning a heat level selects: a pure scaling of the opponent's
/// reaction/pursuit/tackle-range. Heat 1 is a beatable defense; each level up
/// tightens reactions and pursuit toward a swarming top level.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DefenseProfile {
    pub reaction_delay_scale: f32,
    pub pursuit_scale: f32,
    pub tackle_range_scale: f32,
}

/// The defensive profile for `heat` (clamped to `1..=MAX_HEAT`). Linear across
/// the heat band — heat 1 is generous, [`MAX_HEAT`] is relentless.
pub fn heat_profile(heat: u8) -> DefenseProfile {
    let clamped = heat.clamp(1, MAX_HEAT);
    let span = f32::from(MAX_HEAT - 1).max(1.0);
    let t = f32::from(clamped - 1) / span;
    DefenseProfile {
        reaction_delay_scale: 1.5 - 0.9 * t,
        pursuit_scale: 0.82 + 0.4 * t,
        tackle_range_scale: 0.88 + 0.4 * t,
    }
}

/// The camera tuning for the run: the default rig with the screen-shake and
/// reduced-motion preferences applied to the impulse scale.
pub fn camera_tuning(config: &RunConfig) -> CameraTuning {
    let mut tuning = CameraTuning::default();
    let reduced = if config.reduced_motion { 0.6 } else { 1.0 };
    tuning.shake_scale = config.screen_shake.scale() * reduced;
    tuning
}

/// The juice tuning for the run: reduced motion damps the nonessential flash
/// and field wobble while keeping gameplay-legible dust and rings.
pub fn juice_tuning(config: &RunConfig) -> JuiceTuning {
    let mut tuning = JuiceTuning::default();
    if config.reduced_motion {
        tuning.flash_scale = 0.0;
        tuning.field_wobble_amplitude *= 0.4;
    }
    tuning
}

/// The sim-facing resolved bundle for one heat level: rosters in sim slots
/// (offense = slot 0, defense = slot 1), the shared contact tuning, and the
/// deterministic seed.
#[derive(Debug, Clone, PartialEq)]
pub struct RunSetup {
    pub rosters: (RosterDefinition, RosterDefinition),
    pub tuning: BehaviorTuning,
    pub seed: u64,
}

/// Build the defense roster + shared tuning at `heat`: the opponent's defensive
/// archetypes and the shared tackle range scale up with heat.
pub fn resolve_defense(config: &RunConfig, heat: u8) -> (RosterDefinition, BehaviorTuning) {
    let opponent = league_team(config.defense).with_sim_slot(TeamId(1));
    let mut defense = roster_for(opponent, PLAYERS_PER_TEAM as u8, RosterSide::Defense);
    let profile = heat_profile(heat);
    for player in defense.players.iter_mut() {
        let a = &mut player.archetype;
        a.reaction_delay_ticks =
            ((a.reaction_delay_ticks as f32 * profile.reaction_delay_scale).round() as u32).max(1);
        a.pursuit_aggressiveness = (a.pursuit_aggressiveness * profile.pursuit_scale).min(1.0);
    }
    let mut tuning = BehaviorTuning::default();
    tuning.tackle_range *= profile.tackle_range_scale;
    (defense, tuning)
}

/// Resolve a run at `heat` into the sim bundle: the fixed offense, the
/// heat-scaled defense, and the shared contact tuning.
pub fn resolve_run(config: &RunConfig, heat: u8) -> RunSetup {
    let player = league_team(config.offense).with_sim_slot(TeamId(0));
    let offense = roster_for(player, 0, RosterSide::Offense);
    let (defense, tuning) = resolve_defense(config, heat);
    RunSetup {
        rosters: (offense, defense),
        tuning,
        seed: config.seed,
    }
}
