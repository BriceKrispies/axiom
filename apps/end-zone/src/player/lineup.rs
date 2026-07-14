//! Formation placement: build every player's simulation record from the
//! roster DATA and place both teams per the play's formations, in stable id
//! order (home `0..7`, away `7..14`; the possession team fills the offense
//! slots).

use axiom::prelude::Vec3;

use crate::ai::assignment::{defense_player, offense_player};
use crate::ai::steering;
use crate::config::PLAYER_COUNT;
use crate::data::{PlayDefinition, RosterDefinition};
use crate::field::OffenseFrame;

use super::PlayerSim;

/// Place both teams for the play.
pub(crate) fn formation_players(
    play: &PlayDefinition,
    frame: &OffenseFrame,
    rosters: &(RosterDefinition, RosterDefinition),
) -> Vec<PlayerSim> {
    let mut players: Vec<PlayerSim> = Vec::with_capacity(PLAYER_COUNT);
    for roster in [&rosters.0, &rosters.1] {
        for definition in &roster.players {
            players.push(PlayerSim::at(
                definition.id,
                definition.team,
                definition.jersey,
                definition.archetype,
                Vec3::ZERO,
                0.0,
            ));
        }
    }
    let offense_faces = steering::yaw_of(frame.forward(), 0.0);
    let defense_faces = steering::yaw_of(frame.forward().mul_scalar(-1.0), 0.0);
    for slot in &play.offense_formation.slots {
        let id = offense_player(play, slot.roster_slot);
        let player = &mut players[id.index()];
        player.pos = frame.to_world(slot.position);
        player.facing = offense_faces;
    }
    for slot in &play.defense_formation.slots {
        let id = defense_player(play, slot.roster_slot);
        let player = &mut players[id.index()];
        player.pos = frame.to_world(slot.position);
        player.facing = defense_faces;
    }
    players
}
