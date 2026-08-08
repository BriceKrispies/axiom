//! The orchestrator's AI stage, owned by the AI subsystem: build the shared
//! decision context, run every brain in ascending id order, and record this
//! tick's world into the delayed perception ring.

use axiom::prelude::Vec3;

use crate::config::PLAYER_COUNT;
use crate::field::{OffensePoint, FIELD_HALF_WIDTH};
use crate::football::BallState;
use crate::state::{PlayPhase, SimState};

use super::brain::{decide, BrainCtx, PerceptionFrame};

impl SimState {
    /// Stage 2 of the AI pipeline: derive the shared situation, build the shared
    /// play perception (which coordinates defensive responsibilities), then emit
    /// one arbitrated intent per player in ascending id order, and finally let a
    /// live user stick overwrite the ball-holder's intent.
    pub(crate) fn decide_intents(&mut self) {
        let situation = self.update_ai_situation();
        let mut perception = self.build_play_perception(situation);
        // The overseer watches the whole play and issues one team-level directive
        // the individual defenders execute; it never steers a player.
        let directive = self.overseer.update(self.tick, &perception, &self.players);
        perception.directive = directive;
        self.ai_memory.responsibilities = perception.responsibilities;
        let end_zone_target = self
            .frame
            .to_world(OffensePoint::new(0.0, 0.0))
            .add(self.frame.forward().mul_scalar(80.0));
        let controlled = self.controlled_player();
        let ctx = BrainCtx {
            tick: self.tick,
            live: self.phase == PlayPhase::Live,
            pre_snap: self.phase == PlayPhase::PreSnap,
            tuning: &self.tuning,
            ball: &self.ball,
            possession: self.possession,
            players: &self.players,
            perception: &self.perception,
            per: &perception,
            engagements: &self.engagements,
            quarterback: self.quarterback,
            end_zone_target: Vec3::new(
                end_zone_target
                    .x
                    .clamp(-FIELD_HALF_WIDTH + 8.0, FIELD_HALF_WIDTH - 8.0),
                0.0,
                end_zone_target.z,
            ),
            frame: self.frame,
            throw_commanded: self.throw_commanded,
        };
        let mut intents = Vec::with_capacity(PLAYER_COUNT);
        let mut roles = self.roles.clone();
        let mut commitments = self.ai_memory.commitments;
        for index in 0..PLAYER_COUNT {
            let user_controlled = controlled == Some(self.players[index].id);
            let intent = decide(
                &self.players[index],
                &self.assignments[index],
                &mut roles[index],
                &mut commitments[index],
                &ctx,
                user_controlled,
            );
            intents.push(intent);
        }
        drop(ctx);
        self.roles = roles;
        self.ai_memory.commitments = commitments;
        self.intents = intents;
    }

    /// The player the user's controls act on: the ball holder while the OFFENSE
    /// has possession in a live play. `None` otherwise — the ball mid-exchange,
    /// the defense, a downed carrier, or a dead play are never user-driven.
    ///
    /// Note what it does *not* do any more: it no longer hands anyone a
    /// movement stick. The carrier's path is the AI's (see
    /// [`crate::ai::carry`]); what the player owns is
    /// [`crate::runback::RunbackMove`], and only when this resolves to the
    /// running back.
    pub fn controlled_player(&self) -> Option<crate::identity::PlayerId> {
        self.possession
            .filter(|_| self.phase == PlayPhase::Live)
            .filter(|id| {
                let player = &self.players[id.index()];
                player.team == self.play.possession && player.anim.can_act()
            })
    }

    /// Record this tick's true world state into the perception ring the
    /// defenders sample with their configured reaction delays.
    pub(crate) fn push_perception(&mut self) {
        let mut frame = PerceptionFrame {
            positions: [Vec3::ZERO; PLAYER_COUNT],
            velocities: [Vec3::ZERO; PLAYER_COUNT],
            ball_pos: self.ball.pos,
            ball_airborne: self.ball.is_airborne(),
            ball_target: match self.ball.state {
                BallState::Airborne { flight } => flight.target,
                _ => self.ball.pos,
            },
            carrier: self.ball.carrier(),
        };
        for (index, player) in self.players.iter().enumerate() {
            frame.positions[index] = player.pos;
            frame.velocities[index] = player.vel;
        }
        self.perception.push(frame);
    }
}
