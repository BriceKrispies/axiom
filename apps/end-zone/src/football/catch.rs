//! Catch resolution, owned by the football subsystem: the intended receiver's
//! deterministic catch, then the defense's play on the ball. The receiver gets
//! first claim (an open target completes the pass); if he does not secure it,
//! the defense plays the ball — a defender who is right on it with tight timing
//! **intercepts** (a turnover), and one who can only get a hand on it as it
//! arrives **swats it down** (a contested incompletion). Interceptions end the
//! run for now; the possession-flip hook lives in the drive layer.

use axiom::prelude::Vec3;

use crate::data::{BehaviorTuning, PlayerArchetype};
use crate::events::{PlayEndReason, SimEvent};
use crate::identity::PlayerId;
use crate::player::{AnimState, PlayerSim};
use crate::state::SimState;

use super::possession::{catch_point, evaluate_catch, CatchVerdict};
use super::state::BallState;
use super::FlightInfo;

/// A defender's play on a ball in the air this tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Contest {
    /// A clean play on the ball — an interception (turnover).
    Intercept,
    /// In the catch volume as the ball arrives but couldn't secure it — a swat.
    Swat,
    /// Not in position to play the ball.
    None,
}

impl SimState {
    /// Deterministic catch resolution: the intended receiver's catch against his
    /// volume, timing window, and action state; then, if he did not secure it,
    /// the defense's play on the ball.
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
        self.resolve_defense_play(flight);
    }

    /// The defense's play on an uncaught ball. The first eligible defender (id
    /// order, so a replay is stable) who can pick it intercepts; else the first
    /// who can get a hand on it swats it down. Nobody in position → the ball
    /// keeps flying and grounds incomplete on its own.
    fn resolve_defense_play(&mut self, flight: FlightInfo) {
        let receiver_team = self.players[flight.intended.index()].team;
        let arrival = flight.arrival_tick();
        let mut interceptor: Option<PlayerId> = None;
        let mut swatter: Option<PlayerId> = None;
        for defender in &self.players {
            if defender.team == receiver_team {
                continue;
            }
            match contest(self.ball.pos, defender, self.tick, arrival, &self.tuning) {
                Contest::Intercept => interceptor = interceptor.or(Some(defender.id)),
                Contest::Swat => swatter = swatter.or(Some(defender.id)),
                Contest::None => {}
            }
        }
        match (interceptor, swatter) {
            (Some(defender), _) => self.intercept(defender),
            (None, Some(defender)) => self.swat(defender),
            (None, None) => {}
        }
    }

    /// A defender picked the pass off: possession passes to him and the ball is
    /// secured. The play ends as an interception — the run ends on it for now
    /// (the future possession flip re-spots the drive for the new offense from
    /// here instead of ending).
    fn intercept(&mut self, defender: PlayerId) {
        let position = self.ball.pos;
        self.players[defender.index()].set_anim(AnimState::Catch);
        self.ball.state = BallState::Held { carrier: defender };
        self.ball.spin_rate = 0.0;
        self.possession = Some(defender);
        self.rig.park_ball();
        self.events.emit(SimEvent::Intercepted { defender, position });
        self.events.emit(SimEvent::PossessionChanged {
            from: None,
            to: Some(defender),
        });
        self.end_play(PlayEndReason::Intercepted);
    }

    /// A defender got a hand on the ball but could not secure it: the pass is
    /// knocked loose and falls incomplete.
    fn swat(&mut self, defender: PlayerId) {
        let position = self.ball.pos;
        self.ball.state = BallState::Loose;
        self.events.emit(SimEvent::PassBrokenUp { defender, position });
        self.events.emit(SimEvent::BallLoose { position });
    }
}

/// Classify one defender's play on the ball this tick. An interception needs a
/// tight radius AND tight timing (a genuine play on the ball); a swat only needs
/// to be in the catch volume as the ball arrives.
fn contest(
    ball_pos: Vec3,
    defender: &PlayerSim,
    tick: u64,
    arrival: u64,
    tuning: &BehaviorTuning,
) -> Contest {
    let archetype: &PlayerArchetype = &defender.archetype;
    if !defender.anim.can_act() {
        return Contest::None;
    }
    let distance = ball_pos.subtract(catch_point(defender.pos)).length();
    // Only a play on the ball AS IT ARRIVES counts (same timing window as a
    // reception); a defender under a ball still sailing overhead does nothing.
    let arriving =
        distance <= archetype.catch_radius && tick.abs_diff(arrival) <= u64::from(archetype.catch_tolerance_ticks);
    if !arriving {
        return Contest::None;
    }
    // Right on the ball → an interception; at the edge of the volume → a swat.
    (distance <= archetype.catch_radius * tuning.interception_radius_scale)
        .then_some(Contest::Intercept)
        .unwrap_or(Contest::Swat)
}
