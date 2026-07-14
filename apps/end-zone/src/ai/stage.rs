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
    /// Stage 2 of the AI pipeline: one typed intent per player, id order.
    pub(crate) fn decide_intents(&mut self) {
        let end_zone_target = self
            .frame
            .to_world(OffensePoint::new(0.0, 0.0))
            .add(self.frame.forward().mul_scalar(80.0));
        let ctx = BrainCtx {
            tick: self.tick,
            live: self.phase == PlayPhase::Live,
            tuning: &self.tuning,
            ball: &self.ball,
            possession: self.possession,
            players: &self.players,
            perception: &self.perception,
            quarterback: self.quarterback,
            end_zone_target: Vec3::new(
                end_zone_target
                    .x
                    .clamp(-FIELD_HALF_WIDTH + 8.0, FIELD_HALF_WIDTH - 8.0),
                0.0,
                end_zone_target.z,
            ),
            throw_commanded: self.throw_commanded,
        };
        let mut intents = Vec::with_capacity(PLAYER_COUNT);
        let mut roles = self.roles.clone();
        for index in 0..PLAYER_COUNT {
            let intent = decide(
                &self.players[index],
                &self.assignments[index],
                &mut roles[index],
                &ctx,
            );
            intents.push(intent);
        }
        drop(ctx);
        self.roles = roles;
        self.intents = intents;
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
