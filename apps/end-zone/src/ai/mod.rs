//! Data-driven, deterministic, inspectable AI. Three stages, one direction:
//!
//! 1. **Assignment evaluation** ([`assignment`]) — the play's per-slot data is
//!    resolved to per-player assignments (routes compiled to world waypoints).
//! 2. **Intent** ([`brain`], [`offense`], [`defense`]) — small per-role state
//!    machines emit a typed [`PlayerIntent`] each tick.
//! 3. **Execution** ([`crate::player::controller`]) — the only code that turns
//!    intents into movement, under acceleration/turn-rate limits.
//!
//! AI never writes a transform, never mutates the ball, and never touches
//! another subsystem's state. Defenders read *perceived* (delayed) state per
//! their archetype's reaction delay rather than mirroring targets instantly.

pub mod action;
pub mod assignment;
pub mod brain;
pub mod commitment;
pub mod coordination;
pub mod defense;
pub mod engagement;
pub mod offense;
pub mod perception;
pub mod protection;
pub mod stage;
pub mod steering;

pub use action::{Priority, ScoredAction};
pub use assignment::{compile_assignments, AssignmentKind, ResolvedAssignment};
pub use brain::{decide, BrainCtx, Perception, PerceptionFrame, RoleState};
pub use commitment::{AiMemory, Commitment};
pub use perception::{PlayPerception, PocketRegion, Responsibility};

use axiom::prelude::Vec3;

use crate::identity::PlayerId;

/// A typed movement/action intent — the ONLY thing AI may output.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlayerIntent {
    /// Stand (ready stance / idle).
    Hold,
    /// Stand and face a direction.
    Face { direction: Vec3 },
    /// Move toward a point (`sprint` selects the speed tier).
    MoveToward { point: Vec3, sprint: bool },
    /// Move to a point while HOLDING a facing — the quarterback's drop-back and
    /// his steered pocket movement. Distinct from `MoveToward` because the whole
    /// point of a backpedal or a strafe is that the mover does NOT turn to face
    /// where he is going: he keeps his eyes downfield, which is what makes him
    /// able to throw (and what makes the controller play the backpedal
    /// animation).
    DropBack {
        point: Vec3,
        face: Vec3,
        sprint: bool,
    },
    /// Block a specific opponent: move to a leverage `point` (pocket-side of the
    /// rusher) while squaring the body to `face` him. The explicit facing is
    /// what lets a blocker wall and anchor instead of chasing/circling.
    Block {
        target: PlayerId,
        point: Vec3,
        face: Vec3,
    },
    /// Chase an opponent via a predicted interception point.
    Pursue { target: PlayerId, point: Vec3 },
    /// Settle under a pass at its predicted arrival point.
    PrepareCatch { point: Vec3 },
    /// Quarterback wind-up (the release is resolved by the simulation).
    Throw,
    /// Carry the ball toward a point (the end zone).
    Carry { point: Vec3 },
    /// Close on the carrier for a tackle.
    Tackle { target: PlayerId, point: Vec3 },
    /// Down/recovering — no movement.
    Recover,
}

impl PlayerIntent {
    /// The movement target this intent implies, if any, plus its speed tier.
    pub fn movement(&self) -> Option<(Vec3, bool)> {
        match *self {
            PlayerIntent::MoveToward { point, sprint } => Some((point, sprint)),
            PlayerIntent::DropBack { point, sprint, .. } => Some((point, sprint)),
            PlayerIntent::Block { point, .. } => Some((point, false)),
            PlayerIntent::Pursue { point, .. } => Some((point, true)),
            PlayerIntent::PrepareCatch { point } => Some((point, true)),
            PlayerIntent::Carry { point } => Some((point, true)),
            PlayerIntent::Tackle { point, .. } => Some((point, true)),
            PlayerIntent::Hold
            | PlayerIntent::Face { .. }
            | PlayerIntent::Throw
            | PlayerIntent::Recover => None,
        }
    }

    /// Whether this intent is a committed chase on an opponent — pursuit or a
    /// tackle close-in. Such movement runs FLAT OUT (`seek`, no arrival easing)
    /// so the chaser keeps sprinting into contact instead of gliding in.
    pub fn closes_hard(&self) -> bool {
        matches!(
            self,
            PlayerIntent::Pursue { .. } | PlayerIntent::Tackle { .. }
        )
    }

    /// The opponent this intent acts on, if any (drives contact evaluation
    /// and the debug overlay).
    pub fn action_target(&self) -> Option<PlayerId> {
        match *self {
            PlayerIntent::Block { target, .. }
            | PlayerIntent::Pursue { target, .. }
            | PlayerIntent::Tackle { target, .. } => Some(target),
            _ => None,
        }
    }

    /// Whether two intents are the *same committed action* — same kind, and same
    /// opponent for the targeted kinds — ignoring the working point (which is
    /// refreshed while the commitment holds). This is what lets the arbiter keep
    /// tracking a moving target without re-committing every tick.
    pub fn same_action(&self, other: &PlayerIntent) -> bool {
        use PlayerIntent::*;
        match (self, other) {
            (Hold, Hold) | (Throw, Throw) | (Recover, Recover) => true,
            (Face { .. }, Face { .. }) => true,
            (MoveToward { .. }, MoveToward { .. }) => true,
            (DropBack { .. }, DropBack { .. }) => true,
            (PrepareCatch { .. }, PrepareCatch { .. }) => true,
            (Carry { .. }, Carry { .. }) => true,
            (Block { target: a, .. }, Block { target: b, .. }) => a == b,
            (Pursue { target: a, .. }, Pursue { target: b, .. }) => a == b,
            (Tackle { target: a, .. }, Tackle { target: b, .. }) => a == b,
            _ => false,
        }
    }
}
