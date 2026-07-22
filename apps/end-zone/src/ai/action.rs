//! The scored-action vocabulary the unified brain arbitrates over. Every role
//! produces a few [`ScoredAction`]s; the arbiter ([`super::commitment`]) picks
//! one under commitment locking and hands its [`PlayerIntent`] to the
//! controller — so the execution vocabulary is unchanged, only the *decision*
//! is now a scored contest instead of a per-role state machine.

use super::PlayerIntent;

/// The universal football priority bands (spec §3), lowest to highest. A
/// candidate's band dominates its within-band urgency, so a ball-threat action
/// always outranks a shape action no matter their urgencies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    /// Recover to a useful football position (down/reset).
    Recover = 0,
    /// Preserve leverage and team shape.
    Leverage = 1,
    /// Execute the current positional assignment.
    Assignment = 2,
    /// Prevent an imminent touchdown or major gain.
    PreventScore = 3,
    /// Respond to an immediate ball threat.
    BallThreat = 4,
}

impl Priority {
    /// The numeric band used to compose a total score.
    pub fn band(self) -> f32 {
        self as u8 as f32
    }
}

/// One candidate action an arbiter may pick: a concrete [`PlayerIntent`] plus
/// the priority band, the within-band urgency, a debug reason, and the minimum
/// number of ticks the picker should stay committed to it (hysteresis).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScoredAction {
    pub intent: PlayerIntent,
    pub priority: Priority,
    /// Within-band urgency `0..=1`.
    pub urgency: f32,
    pub reason: &'static str,
    pub min_ticks: u32,
}

impl ScoredAction {
    /// A candidate action.
    pub fn new(
        intent: PlayerIntent,
        priority: Priority,
        urgency: f32,
        reason: &'static str,
        min_ticks: u32,
    ) -> Self {
        ScoredAction {
            intent,
            priority,
            urgency: urgency.clamp(0.0, 1.0),
            reason,
            min_ticks,
        }
    }

    /// The total score: the band dominates, urgency breaks within-band ties.
    pub fn score(&self) -> f32 {
        self.priority.band() * 1000.0 + self.urgency.clamp(0.0, 1.0) * 999.0
    }
}
