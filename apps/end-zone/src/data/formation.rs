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

// ---------------------------------------------------------------------------
// Offensive formations. Slots: 0 QB, 1 snapper, 2/3 guards (blockers),
// 4/5/6 the eligible receivers. The QB/snapper/guards keep the same spots
// across formations; only the receiver spread changes, so a play's job list
// stays aligned to the same roster slots.
// ---------------------------------------------------------------------------

/// Shotgun spread: split ends wide, one slot receiver. The showcase default.
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
            slot(6, 7.5, -1.5),   // slot receiver (right)
        ],
    }
}

/// Doubles: two receivers to each side at tighter splits — a quick-game set.
pub fn doubles_offense() -> FormationDefinition {
    FormationDefinition {
        name: "doubles",
        slots: [
            slot(0, 0.0, -5.0),
            slot(1, 0.0, -0.7),
            slot(2, -1.8, -0.8),
            slot(3, 1.8, -0.8),
            slot(4, -12.5, -0.6), // outside left
            slot(5, 12.5, -0.6),  // outside right
            slot(6, -6.5, -1.4),  // slot left
        ],
    }
}

/// Trips right: three receivers stacked to the offense's right, one isolated
/// backside — a flood set that stresses one sideline.
pub fn trips_right_offense() -> FormationDefinition {
    FormationDefinition {
        name: "trips-right",
        slots: [
            slot(0, 0.0, -5.0),
            slot(1, 0.0, -0.7),
            slot(2, -1.8, -0.8),
            slot(3, 1.8, -0.8),
            slot(4, -13.0, -0.6), // isolated backside (left)
            slot(5, 15.0, -0.6),  // #1 right
            slot(6, 9.0, -1.6),   // #2 right (inside)
        ],
    }
}

// ---------------------------------------------------------------------------
// Defensive formations, authored in the OFFENSE's frame (downfield > 0 is the
// defense's side of the ball). The count of players near the line vs deep is
// what makes a front "heavy" or "light" — the selector picks a call whose
// front + coverage suit the offense.
// ---------------------------------------------------------------------------

/// Base 4-front: four near the line, two corners, one deep safety.
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

/// Nickel: a lighter three-man front with an extra defensive back walked out
/// over the slot — a coverage answer to spread pass sets.
pub fn nickel_defense() -> FormationDefinition {
    FormationDefinition {
        name: "nickel",
        slots: [
            slot(0, -2.4, 1.0),  // left edge
            slot(1, 0.0, 1.0),   // nose
            slot(2, 2.4, 1.0),   // right edge
            slot(3, 7.0, 4.0),   // nickel back over the slot
            slot(4, -13.5, 6.5), // left corner
            slot(5, 13.5, 6.5),  // right corner
            slot(6, 0.0, 15.0),  // free safety
        ],
    }
}

/// Dime / prevent: two rushers and five defenders deep — the answer to an
/// obvious long-yardage passing down, giving up the underneath for the deep.
pub fn dime_defense() -> FormationDefinition {
    FormationDefinition {
        name: "dime",
        slots: [
            slot(0, -2.0, 1.0),   // left rusher
            slot(1, 2.0, 1.0),    // right rusher
            slot(2, -7.0, 9.0),   // curl/flat left
            slot(3, 7.0, 9.0),    // curl/flat right
            slot(4, -15.0, 12.0), // deep third left
            slot(5, 15.0, 12.0),  // deep third right
            slot(6, 0.0, 20.0),   // deep middle
        ],
    }
}
