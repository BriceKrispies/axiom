//! The football's simulation flow, owned by the football subsystem: the snap
//! lerp, the held-ball socket, the scripted release, the physics-integrated
//! flight, catch resolution, and the loose/grounded transitions — all as
//! orchestrator stages on [`SimState`].

use axiom::prelude::Vec3;

use crate::ai::{AssignmentKind, RoleState};
use crate::config::DT;
use crate::events::{PlayEndReason, SimEvent};
use crate::identity::PlayerId;
use crate::player::AnimState;
use crate::state::{PlayPhase, SimState};

use super::possession::{catch_point, evaluate_catch, CatchVerdict};
use super::state::{BallState, BALL_RADIUS};
use super::{carry_socket, solve_throw, FlightInfo};

impl SimState {
    /// Snap the dead ball toward the quarterback and go live.
    pub(crate) fn snap(&mut self) {
        if self.phase != PlayPhase::PreSnap || !matches!(self.ball.state, BallState::Dead) {
            return;
        }
        let snapper = self
            .assignments
            .iter()
            .enumerate()
            .find(|(_, a)| matches!(a.kind, AssignmentKind::Snapper))
            .map(|(i, _)| PlayerId(i as u8))
            .unwrap_or(self.quarterback);
        self.ball.state = BallState::Snap {
            from: snapper,
            to: self.quarterback,
            start: self.ball.pos,
            elapsed: 0,
            total: self.tuning.snap_ticks,
        };
        self.phase = PlayPhase::Live;
        self.events.emit(SimEvent::Snap {
            snapper,
            quarterback: self.quarterback,
        });
    }

    /// Pre-physics ball update: held sockets, the snap lerp, the release.
    pub(crate) fn ball_pre_physics(&mut self) {
        match self.ball.state {
            BallState::Dead | BallState::Grounded => {}
            BallState::Held { carrier } => {
                let holder = &self.players[carrier.index()];
                self.ball.pos = carry_socket(holder.pos, holder.facing, holder.anim);
                self.maybe_release(carrier);
            }
            BallState::Snap {
                from,
                to,
                start,
                elapsed,
                total,
            } => {
                let target_player = &self.players[to.index()];
                let target =
                    carry_socket(target_player.pos, target_player.facing, target_player.anim);
                let t = (elapsed + 1) as f32 / total.max(1) as f32;
                self.ball.pos = Vec3::new(
                    start.x + (target.x - start.x) * t,
                    start.y + (target.y - start.y) * t,
                    start.z + (target.z - start.z) * t,
                );
                if elapsed + 1 >= total {
                    self.ball.state = BallState::Held { carrier: to };
                    self.possession = Some(to);
                    self.events.emit(SimEvent::PossessionChanged {
                        from: None,
                        to: Some(to),
                    });
                    self.events.emit(SimEvent::DropBack { quarterback: to });
                } else {
                    self.ball.state = BallState::Snap {
                        from,
                        to,
                        start,
                        elapsed: elapsed + 1,
                        total,
                    };
                }
            }
            BallState::Airborne { .. } | BallState::Loose => {}
        }
    }

    /// Release the scripted pass once the quarterback's wind-up completes:
    /// deterministic release point + velocity, real ballistic flight through
    /// the physics body — never a teleport.
    fn maybe_release(&mut self, carrier: PlayerId) {
        let RoleState::QbWindup { since } = self.roles[carrier.index()] else {
            return;
        };
        if self.tick.saturating_sub(since) < u64::from(self.tuning.throw_windup_ticks) {
            return;
        }
        let AssignmentKind::Quarterback { throw_to, .. } = self.assignments[carrier.index()].kind
        else {
            return;
        };
        let qb = &self.players[carrier.index()];
        let release = carry_socket(qb.pos, qb.facing, AnimState::Throw);
        // Predict the receiver twice (eta depends on distance; two passes fix it).
        let receiver = &self.players[throw_to.index()];
        let mut target = catch_point(receiver.pos);
        for _ in 0..2 {
            let (_, eta) = solve_throw(release, target, &self.tuning);
            let lead = receiver.vel.mul_scalar(eta as f32 / 60.0);
            target = catch_point(receiver.pos.add(lead));
        }
        let (velocity, eta_ticks) = solve_throw(release, target, &self.tuning);
        let flight = FlightInfo {
            intended: throw_to,
            release,
            velocity,
            target,
            release_tick: self.tick,
            eta_ticks,
        };
        let axis = velocity.normalize().unwrap_or(Vec3::UNIT_Z);
        self.ball.state = BallState::Airborne { flight };
        self.ball.pos = release;
        self.ball.vel = velocity;
        self.ball.flight_axis = axis;
        self.ball.spin_rate = 19.0;
        self.rig
            .launch_ball(release, velocity, axis.mul_scalar(self.ball.spin_rate));
        self.roles[carrier.index()] = RoleState::QbDone;
        self.possession = None;
        self.catch_attempted = false;
        self.events.emit(SimEvent::Throw {
            quarterback: carrier,
            release,
            velocity,
            target,
            eta_ticks,
        });
        self.events.emit(SimEvent::PossessionChanged {
            from: Some(carrier),
            to: None,
        });
    }

    /// Post-physics ball update: read the integrated flight, resolve the
    /// catch, and run the loose → grounded transitions.
    pub(crate) fn ball_post_physics(&mut self) {
        match self.ball.state {
            BallState::Airborne { flight } => {
                if let Some((pos, vel)) = self.rig.ball_state() {
                    self.ball.pos = pos;
                    self.ball.vel = vel;
                }
                self.ball.spin_angle += self.ball.spin_rate * DT;
                self.resolve_catch(flight);
                if matches!(self.ball.state, BallState::Airborne { .. })
                    && self.ball.pos.y <= BALL_RADIUS * 1.1
                    && self.tick > flight.release_tick + 2
                {
                    self.ball.state = BallState::Loose;
                    self.events.emit(SimEvent::BallLoose {
                        position: self.ball.pos,
                    });
                }
            }
            BallState::Loose => {
                if let Some((pos, vel)) = self.rig.ball_state() {
                    self.ball.pos = pos;
                    self.ball.vel = vel;
                }
                if self.ball.vel.length() < 0.7 && self.ball.pos.y <= BALL_RADIUS * 1.25 {
                    self.ball.state = BallState::Grounded;
                    self.rig.park_ball();
                    self.events.emit(SimEvent::BallGrounded {
                        position: self.ball.pos,
                    });
                    self.end_play(PlayEndReason::Incomplete);
                }
            }
            _ => {}
        }
    }

    /// Deterministic catch resolution against the intended receiver's catch
    /// volume, timing tolerance, and action state.
    fn resolve_catch(&mut self, flight: FlightInfo) {
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
        }
    }
}
