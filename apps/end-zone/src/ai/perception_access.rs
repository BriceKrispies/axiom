//! The read-only accessors over the perception the AI derived this tick.
//!
//! Split out of [`super::perception`], which owns the derivation, so each file
//! stays narrowly owned. Pure relocation — a second inherent `impl SimState`
//! block, identical methods.

use axiom::prelude::Vec3;
use super::perception::Responsibility;
use crate::config::PLAYER_COUNT;
use crate::data::BehaviorTuning;
use crate::field::{OffenseFrame, GOAL_LINE_Z};
use crate::football::{situation, BallSituation, BallState};
use crate::identity::{PlayerId, TeamId};
use crate::state::SimState;
use super::brain::RoleState;
use super::coordination;
use super::directive::{DefensiveDirective, TacticalMode};
use super::engagement::Engagement;
use super::overseer::PossessionMemory;

impl SimState {
    /// The football situation the AI derived this tick (debug overlay + tests).
    pub fn ball_situation(&self) -> BallSituation {
        self.ai_memory.situation
    }

    /// A defender's coordinated pursuit responsibility this tick.
    pub fn responsibility(&self, player: PlayerId) -> Responsibility {
        self.ai_memory.responsibilities[player.index()]
    }

    /// A player's committed-action debug reason, if committed.
    pub fn commitment_reason(&self, player: PlayerId) -> Option<&'static str> {
        self.ai_memory.commitments[player.index()].map(|c| c.reason)
    }

    /// Ticks of committed action `player` has left before it may freely switch.
    pub fn commitment_ticks_left(&self, player: PlayerId) -> u32 {
        self.ai_memory
            .commitment_ticks_left(player.index(), self.tick)
    }

    /// A blocker's current line engagement, if he is engaged.
    pub fn engagement(&self, blocker: PlayerId) -> Option<Engagement> {
        self.engagements[blocker.index()]
    }

    /// The overseer's active defensive directive (debug overlay + tests).
    pub fn directive(&self) -> DefensiveDirective {
        self.overseer.directive
    }

    /// The overseer's possession-level tendency memory (debug + tests).
    pub fn overseer_memory(&self) -> PossessionMemory {
        self.overseer.memory
    }

    /// The overseer's previous mode and the reason it last transitioned.
    pub fn overseer_transition(&self) -> (TacticalMode, &'static str) {
        (self.overseer.prev_mode(), self.overseer.transition_reason())
    }

    /// The top rejected tactical alternative and its score (debug).
    pub fn overseer_rejected(&self) -> (TacticalMode, f32) {
        self.overseer.rejected()
    }

    /// Reset possession-level overseer memory at a possession boundary.
    pub fn note_new_possession(&mut self) {
        self.overseer.reset_possession();
    }
}

pub(super) fn flat(v: Vec3) -> Vec3 {
    Vec3::new(v.x, 0.0, v.z)
}
