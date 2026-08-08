//! The orchestrator's contact stage, owned by the player subsystem: run the
//! blocking engagements, the tackle, diving commits, and completed falls, then
//! turn their outcomes into ordered [`SimEvent`]s and the play-end. Split out of
//! [`crate::state`] so the tick orchestrator stays narrowly owned (the same
//! pattern as [`crate::ai::stage`] and [`crate::football::sim`]).

use crate::ai::engagement;
use crate::config::DT;
use crate::events::{PlayEndReason, SimEvent};
use crate::player::contact;
use crate::state::SimState;

impl SimState {
    /// Contact resolution: blocks (announced once per pairing per play),
    /// the tackle, and completed falls → ordered events + play end.
    pub(crate) fn resolve_contacts(&mut self) {
        // The point the blockers protect: the ball-holder, else the ball's spot.
        let protect = self
            .possession
            .map(|id| self.players[id.index()].pos)
            .unwrap_or(self.ball.pos);
        let blocks = engagement::advance_engagements(
            &mut self.engagements,
            &mut self.players,
            &self.intents,
            protect,
            &self.tuning,
        );
        for pair in &blocks {
            if !self.engaged_blocks.contains(pair) {
                self.engaged_blocks.push(*pair);
                self.events.emit(SimEvent::BlockEngaged {
                    blocker: pair.0,
                    defender: pair.1,
                });
            }
        }

        // A fresh ball carrier is briefly securing the catch and cannot be
        // tackled yet — a contested catch gets a beat, not an instant swarm.
        let securing =
            self.tick.saturating_sub(self.possession_since) < u64::from(self.tuning.catch_secure_ticks);
        let carrier = self.ball.carrier().filter(|_| !securing);
        let charging = self.runback.charging(self.tick);
        let resolution = contact::resolve_tackle(
            &mut self.players,
            &self.intents,
            carrier,
            charging,
            &self.tuning,
            &self.collision,
        );
        // Every attempt he survived, in tackler order, before whichever one
        // finally got him.
        for contest in &resolution.shed {
            let balance_left = self.players[contest.target.index()].balance;
            self.events.emit(SimEvent::TackleShed {
                tackler: contest.tackler,
                runner: contest.target,
                impulse: contest.impulse,
                resistance: contest.resistance,
                balance_left,
            });
        }
        match resolution.landed {
            Some(outcome) => {
                self.events.emit(SimEvent::TackleContact {
                    tackler: outcome.tackler,
                    target: outcome.target,
                    contact_point: outcome.contact_point,
                    contact_direction: outcome.contact_direction,
                    relative_speed: outcome.relative_speed,
                    strength: outcome.strength,
                    target_airborne: outcome.target_airborne,
                });
                if outcome.target_airborne {
                    self.events.emit(SimEvent::PlayerAirborne {
                        player: outcome.target,
                    });
                }
            }
            // Nobody got him — let close, fast chasers leave their feet. Gated on
            // NO contact at all this tick, so a defender who was just shed is not
            // immediately dived over by the next man before the shed reads.
            None if !resolution.any_contact() => {
                contact::commit_dives(&mut self.players, &self.intents, carrier, &self.tuning)
            }
            None => {}
        }

        for (player, strength) in contact::advance_falls(&mut self.players, &self.tuning, DT) {
            let position = self.players[player.index()].pos;
            self.events.emit(SimEvent::GroundImpact {
                player,
                position,
                strength,
            });
            if self.ball.carrier() == Some(player) {
                self.end_play(PlayEndReason::Tackled);
            }
        }
    }
}
