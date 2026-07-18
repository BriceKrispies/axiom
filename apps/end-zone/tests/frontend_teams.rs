//! The two fixed teams: exactly one offense and one defense, distinct, always
//! used by the run bootstrap, with no user-facing team selection.

use axiom_end_zone::data::team::{
    frostbite, league, magma, DEFENSE_TEAM, LEAGUE_SIZE, OFFENSE_TEAM,
};
use axiom_end_zone::launch::RunConfig;
use axiom_end_zone::showcase::ShowcaseRun;

#[test]
fn exactly_two_fixed_teams_exist() {
    assert_eq!(LEAGUE_SIZE, 2);
    assert_eq!(league().len(), 2);
}

#[test]
fn the_offense_and_defense_are_distinct_and_valid() {
    assert_ne!(OFFENSE_TEAM, DEFENSE_TEAM);
    assert_ne!(magma().name, frostbite().name);
    assert_ne!(magma().league_id, frostbite().league_id);
    assert!(magma().ratings.is_valid());
    assert!(frostbite().ratings.is_valid());
}

#[test]
fn run_config_fixes_the_matchup() {
    let config = RunConfig::new(9);
    assert_eq!(config.offense, OFFENSE_TEAM);
    assert_eq!(config.defense, DEFENSE_TEAM);
}

#[test]
fn the_run_bootstrap_always_uses_the_fixed_teams() {
    let run = ShowcaseRun::new_run(&RunConfig::new(0xABC));
    assert_eq!(run.sim.rosters.0.team.league_id, OFFENSE_TEAM);
    assert_eq!(run.sim.rosters.1.team.league_id, DEFENSE_TEAM);
}

#[test]
fn the_teams_are_pure_data_the_ai_reads() {
    // Team strength lives entirely in the ratings/palette data — distinct
    // numbers the generic systems scale, never a team branch in code.
    assert_ne!(magma().ratings, frostbite().ratings);
    assert_ne!(magma().palette.primary(), frostbite().palette.primary());
}
