//! The six-team league: identity validity, uniqueness, palette-driven
//! branding, ratings inside bounds, distinct data-driven strengths (no team
//! branches), and the no-duplicate-selection rule.

use axiom_end_zone::data::player::{roster_for, RosterSide};
use axiom_end_zone::data::team::{league, league_team, LeagueTeamId, LEAGUE_SIZE, MAX_RATING};
use axiom_end_zone::frontend::input::FrontendInputFrame;
use axiom_end_zone::frontend::persistence::FrontendProfile;
use axiom_end_zone::frontend::state::{Screen, TeamStage};
use axiom_end_zone::frontend::FrontendApp;

#[test]
fn the_league_has_six_complete_original_teams() {
    let teams = league();
    assert_eq!(teams.len(), LEAGUE_SIZE);
    for (index, team) in teams.iter().enumerate() {
        assert_eq!(usize::from(team.league_id.0), index);
        assert!(!team.city.is_empty());
        assert!(!team.name.is_empty());
        assert!(
            (2..=4).contains(&team.abbreviation.len()),
            "abbreviation is a plate mark"
        );
        assert!(team.ratings.is_valid(), "{} ratings bounded", team.name);
        assert!(team.emblem.is_valid(), "{} emblem valid", team.name);
    }
}

#[test]
fn team_identity_fields_are_unique_across_the_league() {
    let teams = league();
    for a in 0..teams.len() {
        for b in (a + 1)..teams.len() {
            assert_ne!(teams[a].name, teams[b].name);
            assert_ne!(teams[a].city, teams[b].city);
            assert_ne!(teams[a].abbreviation, teams[b].abbreviation);
        }
    }
}

#[test]
fn ratings_stay_inside_the_declared_bounds() {
    for team in league() {
        for value in [
            team.ratings.power,
            team.ratings.speed,
            team.ratings.pass,
            team.ratings.defense,
        ] {
            assert!((1..=MAX_RATING).contains(&value));
        }
    }
}

#[test]
fn every_team_has_a_distinct_strength_profile() {
    let teams = league();
    for a in 0..teams.len() {
        for b in (a + 1)..teams.len() {
            let ra = teams[a].ratings;
            let rb = teams[b].ratings;
            assert_ne!(
                (ra.power, ra.speed, ra.pass, ra.defense),
                (rb.power, rb.speed, rb.pass, rb.defense),
                "{} vs {}",
                teams[a].name,
                teams[b].name
            );
        }
    }
}

#[test]
fn ratings_scale_rosters_through_data_not_branches() {
    let teams = league();
    // VOLTAGE (speed 10) outruns ANVILS (speed 4) at the same archetype slot.
    let fast = roster_for(teams[3], 0, RosterSide::Offense);
    let heavy = roster_for(teams[2], 0, RosterSide::Offense);
    assert!(fast.players[4].archetype.max_speed > heavy.players[4].archetype.max_speed);
    // ANVILS (power 10) blocks harder and carries more mass.
    assert!(heavy.players[1].archetype.mass > fast.players[1].archetype.mass);
    // FROSTBITE (defense 9) reacts faster than TEMPEST (defense 4).
    let stout = roster_for(teams[1], 0, RosterSide::Defense);
    let soft = roster_for(teams[4], 0, RosterSide::Defense);
    assert!(
        stout.players[6].archetype.reaction_delay_ticks
            < soft.players[6].archetype.reaction_delay_ticks
    );
}

#[test]
fn league_lookup_is_total() {
    for id in 0..(LEAGUE_SIZE as u8) {
        assert_eq!(league_team(LeagueTeamId(id)).league_id, LeagueTeamId(id));
    }
    // Out-of-range ids wrap instead of panicking.
    let _ = league_team(LeagueTeamId(200));
}

fn tap(fe: &mut FrontendApp, token: &str) {
    let input = FrontendInputFrame {
        keys_down: vec![token.to_string()],
        ..Default::default()
    };
    fe.frame(&input, 1280.0, 720.0);
    fe.frame(&FrontendInputFrame::default(), 1280.0, 720.0);
}

#[test]
fn the_opponent_cursor_can_never_select_the_locked_player_team() {
    let mut fe = FrontendApp::new(11, FrontendProfile::default());
    tap(&mut fe, "Enter");
    tap(&mut fe, "Enter");
    tap(&mut fe, "Enter"); // lock player (team 0)
    let player = fe.state().team_select.locked_player.expect("locked");
    assert_eq!(fe.state().team_select.stage, TeamStage::Opponent);
    // Walk the opponent carousel all the way around, both directions.
    for _ in 0..(LEAGUE_SIZE * 2) {
        tap(&mut fe, "ArrowRight");
        assert_ne!(fe.state().team_select.opponent_cursor, player.0);
    }
    for _ in 0..(LEAGUE_SIZE * 2) {
        tap(&mut fe, "ArrowLeft");
        assert_ne!(fe.state().team_select.opponent_cursor, player.0);
    }
    // Lock the opponent: the launch pair differs by construction.
    tap(&mut fe, "Enter");
    assert_eq!(fe.state().screen, Screen::MatchSetup);
    let launch = fe.state().build_launch().expect("valid launch");
    assert_ne!(launch.player_team, launch.opponent_team);
}

#[test]
fn selections_are_remembered_into_the_profile() {
    let mut fe = FrontendApp::new(11, FrontendProfile::default());
    tap(&mut fe, "Enter");
    tap(&mut fe, "Enter");
    tap(&mut fe, "ArrowRight"); // player team 1
    tap(&mut fe, "Enter");
    tap(&mut fe, "ArrowRight");
    tap(&mut fe, "Enter"); // opponent locked
    assert_eq!(fe.state().profile.last_player_team, LeagueTeamId(1));
    assert_ne!(
        fe.state().profile.last_opponent_team,
        fe.state().profile.last_player_team
    );
}
