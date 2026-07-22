//! Catch resolution, owned by the football subsystem: the intended receiver's
//! deterministic catch, and break-up-only interceptions (a defender who reaches
//! the ball inside its catch window knocks it down — the pass falls incomplete
//! with no possession change; true turnovers are a later pass).

use crate::events::SimEvent;
use crate::player::AnimState;
use crate::state::SimState;

use super::possession::{evaluate_catch, CatchVerdict};
use super::state::BallState;
use super::FlightInfo;

impl SimState {
    /// Deterministic catch resolution against the intended receiver's catch
    /// volume, timing tolerance, and action state; then a break-up check.
    pub(crate) fn resolve_catch(&mut self, flight: FlightInfo) {
        let receiver = &self.players[flight.intended.index()];
        let verdict = evaluate_catch(
            self.ball.pos,
            receiver.pos,
            &receiver.archetype,
            self.tick,
            flight.arrival_tick(),
            receiver.anim.can_act(),
        );
        if verdict != CatchVerdict::OutOfReach && !self.catch_attempted {
            self.catch_attempted = true;
            self.events.emit(SimEvent::CatchAttempt {
                player: flight.intended,
            });
        }
        if verdict == CatchVerdict::Caught {
            let receiver = &mut self.players[flight.intended.index()];
            receiver.set_anim(AnimState::Catch);
            self.ball.state = BallState::Held {
                carrier: flight.intended,
            };
            self.ball.spin_rate = 0.0;
            self.possession = Some(flight.intended);
            self.rig.park_ball();
            self.events.emit(SimEvent::CatchCompleted {
                player: flight.intended,
            });
            self.events.emit(SimEvent::PossessionChanged {
                from: None,
                to: Some(flight.intended),
            });
            return;
        }
        self.resolve_break_up(flight);
    }

    /// Break-up-only interception: the first eligible defender (id order, so a
    /// replay is stable) inside the ball's catch window knocks it loose.
    fn resolve_break_up(&mut self, flight: FlightInfo) {
        let receiver_team = self.players[flight.intended.index()].team;
        let breaker = self.players.iter().find(|d| {
            d.team != receiver_team
                && evaluate_catch(
                    self.ball.pos,
                    d.pos,
                    &d.archetype,
                    self.tick,
                    flight.arrival_tick(),
                    d.anim.can_act(),
                ) == CatchVerdict::Caught
        });
        if let Some(defender) = breaker {
            let defender = defender.id;
            let position = self.ball.pos;
            self.ball.state = BallState::Loose;
            self.events.emit(SimEvent::PassBrokenUp { defender, position });
            self.events.emit(SimEvent::BallLoose { position });
        }
    }
}
