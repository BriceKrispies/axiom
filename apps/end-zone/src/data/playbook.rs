//! The playbook: the catalog of selectable offensive plays and the defensive
//! calls the game answers them with. Everything here is plain data — formations
//! and per-slot jobs — interpreted by the generic simulation systems. The
//! player picks an [`OffensivePlay`]; [`crate::ai::playcall`] deterministically
//! picks a [`DefensiveCall`] in response.
//!
//! Only pass concepts live here today: the simulation runs snap → drop → routes
//! → user throw, and has no snap-handoff mechanic yet, so a run play would not
//! function. Run concepts arrive with a handoff mechanic in the simulation.

use crate::field::{DriveDirection, OffensePoint};
use crate::identity::{PlayId, TeamId};

use super::formation::{
    base_defense, dime_defense, doubles_offense, nickel_defense, spread_offense,
    trips_right_offense,
};
use super::play::{
    Coverage, DefenseAssignment, DefenseFront, DefensiveCall, OffenseAssignment, OffenseTag,
    OffensivePlay, PlayDefinition, RouteDefinition, RouteShape,
};

fn route(shape: RouteShape) -> OffenseAssignment {
    OffenseAssignment::Route(RouteDefinition::Shape(shape))
}

fn decoy(shape: RouteShape) -> OffenseAssignment {
    OffenseAssignment::DecoyRoute(RouteDefinition::Shape(shape))
}

fn zone(lateral: f32, downfield: f32, radius: f32) -> DefenseAssignment {
    DefenseAssignment::ZoneCover {
        center: OffensePoint::new(lateral, downfield),
        radius,
    }
}

// ---------------------------------------------------------------------------
// Offensive playbook. Slots: 0 QB, 1 snapper, 2/3 pass blockers, 4/5/6 the
// eligible receivers (aligned to the formation's receiver slots).
// ---------------------------------------------------------------------------

/// Slot post: the showcase default — a slot post primary off a spread set with
/// an out and a clear-out decoy.
pub fn slot_post() -> OffensivePlay {
    OffensivePlay {
        id: PlayId(1),
        name: "SLOT POST",
        tag: OffenseTag::DeepPass,
        formation: spread_offense(),
        assignments: [
            OffenseAssignment::Quarterback { drop_depth: 3.0 },
            OffenseAssignment::Snapper,
            OffenseAssignment::PassBlock,
            OffenseAssignment::PassBlock,
            decoy(RouteShape::Straight { depth: 16.0 }),
            route(RouteShape::Out { stem: 8.0, cut: 5.0 }),
            route(RouteShape::Post { stem: 7.0, cut: 6.0 }),
        ],
    }
}

/// Four verticals: everyone runs deep to stress the coverage over the top.
pub fn four_verticals() -> OffensivePlay {
    OffensivePlay {
        id: PlayId(2),
        name: "FOUR VERTS",
        tag: OffenseTag::DeepPass,
        formation: spread_offense(),
        assignments: [
            OffenseAssignment::Quarterback { drop_depth: 5.0 },
            OffenseAssignment::Snapper,
            OffenseAssignment::PassBlock,
            OffenseAssignment::PassBlock,
            route(RouteShape::Straight { depth: 18.0 }),
            route(RouteShape::Straight { depth: 18.0 }),
            route(RouteShape::Straight { depth: 16.0 }),
        ],
    }
}

/// Quick slants: a rhythm-throw beater from a doubles set — get it out fast.
pub fn quick_slants() -> OffensivePlay {
    OffensivePlay {
        id: PlayId(3),
        name: "QUICK SLANTS",
        tag: OffenseTag::QuickPass,
        formation: doubles_offense(),
        assignments: [
            OffenseAssignment::Quarterback { drop_depth: 1.5 },
            OffenseAssignment::Snapper,
            OffenseAssignment::PassBlock,
            OffenseAssignment::PassBlock,
            route(RouteShape::Slant { stem: 3.0, cut: 3.0 }),
            route(RouteShape::Slant { stem: 3.0, cut: 3.0 }),
            route(RouteShape::Slant { stem: 4.0, cut: 3.0 }),
        ],
    }
}

/// Smash flood: a corner-and-out combination that outnumbers one sideline off
/// a trips set, with a backside clear-out.
pub fn smash_flood() -> OffensivePlay {
    OffensivePlay {
        id: PlayId(4),
        name: "SMASH FLOOD",
        tag: OffenseTag::Flood,
        formation: trips_right_offense(),
        assignments: [
            OffenseAssignment::Quarterback { drop_depth: 3.0 },
            OffenseAssignment::Snapper,
            OffenseAssignment::PassBlock,
            OffenseAssignment::PassBlock,
            decoy(RouteShape::Straight { depth: 12.0 }),
            route(RouteShape::Corner { stem: 8.0, cut: 6.0 }),
            route(RouteShape::Out { stem: 5.0, cut: 5.0 }),
        ],
    }
}

