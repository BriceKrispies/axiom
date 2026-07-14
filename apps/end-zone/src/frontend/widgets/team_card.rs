//! The team card: emblem, city + name, abbreviation, team colors, the four
//! rating bars, and a small procedural lineup strip. Under enhanced color
//! distinction the card always carries its abbreviation, emblem silhouette,
//! home/away label, and a patterned edge — never color alone.

use crate::data::team::{EmblemBase, EmblemMotif, TeamDefinition};
use crate::frontend::theme::css_color;

/// Home/away identification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Home,
    Away,
}

impl Side {
    pub fn label(self) -> &'static str {
        match self {
            Side::Home => "HOME",
            Side::Away => "AWAY",
        }
    }
}

/// The procedural emblem view (interpreted by app-local drawing code).
#[derive(Debug, Clone, PartialEq)]
pub struct EmblemView {
    pub base: EmblemBase,
    pub motif: EmblemMotif,
    pub initial: Option<char>,
    pub primary: String,
    pub secondary: String,
    pub accent: String,
}

impl EmblemView {
    pub fn of(team: &TeamDefinition) -> Self {
        EmblemView {
            base: team.emblem.base,
            motif: team.emblem.motif,
            initial: team.emblem.initial,
            primary: css_color(team.palette.primary()),
            secondary: css_color(team.palette.secondary()),
            accent: css_color(team.palette.accent()),
        }
    }
}

/// The four bounded rating bars (POWER / SPEED / PASS / DEFENSE).
#[derive(Debug, Clone, PartialEq)]
pub struct RatingBars {
    pub power: u8,
    pub speed: u8,
    pub pass: u8,
    pub defense: u8,
    pub accent: String,
    pub compact: bool,
}

impl RatingBars {
    pub fn of(team: &TeamDefinition) -> Self {
        RatingBars {
            power: team.ratings.power,
            speed: team.ratings.speed,
            pass: team.ratings.pass,
            defense: team.ratings.defense,
            accent: css_color(team.palette.accent()),
            compact: false,
        }
    }
}

/// One team card.
#[derive(Debug, Clone, PartialEq)]
pub struct TeamCard {
    pub league_id: u8,
    pub city: String,
    pub name: String,
    pub abbreviation: String,
    pub primary: String,
    pub secondary: String,
    pub accent: String,
    pub emblem: EmblemView,
    pub ratings: RatingBars,
    /// A locked (confirmed) card shows its lock plate.
    pub locked: bool,
    /// Home/away tag (always shown under enhanced color distinction).
    pub side: Option<Side>,
    /// Show the mini procedural lineup strip (seven jersey chips).
    pub lineup: bool,
    /// Compressed layout for narrow viewports / preview cards.
    pub compact: bool,
    /// Preview cards render dimmed.
    pub preview: bool,
}

impl TeamCard {
    pub fn of(team: &TeamDefinition) -> Self {
        TeamCard {
            league_id: team.league_id.0,
            city: team.city.to_string(),
            name: team.name.to_string(),
            abbreviation: team.abbreviation.to_string(),
            primary: css_color(team.palette.primary()),
            secondary: css_color(team.palette.secondary()),
            accent: css_color(team.palette.accent()),
            emblem: EmblemView::of(team),
            ratings: RatingBars::of(team),
            locked: false,
            side: None,
            lineup: false,
            compact: false,
            preview: false,
        }
    }
}
