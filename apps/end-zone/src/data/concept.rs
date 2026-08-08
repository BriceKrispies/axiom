//! The selectable offensive **run concepts** — what the play-call phase picks
//! between.
//!
//! Every concept is the same football sentence: snap it, mesh with the back,
//! and open one hole. What changes concept to concept is *which* hole, *where*
//! the exchange happens, and *how long the play takes to get there* — and those
//! three numbers are the whole difference between a dive that hits before the
//! defense has moved and a sweep that has to beat the edge to the corner.
//!
//! | Concept | The hole | Mesh | Character |
//! |---|---|---|---|
//! | DIVE | straight up the middle | tight and immediate | fastest to contact, least room |
//! | OFF TACKLE | outside the right guard | a step across | the balanced one |
//! | SWEEP | wide to the left edge | deep and behind | slowest, most room if you win the corner |
//!
//! The **grammar is a contract**, exactly as the old read order was: concept 1
//! is always the quickest and tightest, concept 3 always the slowest and widest.
//! That is what lets the player carry one mental model across calls — pressing
//! `3` always means "the long way round" — so the call changes the *shape* of
//! the run without changing what the buttons mean.
//!
//! Nothing here is a playbook in the football sense; it is three variations on
//! one tension (get to the hole before it closes) so the run stays fresh across
//! attempts.

use crate::field::{DriveDirection, OffensePoint};
use crate::identity::{PlayId, TeamId};

use super::formation::{power_i_offense, single_back_offense, wing_left_offense};
use super::play::{
    OffenseAssignment, OffenseTag, OffensivePlay, PlayDefinition,
};
use super::player::RUNNING_BACK_SLOT;
use super::playbook::cover_man;

/// Where a run attempt always snaps from, yards from the offense's own goal.
///
/// Fixed so a gain is comparable attempt to attempt and concept to concept, and
/// far enough out that breaking one is a genuine forty-yard run rather than a
/// formality — the touchdown has to be earned by the moves, not by the spot.
pub const RUN_LINE: f32 = 60.0;

/// One selectable run concept.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Concept {
    /// Shown on the pre-snap play card.
    pub name: &'static str,
    /// The one-line description of the hole, shown under the name so the player
    /// is choosing between three described shapes rather than three names.
    pub blurb: &'static str,
    /// Where the exchange happens, offense-relative. Behind the line of
    /// scrimmage by construction — a handoff in front of it is a forward pass.
    pub mesh: OffensePoint,
    /// The designed hole, offense-relative: the point the back attacks first.
    /// Once he is past it, the carry AI turns him upfield for the end zone.
    pub aim: OffensePoint,
}

/// The selectable concepts, in play-card order.
pub const CONCEPT_COUNT: usize = 3;

/// Concept `index` (clamped) — the single lookup every consumer goes through.
pub fn concept(index: usize) -> Concept {
    CONCEPTS[index.min(CONCEPT_COUNT - 1)]
}

const CONCEPTS: [Concept; CONCEPT_COUNT] = [
    Concept {
        name: "DIVE",
        blurb: "STRAIGHT UP THE GUT",
        mesh: OffensePoint {
            lateral: 0.0,
            downfield: -3.4,
        },
        aim: OffensePoint {
            lateral: 0.0,
            downfield: 5.0,
        },
    },
    Concept {
        name: "OFF TACKLE",
        blurb: "OUTSIDE THE RIGHT GUARD",
        mesh: OffensePoint {
            lateral: 1.4,
            downfield: -4.2,
        },
        aim: OffensePoint {
            lateral: 6.5,
            downfield: 4.5,
        },
    },
    Concept {
        name: "SWEEP",
        blurb: "WIDE, ROUND THE LEFT EDGE",
        mesh: OffensePoint {
            lateral: -1.6,
            downfield: -5.0,
        },
        aim: OffensePoint {
            lateral: -13.5,
            downfield: 2.5,
        },
    },
];

/// The offensive play for concept `index`. Slots: 0 quarterback, 1 snapper,
/// 2/3 guards, 4/5 the two wide players, 6 the running back.
///
/// Only two assignments are new (`HandOff` and `RunBack`); everything else is
/// the app's existing blocking vocabulary, which is the point — the linemen and
/// the wide players run the same `lead_block` decision they always did, so the
/// blocking the player is reading is the blocking the AI already knew how to do.
pub fn concept_play(index: usize) -> OffensivePlay {
    let picked = concept(index);
    let (id, formation) = match index.min(CONCEPT_COUNT - 1) {
        0 => (12, power_i_offense()),
        1 => (13, single_back_offense()),
        _ => (14, wing_left_offense()),
    };
    OffensivePlay {
        id: PlayId(id),
        name: picked.name,
        tag: OffenseTag::Run,
        formation,
        assignments: [
            OffenseAssignment::HandOff {
                back_slot: RUNNING_BACK_SLOT,
                mesh: picked.mesh,
            },
            OffenseAssignment::Snapper,
            OffenseAssignment::LeadBlock,
            OffenseAssignment::LeadBlock,
            OffenseAssignment::LeadBlock,
            OffenseAssignment::LeadBlock,
            OffenseAssignment::RunBack {
                mesh: picked.mesh,
                aim: picked.aim,
            },
        ],
    }
}

/// The formation a concept lines up in. Distinct per concept ON PURPOSE: a
/// picker that changed only the aiming point would look identical at the line,
/// so the player could never see that their call had taken.
pub fn concept_formation(index: usize) -> super::formation::FormationDefinition {
    concept_play(index).formation
}

/// The composed play a run attempt lines up before any concept is chosen.
pub fn opening_play() -> PlayDefinition {
    PlayDefinition::compose(
        &concept_play(0),
        &cover_man(),
        TeamId(0),
        DriveDirection::PlusZ,
        RUN_LINE,
    )
}