/// The offensive playbook, in a stable display order (id-aligned). Index `0` is
/// the default the huddle pre-selects.
pub fn offensive_playbook() -> [OffensivePlay; 4] {
    [slot_post(), four_verticals(), quick_slants(), smash_flood()]
}

// ---------------------------------------------------------------------------
// Defensive calls. Each is a real, exploitable tradeoff — the selector picks
// among the sensible answers to the offense's tag and the down/distance.
// ---------------------------------------------------------------------------

/// Base man: a four-man rush with man coverage outside and a free safety. The
/// showcase default answer.
pub fn cover_man() -> DefensiveCall {
    DefensiveCall {
        name: "COVER MAN",
        front: DefenseFront::Base,
        coverage: Coverage::Man,
        formation: base_defense(),
        assignments: [
            DefenseAssignment::QuarterbackRush,
            DefenseAssignment::EdgeContain { lateral: -4.5 },
            DefenseAssignment::EdgeContain { lateral: 4.5 },
            DefenseAssignment::QuarterbackRush,
            DefenseAssignment::ManCover { target_slot: 4 },
            DefenseAssignment::ManCover { target_slot: 5 },
            DefenseAssignment::Pursuit,
        ],
    }
}

/// Base zone: the same four-man front, but the secondary guards areas and
/// rallies to the ball — softer underneath, harder to beat deep.
pub fn cover_zone() -> DefensiveCall {
    DefensiveCall {
        name: "COVER ZONE",
        front: DefenseFront::Base,
        coverage: Coverage::Zone,
        formation: base_defense(),
        assignments: [
            DefenseAssignment::QuarterbackRush,
            DefenseAssignment::EdgeContain { lateral: -4.5 },
            DefenseAssignment::EdgeContain { lateral: 4.5 },
            DefenseAssignment::QuarterbackRush,
            zone(-11.0, 7.0, 6.0),
            zone(11.0, 7.0, 6.0),
            zone(0.0, 15.0, 8.0),
        ],
    }
}

/// Nickel zone: a lighter front with an extra defensive back — a coverage
/// answer to a spread pass set, giving up some rush to blanket the routes.
pub fn nickel_zone() -> DefensiveCall {
    DefensiveCall {
        name: "NICKEL ZONE",
        front: DefenseFront::Nickel,
        coverage: Coverage::Zone,
        formation: nickel_defense(),
        assignments: [
            DefenseAssignment::QuarterbackRush,
            DefenseAssignment::QuarterbackRush,
            DefenseAssignment::QuarterbackRush,
            zone(6.0, 5.0, 5.0),
            zone(-12.0, 7.0, 6.0),
            zone(12.0, 7.0, 6.0),
            zone(0.0, 14.0, 8.0),
        ],
    }
}

/// Edge blitz: the safety comes off the edge for a fifth rusher, man behind and
/// no deep help — the exposed region is the middle the safety vacated.
pub fn edge_blitz() -> DefensiveCall {
    DefensiveCall {
        name: "EDGE BLITZ",
        front: DefenseFront::Base,
        coverage: Coverage::Blitz,
        formation: base_defense(),
        assignments: [
            DefenseAssignment::QuarterbackRush,
            DefenseAssignment::EdgeContain { lateral: -4.5 },
            DefenseAssignment::EdgeContain { lateral: 4.5 },
            DefenseAssignment::QuarterbackRush,
            DefenseAssignment::ManCover { target_slot: 4 },
            DefenseAssignment::ManCover { target_slot: 5 },
            DefenseAssignment::QuarterbackRush,
        ],
    }
}

/// Prevent: a two-man rush and five defenders deep — the answer to an obvious
/// long-yardage passing down, conceding the underneath to protect the sticks.
pub fn prevent() -> DefensiveCall {
    DefensiveCall {
        name: "PREVENT",
        front: DefenseFront::Dime,
        coverage: Coverage::Zone,
        formation: dime_defense(),
        assignments: [
            DefenseAssignment::QuarterbackRush,
            DefenseAssignment::QuarterbackRush,
            zone(-8.0, 9.0, 6.0),
            zone(8.0, 9.0, 6.0),
            zone(-14.0, 16.0, 8.0),
            zone(14.0, 16.0, 8.0),
            zone(0.0, 20.0, 8.0),
        ],
    }
}

/// Every defensive call the selector may reach for, in a stable order.
pub fn defensive_calls() -> [DefensiveCall; 5] {
    [
        cover_man(),
        cover_zone(),
        nickel_zone(),
        edge_blitz(),
        prevent(),
    ]
}

/// The showcase play: the default offense against the default defensive answer,
/// composed at the showcase line of scrimmage. Byte-identical to the original
/// fused definition, so the deterministic replay is unchanged.
pub fn showcase_play() -> PlayDefinition {
    PlayDefinition::compose(
        &slot_post(),
        &cover_man(),
        TeamId(0),
        DriveDirection::PlusZ,
        35.0,
    )
}
