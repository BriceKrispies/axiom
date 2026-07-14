//! Formation definitions: named pre-snap placements in offense-relative
//! coordinates. `lateral` is toward the offense's right, `downfield` is toward
//! the opponent end zone — so a formation authored once lines up correctly in
//! either drive direction through [`crate::field::OffenseFrame`].

use crate::config::PLAYERS_PER_TEAM;
use crate::field::OffensePoint;

/// One player's spot: roster slot `0..=6` plus the offense-relative position.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FormationSlot {
    pub roster_slot: usize,
    pub position: OffensePoint,
}

/// A named seven-player formation.
#[derive(Debug, Clone, PartialEq)]
pub struct FormationDefinition {
    pub name: &'static str,
    pub slots: [FormationSlot; PLAYERS_PER_TEAM],
}

fn slot(roster_slot: usize, lateral: f32, downfield: f32) -> FormationSlot {
    FormationSlot {
        roster_slot,
        position: OffensePoint::new(lateral, downfield),
    }
}

/// The showcase offense: shotgun-ish spread. Slots: 0 QB, 1 snapper,
/// 2/3 guards, 4/5 wide receivers, 6 slot receiver.
pub fn spread_offense() -> FormationDefinition {
    FormationDefinition {
        name: "spread",
        slots: [
            slot(0, 0.0, -5.0),   // quarterback, in the gun
            slot(1, 0.0, -0.7),   // snapper on the ball
            slot(2, -1.8, -0.8),  // left guard
            slot(3, 1.8, -0.8),   // right guard
            slot(4, -14.0, -0.6), // split end (offense left)
            slot(5, 14.0, -0.6),  // flanker (offense right)
            slot(6, 7.5, -1.5),   // slot receiver
        ],
    }
}

/// The showcase defense, authored in the OFFENSE's frame (downfield > 0 is the
/// defense's side of the ball). Slots: 0/3 rushers, 1/2 line, 4/5 corners,
/// 6 deep safety.
pub fn base_defense() -> FormationDefinition {
    FormationDefinition {
        name: "base",
        slots: [
            slot(0, -3.2, 1.0),  // left edge rusher
            slot(1, -1.0, 1.0),  // nose
            slot(2, 1.0, 1.0),   // tackle
            slot(3, 3.2, 1.0),   // right edge rusher
            slot(4, -13.5, 6.0), // corner over the split end
            slot(5, 13.5, 6.0),  // corner over the flanker
            slot(6, 0.0, 17.0),  // free safety
        ],
    }
}
