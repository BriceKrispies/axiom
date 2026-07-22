//! The drawable shape of an offensive play: each player's alignment, role, and
//! route as a chain of offense-relative points. This is a neutral geometric
//! view — no screen or scene types — so the browser-free frontend can draw it as
//! a chalkboard and the presentation layer can draw the same lines on the field
//! (each point mapped through [`crate::field::OffenseFrame`]). One source of
//! truth for "the desired play," rendered on two surfaces.

use crate::field::OffensePoint;

use super::play::{OffenseAssignment, OffensivePlay};

/// What a diagrammed player does — drives the mark's glyph and color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagramRole {
    Quarterback,
    Snapper,
    Blocker,
    Receiver,
    Carrier,
}

/// One player's mark on the diagram: where he lines up, his role, and his route
/// as absolute offense-relative points (empty for players with no route). The
/// route always begins at `align`, so a renderer draws a single polyline.
#[derive(Debug, Clone, PartialEq)]
pub struct DiagramMark {
    pub roster_slot: usize,
    pub align: OffensePoint,
    pub role: DiagramRole,
    /// The primary read — a renderer highlights it.
    pub primary: bool,
    /// A decoy/clear-out route — a renderer may dash it.
    pub decoy: bool,
    /// Absolute offense-relative route points, starting at `align`.
    pub route: Vec<OffensePoint>,
}

/// The full drawable play: a name and one mark per player.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayDiagram {
    pub name: &'static str,
    pub marks: Vec<DiagramMark>,
}

impl PlayDiagram {
    /// Build the diagram for an offensive play. The primary read is the
    /// highest-slot live route (the convention the playbook authors to).
    pub fn of(play: &OffensivePlay) -> Self {
        let primary_slot = play
            .assignments
            .iter()
            .enumerate()
            .filter(|(_, a)| matches!(a, OffenseAssignment::Route(_)))
            .map(|(i, _)| i)
            .max();

        let marks = play
            .assignments
            .iter()
            .enumerate()
            .map(|(slot, assignment)| {
                let align = play.formation.slots[slot].position;
                mark_for(slot, align, assignment, primary_slot == Some(slot))
            })
            .collect();

        PlayDiagram {
            name: play.name,
            marks,
        }
    }
}

/// Prepend the alignment to a compiled route so the polyline starts at the
/// receiver's spot.
fn from_align(align: OffensePoint, tail: Vec<OffensePoint>) -> Vec<OffensePoint> {
    std::iter::once(align).chain(tail).collect()
}

fn mark_for(
    roster_slot: usize,
    align: OffensePoint,
    assignment: &OffenseAssignment,
    primary: bool,
) -> DiagramMark {
    let (role, decoy, route) = match assignment {
        OffenseAssignment::Quarterback { drop_depth } => (
            DiagramRole::Quarterback,
            false,
            vec![
                align,
                OffensePoint::new(align.lateral, align.downfield - drop_depth),
            ],
        ),
        OffenseAssignment::Snapper => (DiagramRole::Snapper, false, Vec::new()),
        OffenseAssignment::PassBlock | OffenseAssignment::LeadBlock => {
            (DiagramRole::Blocker, false, Vec::new())
        }
        OffenseAssignment::Route(def) => {
            (DiagramRole::Receiver, false, from_align(align, def.waypoints(align)))
        }
        OffenseAssignment::DecoyRoute(def) => {
            (DiagramRole::Receiver, true, from_align(align, def.waypoints(align)))
        }
        OffenseAssignment::BallCarry => (DiagramRole::Carrier, false, Vec::new()),
    };
    DiagramMark {
        roster_slot,
        align,
        role,
        primary,
        decoy,
        route,
    }
}
