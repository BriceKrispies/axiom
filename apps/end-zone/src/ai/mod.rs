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

pub mod assignment;
pub mod brain;
pub mod defense;
pub mod offense;
pub mod stage;
pub mod steering;

pub use assignment::{compile_assignments, AssignmentKind, ResolvedAssignment};
pub use brain::{decide, BrainCtx, Perception, PerceptionFrame, RoleState};

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
    /// Block a specific opponent at a working point.
    Block { target: PlayerId, point: Vec3 },
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
}
