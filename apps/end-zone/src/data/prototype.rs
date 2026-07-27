//! The decision-window prototype's selectable offensive **concepts**.
//!
//! Every concept fields the same three-receiver formation and answers the same
//! question — *which of these three, or run?* — but sets a different problem:
//!
//! | Concept | Read 1 | Read 2 | Read 3 |
//! |---|---|---|---|
//! | TRIPLE READ | slant, ~1.1 s | dig across the middle, ~2.4 s | post, ~3.2 s |
//! | FLOOD | flat, immediate | corner over the top | deep go |
//! | MIRROR | hitch, safest of all | shallow cross | wheel up the sideline |
//!
//! The **read order is a contract**: read 1 is always the earliest and safest,
//! read 3 always the latest and largest. That is what lets the player carry one
//! mental model across concepts — pressing `3` always means "the big one" — so
//! choosing a play changes the *shape* of the decision without changing the
//! grammar of it. Nothing here is a playbook in the football sense; it is three
//! variations on one tension so the read stays fresh across attempts.
//!
//! Route depths are authored so that ordering holds: `read_slots` maps read
//! order onto roster slots, which is why the slot order differs per concept
//! (the deepest route is not always the same receiver).

use crate::field::{DriveDirection, OffensePoint};
use crate::identity::{PlayId, TeamId};

use super::formation::{doubles_offense, spread_offense, trips_right_offense};
use super::play::{
    OffenseAssignment, OffenseTag, OffensivePlay, PlayDefinition, RouteDefinition, RouteShape,
};
use super::playbook::cover_man;

/// The number of eligible targets every concept fields.
pub const READ_COUNT: usize = 3;

/// Where the prototype always snaps from. Fixed so the yards a read gains are
/// comparable attempt to attempt, and concept to concept.
pub const PROTOTYPE_LINE: f32 = 40.0;

/// One selectable offensive concept.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Concept {
    /// Shown on the pre-snap picker.
    pub name: &'static str,
    /// Offensive roster slots in READ ORDER (read 1 first).
    pub read_slots: [usize; READ_COUNT],
    /// Each read's route name, for the decision prompt.
    pub read_names: [&'static str; READ_COUNT],
    /// Roughly what each read is worth caught clean, yards. Presentation and
    /// AI valuation only — the real gain is always measured on the field.
    pub read_rewards: [f32; READ_COUNT],
}

/// The selectable concepts, in picker order.
pub const CONCEPT_COUNT: usize = 3;

/// Concept `index` (clamped), the single lookup every consumer goes through.
pub fn concept(index: usize) -> Concept {
    CONCEPTS[index.min(CONCEPT_COUNT - 1)]
}

const CONCEPTS: [Concept; CONCEPT_COUNT] = [
    Concept {
        name: "TRIPLE READ",
        read_slots: [6, 5, 4],
        read_names: ["SLANT", "DIG", "POST"],
        read_rewards: [5.0, 12.0, 20.0],
    },
    Concept {
        name: "FLOOD",
        read_slots: [6, 5, 4],
        read_names: ["FLAT", "CORNER", "GO"],
        read_rewards: [4.0, 14.0, 22.0],
    },
    Concept {
        name: "MIRROR",
        read_slots: [6, 4, 5],
        read_names: ["HITCH", "CROSS", "WHEEL"],
        read_rewards: [5.0, 11.0, 21.0],
    },
];

fn route(shape: RouteShape) -> OffenseAssignment {
    OffenseAssignment::Route(RouteDefinition::Shape(shape))
}

/// The offensive play for concept `index`. Slots: 0 quarterback, 1 snapper,
/// 2/3 pass blockers, 4/5/6 the three eligible reads.
///
/// The quarterback's five-yard drop is shared: the pocket has to be a real,
/// visible space that collapses over the play, whichever concept is called.
pub fn concept_play(index: usize) -> OffensivePlay {
    let quarterback = OffenseAssignment::Quarterback { drop_depth: 5.0 };
    let (id, name, tag, slot4, slot5, slot6) = match index.min(CONCEPT_COUNT - 1) {
        // TRIPLE READ — a slant under, a dig across, a post over the top.
        0 => (
            9,
            "TRIPLE READ",
            OffenseTag::DeepPass,
            route(RouteShape::Post { stem: 8.0, cut: 8.0 }),
            route(RouteShape::In { stem: 11.0, cut: 16.0 }),
            route(RouteShape::Slant { stem: 3.5, cut: 3.5 }),
        ),
        // FLOOD — three levels down ONE sideline, so the read is vertical
        // rather than lateral: the coverage cannot cover all three depths.
        1 => (
            10,
            "FLOOD",
            OffenseTag::Flood,
            route(RouteShape::Straight { depth: 20.0 }),
            route(RouteShape::Corner { stem: 9.0, cut: 7.0 }),
            route(RouteShape::Out { stem: 2.0, cut: 6.0 }),
        ),
        // MIRROR — a sit-down hitch, a shallow crosser running away from the
        // traffic, and a wheel turning up the sideline behind them.
        _ => (
            11,
            "MIRROR",
            OffenseTag::QuickPass,
            route(RouteShape::In { stem: 4.0, cut: 18.0 }),
            route(RouteShape::Corner { stem: 4.0, cut: 12.0 }),
            route(RouteShape::Curl { stem: 6.0, back: 1.5 }),
        ),
    };
    OffensivePlay {
        id: PlayId(id),
        name,
        tag,
        formation: concept_formation(index),
        assignments: [
            quarterback,
            OffenseAssignment::Snapper,
            OffenseAssignment::PassBlock,
            OffenseAssignment::PassBlock,
            slot4,
            slot5,
            slot6,
        ],
    }
}

/// The formation a concept lines up in. Distinct per concept ON PURPOSE: a
/// picker that changed only the routes would look identical at the line, so the
/// player could never see that their call had taken.
pub fn concept_formation(index: usize) -> super::formation::FormationDefinition {
    match index.min(CONCEPT_COUNT - 1) {
        0 => spread_offense(),
        1 => trips_right_offense(),
        _ => doubles_offense(),
    }
}

/// The offense-relative alignment of a read's receiver in `concept`.
pub fn read_alignment(concept_index: usize, read: usize) -> OffensePoint {
    let slot = concept(concept_index).read_slots[read.min(READ_COUNT - 1)];
    concept_formation(concept_index).slots[slot].position
}

/// The composed play the prototype lines up before any concept is chosen.
pub fn prototype_play() -> PlayDefinition {
    PlayDefinition::compose(
        &concept_play(0),
        &cover_man(),
        TeamId(0),
        DriveDirection::PlusZ,
        PROTOTYPE_LINE,
    )
}
