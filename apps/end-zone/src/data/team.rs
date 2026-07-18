//! Fictional team definitions: the two fixed END ZONE teams — CRATER CITY
//! MAGMA on offense, GLACIER FALLS FROSTBITE on defense. Both are original —
//! fictional city, original name, procedural emblem — with bounded ratings and
//! a palette consumed by the player-model construction and the end-zone paint.
//! No real-world league, team, or player branding appears anywhere, and
//! gameplay code contains zero team branches: everything a team IS lives in
//! this data. The score-attack game fixes the matchup (see `RunConfig`); there
//! is no team selection.

use crate::identity::TeamId;

/// A league team's stable identity (index into [`league`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LeagueTeamId(pub u8);

/// How many teams the league carries (the fixed offense/defense pair).
pub const LEAGUE_SIZE: usize = 2;

/// Rating ceiling (ratings are `1..=MAX_RATING`).
pub const MAX_RATING: u8 = 10;

/// Bounded team ratings — the ONLY strength vocabulary. Gameplay derives
/// archetype numbers from these by data scaling, never by team branches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TeamRatings {
    pub power: u8,
    pub speed: u8,
    pub pass: u8,
    pub defense: u8,
}

impl TeamRatings {
    /// Whether every rating sits inside `1..=MAX_RATING`.
    pub fn is_valid(&self) -> bool {
        [self.power, self.speed, self.pass, self.defense]
            .iter()
            .all(|r| (1..=MAX_RATING).contains(r))
    }
}

pub use super::emblem::{EmblemBase, EmblemDefinition, EmblemMotif};

/// Uniform + trim colors (linear RGB). One palette slot per model part tag —
/// player construction reads the palette and contains zero team branches.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TeamPalette {
    pub helmet: [f32; 3],
    pub facemask: [f32; 3],
    pub jersey: [f32; 3],
    pub pants: [f32; 3],
    pub skin: [f32; 3],
    pub shoes: [f32; 3],
    /// End-zone paint + accents.
    pub trim: [f32; 3],
}

impl TeamPalette {
    /// The palette as part-tag-indexed slots, in the model's tag order:
    /// helmet, facemask, jersey, pants, skin, shoes, trim.
    pub fn slots(&self) -> [[f32; 3]; 7] {
        [
            self.helmet,
            self.facemask,
            self.jersey,
            self.pants,
            self.skin,
            self.shoes,
            self.trim,
        ]
    }

    /// Brand colors: primary (jersey), secondary (pants), accent (trim).
    pub fn primary(&self) -> [f32; 3] {
        self.jersey
    }

    pub fn secondary(&self) -> [f32; 3] {
        self.pants
    }

    pub fn accent(&self) -> [f32; 3] {
        self.trim
    }
}

/// One fictional team. `id` is the SIM side slot (home `0` / away `1`)
/// assigned when the team is placed into a match; `league_id` is the stable
/// league identity the frontend selects by.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TeamDefinition {
    pub id: TeamId,
    pub league_id: LeagueTeamId,
    pub city: &'static str,
    pub name: &'static str,
    pub abbreviation: &'static str,
    pub palette: TeamPalette,
    pub emblem: EmblemDefinition,
    pub ratings: TeamRatings,
}

impl TeamDefinition {
    /// The same team assigned to a different sim side slot.
    pub fn with_sim_slot(mut self, slot: TeamId) -> Self {
        self.id = slot;
        self
    }
}

fn team(
    index: u8,
    city: &'static str,
    name: &'static str,
    abbreviation: &'static str,
    palette: TeamPalette,
    emblem: EmblemDefinition,
    // POWER / SPEED / PASS / DEFENSE, each 1..=MAX_RATING.
    (power, speed, pass, defense): (u8, u8, u8, u8),
) -> TeamDefinition {
    TeamDefinition {
        id: TeamId(index.min(1)),
        league_id: LeagueTeamId(index),
        city,
        name,
        abbreviation,
        palette,
        emblem,
        ratings: TeamRatings {
            power,
            speed,
            pass,
            defense,
        },
    }
}

/// The fixed offense id: CRATER CITY MAGMA (the player's team).
pub const OFFENSE_TEAM: LeagueTeamId = LeagueTeamId(0);
/// The fixed defense id: GLACIER FALLS FROSTBITE (the opponent).
pub const DEFENSE_TEAM: LeagueTeamId = LeagueTeamId(1);

/// The two fixed teams, indexed by [`LeagueTeamId`].
pub fn league() -> [TeamDefinition; LEAGUE_SIZE] {
    [
        // 0 — the fixed offensive team the player always controls.
        team(
            0,
            "CRATER CITY",
            "MAGMA",
            "MAG",
            TeamPalette {
                helmet: [0.62, 0.10, 0.08],
                facemask: [0.12, 0.12, 0.13],
                jersey: [0.78, 0.16, 0.10],
                pants: [0.92, 0.78, 0.34],
                skin: [0.82, 0.62, 0.44],
                shoes: [0.14, 0.13, 0.13],
                trim: [0.55, 0.09, 0.07],
            },
            EmblemDefinition {
                base: EmblemBase::Shield,
                motif: EmblemMotif::Star,
                initial: Some('M'),
            },
            (8, 6, 7, 6),
        ),
        // 1 — the fixed defensive team that always opposes the player.
        team(
            1,
            "GLACIER FALLS",
            "FROSTBITE",
            "FRB",
            TeamPalette {
                helmet: [0.12, 0.32, 0.66],
                facemask: [0.85, 0.88, 0.92],
                jersey: [0.16, 0.42, 0.80],
                pants: [0.82, 0.86, 0.90],
                skin: [0.66, 0.46, 0.32],
                shoes: [0.90, 0.91, 0.94],
                trim: [0.10, 0.26, 0.55],
            },
            EmblemDefinition {
                base: EmblemBase::Hex,
                motif: EmblemMotif::Claw,
                initial: Some('F'),
            },
            (6, 6, 6, 9),
        ),
    ]
}

/// One league team by id (wraps out-of-range ids to the home showcase team —
/// callers validate ids at their boundary; this lookup is total).
pub fn league_team(id: LeagueTeamId) -> TeamDefinition {
    let teams = league();
    teams[usize::from(id.0) % LEAGUE_SIZE]
}

/// The fixed offensive team (league slot 0, sim slot 0).
pub fn magma() -> TeamDefinition {
    league()[0]
}

/// The fixed defensive team (league slot 1, sim slot 1).
pub fn frostbite() -> TeamDefinition {
    league()[1].with_sim_slot(TeamId(1))
}
