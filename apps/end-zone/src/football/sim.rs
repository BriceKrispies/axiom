//! The football's simulation flow, owned by the football subsystem: the snap
//! lerp, the held-ball socket, the scripted release, the physics-integrated
//! flight, catch resolution, and the loose/grounded transitions — all as
//! orchestrator stages on [`SimState`].

use axiom::prelude::Vec3;

use crate::ai::{AssignmentKind, RoleState};
use crate::config::DT;
use crate::events::{PlayEndReason, SimEvent};
use crate::field::{FIELD_HALF_WIDTH, GOAL_LINE_Z};
use crate::identity::PlayerId;
use crate::player::AnimState;
use crate::state::{PlayPhase, SimState};

/// The ball's resting offset above the turf when dead at the spot.
pub(crate) fn ball_rest() -> Vec3 {
    Vec3::new(0.0, BALL_RADIUS, 0.0)
}

use super::possession::catch_point;
use super::targeting;
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
        // Lock the target on the first tick of the wind-up: the pass commits to
        // whoever the quarterback was aiming at when the player pressed throw,
        // so a defender crossing the cone mid-wind-up cannot steal the read.
        if self.throw_target.is_none() {
            let picks = {
                let qb = &self.players[carrier.index()];
                targeting::candidates(qb, &self.players, &self.assignments, &self.tuning)
            };
            let Some(target) = targeting::best(&picks) else {
                // Nobody in the cone — there is no pass to make. Drop out of the
                // wind-up so the quarterback keeps scanning (and stays sackable)
                // instead of freezing mid-throw with no receiver.
                self.roles[carrier.index()] = RoleState::QbScan;
                return;
            };
            self.throw_target = Some(target);
        }
        if self.tick.saturating_sub(since) < u64::from(self.tuning.throw_windup_ticks) {
            return;
        }
        let Some(throw_to) = self.throw_target else {
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
        self.throw_target = None;
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

    /// Recompute this tick's eligible receivers: everyone inside the
    /// quarterback's throwing cone while he holds a live ball. This is the
    /// single owner of the eligibility rule — presentation only reads the
    /// resulting list, so the rings a player sees can never disagree with who
    /// the ball would actually go to.
    pub(crate) fn update_throwable(&mut self) {
        self.throwable.clear();
        let Some(carrier) = self.ball.carrier() else {
            return;
        };
        let scanning = !matches!(self.roles[carrier.index()], RoleState::QbDone);
        if carrier != self.quarterback || !scanning {
            return;
        }
        let picks = {
            let qb = &self.players[carrier.index()];
            targeting::candidates(qb, &self.players, &self.assignments, &self.tuning)
        };
        self.throwable = picks.iter().map(|c| c.id).collect();
    }

    /// Post-physics ball update: read the integrated flight, resolve the catch,
    /// and — the instant an uncaught forward pass touches the turf — blow the
    /// play dead as an incompletion.
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
                    self.ground_incomplete();
                }
            }
            // A deflected (broken-up) pass falls and is dead on ground contact.
            BallState::Loose => {
                if let Some((pos, vel)) = self.rig.ball_state() {
                    self.ball.pos = pos;
                    self.ball.vel = vel;
                }
                if self.ball.pos.y <= BALL_RADIUS * 1.25 {
                    self.ground_incomplete();
                }
            }
            _ => {}
        }
    }

    /// A forward pass hit the ground uncaught: the down is over. Real-football
    /// rule — the play is dead the moment the ball touches the turf, and the
    /// ball returns to the previous line of scrimmage (so the offense keeps its
    /// spot; `ball_yard_line` reports the LOS, not where the ball landed).
    fn ground_incomplete(&mut self) {
        self.ball.state = BallState::Grounded;
        self.ball.vel = Vec3::ZERO;
        self.ball.pos = Vec3::new(0.0, BALL_RADIUS, self.frame.line_of_scrimmage_z);
        self.rig.park_ball();
        self.events.emit(SimEvent::BallGrounded {
            position: self.ball.pos,
        });
        self.end_play(PlayEndReason::Incomplete);
    }

    /// End the play when the carrier leaves the field of play: a sideline
    /// exit is out of bounds, crossing the attacked goal line is a clean
    /// break (no scoring rules yet — the play simply ends).
    pub(crate) fn check_carrier_bounds(&mut self) {
        if let Some(carrier) = self.ball.carrier() {
            let pos = self.players[carrier.index()].pos;
            if pos.x.abs() >= FIELD_HALF_WIDTH - self.tuning.bounds_margin {
                self.end_play(PlayEndReason::OutOfBounds);
            } else if pos.z * self.frame.direction.sign() >= GOAL_LINE_Z {
                self.end_play(PlayEndReason::BrokeFree);
            }
        }
    }

}
