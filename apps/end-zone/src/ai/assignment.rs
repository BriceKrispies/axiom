//! Assignment evaluation: resolve a [`PlayDefinition`]'s per-slot data into
//! per-player assignments in ascending [`PlayerId`] order — routes compiled to
//! world waypoints through the play's offense frame, coverage targets resolved
//! to player ids. Pure data-to-data; no behavior lives here.

use axiom::prelude::Vec3;

use crate::config::{PLAYERS_PER_TEAM, PLAYER_COUNT};
use crate::data::play::{DefenseAssignment, OffenseAssignment, PlayDefinition};
use crate::field::OffenseFrame;
use crate::identity::PlayerId;

/// A resolved per-player assignment.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedAssignment {
    pub kind: AssignmentKind,
    /// World-space route waypoints (empty when the assignment has no route).
    pub route: Vec<Vec3>,
}

/// The resolved assignment vocabulary the brains interpret.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AssignmentKind {
    Quarterback { drop_to: Vec3 },
    Snapper,
    Route { decoy: bool },
    PassBlock,
    LeadBlock,
    BallCarry,
    ManCover { target: PlayerId },
    ZoneCover { center: Vec3, radius: f32 },
    QuarterbackRush { quarterback: PlayerId },
    EdgeContain { post: Vec3, quarterback: PlayerId },
    Pursuit,
    TackleTarget,
}

/// The global [`PlayerId`] of an offense/defense roster slot for this play.
/// Home ids are `0..7`, away ids are `7..14`; the possession team fills the
/// offense slots.
pub fn offense_player(play: &PlayDefinition, roster_slot: usize) -> PlayerId {
    let base = usize::from(play.possession.0) * PLAYERS_PER_TEAM;
    PlayerId((base + roster_slot) as u8)
}

/// The defense counterpart of [`offense_player`].
pub fn defense_player(play: &PlayDefinition, roster_slot: usize) -> PlayerId {
    let base = (1 - usize::from(play.possession.0)) * PLAYERS_PER_TEAM;
    PlayerId((base + roster_slot) as u8)
}

/// Resolve every player's assignment, indexed by [`PlayerId`].
pub fn compile_assignments(play: &PlayDefinition, frame: &OffenseFrame) -> Vec<ResolvedAssignment> {
    let mut resolved: Vec<Option<ResolvedAssignment>> = (0..PLAYER_COUNT).map(|_| None).collect();

    let quarterback_id = play
        .offense_assignments
        .iter()
        .position(|a| matches!(a, OffenseAssignment::Quarterback { .. }))
        .map(|slot| offense_player(play, slot))
        .unwrap_or_else(|| offense_player(play, 0));

    for (slot, assignment) in play.offense_assignments.iter().enumerate() {
        let id = offense_player(play, slot);
        let start = play.offense_formation.slots[slot].position;
        let (kind, route) = match assignment {
            OffenseAssignment::Quarterback { drop_depth } => (
                AssignmentKind::Quarterback {
                    drop_to: frame.to_world(crate::field::OffensePoint::new(
                        start.lateral,
                        start.downfield - drop_depth,
                    )),
                },
                Vec::new(),
            ),
            OffenseAssignment::Snapper => (AssignmentKind::Snapper, Vec::new()),
            OffenseAssignment::Route(route) => (
                AssignmentKind::Route { decoy: false },
                route
                    .waypoints(start)
                    .into_iter()
                    .map(|p| frame.to_world(p))
                    .collect(),
            ),
            OffenseAssignment::DecoyRoute(route) => (
                AssignmentKind::Route { decoy: true },
                route
                    .waypoints(start)
                    .into_iter()
                    .map(|p| frame.to_world(p))
                    .collect(),
            ),
            OffenseAssignment::PassBlock => (AssignmentKind::PassBlock, Vec::new()),
            OffenseAssignment::LeadBlock => (AssignmentKind::LeadBlock, Vec::new()),
            OffenseAssignment::BallCarry => (AssignmentKind::BallCarry, Vec::new()),
        };
        resolved[id.index()] = Some(ResolvedAssignment { kind, route });
    }

    for (slot, assignment) in play.defense_assignments.iter().enumerate() {
        let id = defense_player(play, slot);
        let kind = match assignment {
            DefenseAssignment::ManCover { target_slot } => AssignmentKind::ManCover {
                target: offense_player(play, *target_slot),
            },
            DefenseAssignment::ZoneCover { center, radius } => AssignmentKind::ZoneCover {
                center: frame.to_world(*center),
                radius: *radius,
            },
            DefenseAssignment::QuarterbackRush => AssignmentKind::QuarterbackRush {
                quarterback: quarterback_id,
            },
            DefenseAssignment::EdgeContain { lateral } => AssignmentKind::EdgeContain {
                post: frame.to_world(crate::field::OffensePoint::new(*lateral, 1.5)),
                quarterback: quarterback_id,
            },
            DefenseAssignment::Pursuit => AssignmentKind::Pursuit,
            DefenseAssignment::TackleTarget => AssignmentKind::TackleTarget,
        };
        resolved[id.index()] = Some(ResolvedAssignment {
            kind,
            route: Vec::new(),
        });
    }

    resolved
        .into_iter()
        .map(|a| {
            a.unwrap_or(ResolvedAssignment {
                kind: AssignmentKind::Pursuit,
                route: Vec::new(),
            })
        })
        .collect()
}
