//! Player archetypes and rosters. Archetype numbers are the ONLY thing that
//! differentiates player behavior — the same controller and AI code runs every
//! player of both teams.

use crate::config::PLAYERS_PER_TEAM;
use crate::identity::{PlayerId, TeamId};

use super::team::{frostbite, magma, TeamDefinition};

/// Movement, contact, and catching numbers for one kind of player. Units:
/// yards, seconds, radians, ticks (60 Hz).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerArchetype {
    pub name: &'static str,
    /// Top speed, yd/s.
    pub max_speed: f32,
    /// Acceleration limit, yd/s².
    pub acceleration: f32,
    /// Turn-rate limit, rad/s.
    pub turn_rate: f32,
    /// Contact circle radius, yd.
    pub body_radius: f32,
    /// Relative mass (contact contests).
    pub mass: f32,
    /// Blocking contest strength, 0..=1.
    pub block_strength: f32,
    /// Tackling contest strength, 0..=1.
    pub tackle_strength: f32,
    /// Catch volume radius, yd.
    pub catch_radius: f32,
    /// Catch timing tolerance around the ball's arrival, ticks.
    pub catch_tolerance_ticks: u32,
    /// Pursuit prediction gain, 0..=1 (how far ahead of the carrier to aim).
    pub pursuit_aggressiveness: f32,
    /// Perception delay before reacting to a change, ticks.
    pub reaction_delay_ticks: u32,
}

pub const fn quarterback() -> PlayerArchetype {
    PlayerArchetype {
        name: "quarterback",
        max_speed: 6.4,
        acceleration: 9.0,
        turn_rate: 6.0,
        body_radius: 0.55,
        mass: 1.0,
        block_strength: 0.2,
        tackle_strength: 0.2,
        catch_radius: 1.0,
        catch_tolerance_ticks: 8,
        pursuit_aggressiveness: 0.3,
        reaction_delay_ticks: 6,
    }
}

pub const fn receiver() -> PlayerArchetype {
    PlayerArchetype {
        name: "receiver",
        max_speed: 8.6,
        acceleration: 11.0,
        turn_rate: 7.5,
        body_radius: 0.52,
        mass: 0.9,
        block_strength: 0.25,
        tackle_strength: 0.3,
        catch_radius: 1.6,
        catch_tolerance_ticks: 14,
        pursuit_aggressiveness: 0.5,
        reaction_delay_ticks: 5,
    }
}

pub const fn lineman() -> PlayerArchetype {
    PlayerArchetype {
        name: "lineman",
        max_speed: 5.6,
        acceleration: 7.0,
        turn_rate: 4.5,
        body_radius: 0.68,
        mass: 1.5,
        block_strength: 0.9,
        tackle_strength: 0.7,
        catch_radius: 0.9,
        catch_tolerance_ticks: 6,
        pursuit_aggressiveness: 0.2,
        reaction_delay_ticks: 8,
    }
}

pub const fn defender() -> PlayerArchetype {
    PlayerArchetype {
        name: "defender",
        max_speed: 8.2,
        acceleration: 10.0,
        turn_rate: 6.5,
        body_radius: 0.56,
        mass: 1.1,
        block_strength: 0.5,
        tackle_strength: 0.85,
        catch_radius: 1.2,
        catch_tolerance_ticks: 8,
        pursuit_aggressiveness: 0.7,
        reaction_delay_ticks: 7,
    }
}

pub const fn safety() -> PlayerArchetype {
    PlayerArchetype {
        name: "safety",
        max_speed: 9.4,
        acceleration: 11.5,
        turn_rate: 7.0,
        body_radius: 0.54,
        mass: 1.0,
        block_strength: 0.4,
        tackle_strength: 0.8,
        catch_radius: 1.3,
        catch_tolerance_ticks: 9,
        pursuit_aggressiveness: 0.9,
        reaction_delay_ticks: 16,
    }
}

/// One rostered player.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayerDefinition {
    pub id: PlayerId,
    pub team: TeamId,
    pub jersey: u8,
    pub archetype: PlayerArchetype,
}

/// A seven-player showcase roster.
#[derive(Debug, Clone, PartialEq)]
pub struct RosterDefinition {
    pub team: TeamDefinition,
    pub players: [PlayerDefinition; PLAYERS_PER_TEAM],
}

fn roster(
    team: TeamDefinition,
    base_id: u8,
    archetypes: [PlayerArchetype; PLAYERS_PER_TEAM],
) -> RosterDefinition {
    let mut slot = 0u8;
    let players = archetypes.map(|archetype| {
        let p = PlayerDefinition {
            id: PlayerId(base_id + slot),
            team: team.id,
            jersey: 10 + slot * 11,
            archetype,
        };
        slot += 1;
        p
    });
    RosterDefinition { team, players }
}

/// Which side of the ball a roster is built for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RosterSide {
    Offense,
    Defense,
}

/// The rating→multiplier curve: rating 5 is neutral (`1.0`), 1 is `0.88`,
/// 10 is `1.15`. Bounded by construction.
fn rating_scale(rating: u8) -> f32 {
    0.85 + 0.03 * f32::from(rating.clamp(1, super::team::MAX_RATING))
}

/// Apply a team's bounded ratings to one archetype — pure data scaling, the
/// ONLY place team strength touches numbers (no team branches anywhere).
fn rated(mut a: PlayerArchetype, team: &TeamDefinition, side: RosterSide) -> PlayerArchetype {
    let speed = rating_scale(team.ratings.speed);
    let power = rating_scale(team.ratings.power);
    a.max_speed *= speed;
    a.acceleration *= speed;
    a.mass *= power;
    a.block_strength = (a.block_strength * power).min(1.0);
    match side {
        RosterSide::Offense => {
            let pass = rating_scale(team.ratings.pass);
            a.catch_radius *= pass;
            a.catch_tolerance_ticks =
                ((a.catch_tolerance_ticks as f32 * pass).round() as u32).max(1);
        }
        RosterSide::Defense => {
            let defense = rating_scale(team.ratings.defense);
            a.tackle_strength = (a.tackle_strength * defense).min(1.0);
            a.pursuit_aggressiveness = (a.pursuit_aggressiveness * defense).min(1.0);
            a.reaction_delay_ticks =
                ((a.reaction_delay_ticks as f32 / defense).round() as u32).max(1);
        }
    }
    a
}

/// Build a league team's seven-player roster for one side of the ball, its
/// archetypes scaled by the team's ratings. Roster slot order is meaningful:
/// play formations and assignments address players by roster slot `0..=6`.
///
/// Offense slots: 0 QB, 1 snapper, 2/3 linemen, 4/5 receivers, 6 slot.
/// Defense slots: 0/3 rushers, 1/2 line, 4/5 corners, 6 safety.
pub fn roster_for(team: TeamDefinition, base_id: u8, side: RosterSide) -> RosterDefinition {
    let archetypes = match side {
        RosterSide::Offense => [
            quarterback(),
            lineman(),
            lineman(),
            lineman(),
            receiver(),
            receiver(),
            receiver(),
        ],
        RosterSide::Defense => [
            defender(),
            lineman(),
            lineman(),
            defender(),
            defender(),
            defender(),
            safety(),
        ],
    };
    roster(team, base_id, archetypes.map(|a| rated(a, &team, side)))
}

/// The two fictional showcase rosters (league slots 0 and 1).
pub fn showcase_rosters() -> (RosterDefinition, RosterDefinition) {
    (
        roster_for(magma(), 0, RosterSide::Offense),
        roster_for(frostbite(), PLAYERS_PER_TEAM as u8, RosterSide::Defense),
    )
}
