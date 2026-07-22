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
        let perception = self.build_play_perception(situation);
        self.ai_memory.responsibilities = perception.responsibilities;
        let end_zone_target = self
            .frame
            .to_world(OffensePoint::new(0.0, 0.0))
            .add(self.frame.forward().mul_scalar(80.0));
        let controlled = self.controlled_player();
        let ctx = BrainCtx {
            tick: self.tick,
            live: self.phase == PlayPhase::Live,
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
        self.apply_user_stick();
    }

    /// The player the user steers: the ball holder while the OFFENSE has
    /// possession in a live play (the quarterback pre-throw, the receiver
    /// after the catch). `None` otherwise — the ball in flight, the defense,
    /// a downed carrier, or a dead play are never user-driven.
    pub fn controlled_player(&self) -> Option<crate::identity::PlayerId> {
        self.possession
            .filter(|_| self.phase == PlayPhase::Live)
            .filter(|id| {
                let player = &self.players[id.index()];
                player.team == self.play.possession && player.anim.can_act()
            })
    }

    /// A live stick past the dead zone replaces the controlled player's AI
    /// intent with a movement intent (the controller still applies every
    /// acceleration/turn-rate/boundary limit). Stick `x` is toward the
    /// offense's right hand, `y` is downfield — matching the follow cameras,
    /// which look downfield from behind the offense.
    fn apply_user_stick(&mut self) {
        const DEAD_ZONE: f32 = 0.18;
        const REACH: f32 = 4.0;
        let stick = self.user_stick;
        let magnitude = (stick.x * stick.x + stick.y * stick.y).sqrt();
        if magnitude <= DEAD_ZONE {
            return;
        }
        let Some(id) = self.controlled_player() else {
            return;
        };
        let clamped = magnitude.min(1.0);
        let direction = self
            .frame
            .right()
            .mul_scalar(stick.x)
            .add(self.frame.forward().mul_scalar(stick.y));
        let length = direction.length();
        if length <= 1.0e-4 {
            return;
        }
        let player = &self.players[id.index()];
        let point = player
            .pos
            .add(direction.mul_scalar(REACH * clamped / length));
        let point = Vec3::new(point.x, 0.0, point.z);
        let sprint = clamped > 0.55;
        // The passer is steered differently from a ball carrier: he STRAFES.
        // A carrier turns to run where he is going, but a quarterback keeps his
        // eyes downfield — the stick slides him around the pocket and only
        // swings his aim within a bounded forward arc, so he never spins away
        // from the play (and the throwing cone stays usable).
        self.intents[id.index()] = match id == self.quarterback {
            true => super::PlayerIntent::DropBack {
                point,
                face: self.passer_aim(stick.x),
                sprint,
            },
            false => super::PlayerIntent::MoveToward { point, sprint },
        };
    }

    /// The direction a steered quarterback faces for a given lateral stick.
    /// Straight downfield at centre, swinging to at most `qb_aim_max_yaw` off
    /// it at full deflection — the forward arc his facing is clamped to.
    fn passer_aim(&self, lateral: f32) -> Vec3 {
        let yaw = lateral.clamp(-1.0, 1.0) * self.tuning.qb_aim_max_yaw;
        self.frame
            .forward()
            .mul_scalar(yaw.cos())
            .add(self.frame.right().mul_scalar(yaw.sin()))
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
