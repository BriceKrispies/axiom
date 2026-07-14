//! Fictional team definitions: the six-team END ZONE league. Every team is
//! original — fictional city, original name, procedural emblem — with bounded
//! ratings and a palette consumed by the player-model construction and the
//! end-zone paint. No real-world league, team, or player branding appears
//! anywhere, and gameplay code contains zero team branches: everything a team
//! IS lives in this data.

use crate::identity::TeamId;

/// A league team's stable identity (index into [`league`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LeagueTeamId(pub u8);

/// How many teams the league carries.
pub const LEAGUE_SIZE: usize = 6;

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

/// The six-team league, indexed by [`LeagueTeamId`].
pub fn league() -> [TeamDefinition; LEAGUE_SIZE] {
    [
        // 0 — the original home showcase team.
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
        // 1 — the original away showcase team.
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
        // 2 — the heaviest line in the league.
        team(
            2,
            "IRONPORT",
            "ANVILS",
            "ANV",
            TeamPalette {
                helmet: [0.16, 0.17, 0.19],
                facemask: [0.90, 0.48, 0.10],
                jersey: [0.24, 0.25, 0.28],
                pants: [0.62, 0.63, 0.66],
                skin: [0.74, 0.54, 0.38],
                shoes: [0.10, 0.10, 0.11],
                trim: [0.90, 0.48, 0.10],
            },
            EmblemDefinition {
                base: EmblemBase::Pennant,
                motif: EmblemMotif::Chevrons,
                initial: Some('A'),
            },
            (10, 4, 5, 7),
        ),
        // 3 — pure track speed.
        team(
            3,
            "NEON VALLEY",
            "VOLTAGE",
            "VLT",
            TeamPalette {
                helmet: [0.05, 0.07, 0.06],
                facemask: [0.35, 0.95, 0.25],
                jersey: [0.24, 0.82, 0.20],
                pants: [0.07, 0.09, 0.08],
                skin: [0.58, 0.42, 0.30],
                shoes: [0.85, 0.95, 0.30],
                trim: [0.35, 0.95, 0.25],
            },
            EmblemDefinition {
                base: EmblemBase::Disc,
                motif: EmblemMotif::Bolt,
                initial: Some('V'),
            },
            (4, 10, 6, 5),
        ),
        // 4 — an air-raid passing attack.
        team(
            4,
            "STORM HARBOR",
            "TEMPEST",
            "TMP",
            TeamPalette {
                helmet: [0.34, 0.16, 0.55],
                facemask: [0.95, 0.82, 0.25],
                jersey: [0.42, 0.20, 0.68],
                pants: [0.93, 0.85, 0.42],
                skin: [0.80, 0.60, 0.42],
                shoes: [0.20, 0.12, 0.30],
                trim: [0.95, 0.82, 0.25],
            },
            EmblemDefinition {
                base: EmblemBase::Shield,
                motif: EmblemMotif::Wing,
                initial: Some('T'),
            },
            (5, 7, 10, 4),
        ),
        // 5 — a snarling geometric-animal defense.
        team(
            5,
            "BLACKRIDGE",
            "HOWLERS",
            "HWL",
            TeamPalette {
                helmet: [0.55, 0.56, 0.60],
                facemask: [0.10, 0.10, 0.11],
                jersey: [0.66, 0.10, 0.16],
                pants: [0.55, 0.56, 0.60],
                skin: [0.70, 0.50, 0.36],
                shoes: [0.12, 0.12, 0.13],
                trim: [0.86, 0.88, 0.92],
            },
            EmblemDefinition {
                base: EmblemBase::Hex,
                motif: EmblemMotif::Fang,
                initial: None,
            },
            (7, 8, 4, 8),
        ),
    ]
}

/// One league team by id (wraps out-of-range ids to the home showcase team —
/// callers validate ids at their boundary; this lookup is total).
pub fn league_team(id: LeagueTeamId) -> TeamDefinition {
    let teams = league();
    teams[usize::from(id.0) % LEAGUE_SIZE]
}

/// Home showcase team (league slot 0, sim slot 0).
pub fn magma() -> TeamDefinition {
    league()[0]
}

/// Away showcase team (league slot 1, sim slot 1).
pub fn frostbite() -> TeamDefinition {
    league()[1].with_sim_slot(TeamId(1))
}
