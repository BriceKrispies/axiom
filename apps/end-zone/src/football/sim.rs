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

use crate::state::{PlayPhase, SimState};

/// The ball's resting offset above the turf when dead at the spot.
pub(crate) fn ball_rest() -> Vec3 {
    Vec3::new(0.0, BALL_RADIUS, 0.0)
}


use super::state::{BallState, BALL_RADIUS};
use super::targeting;
use super::carry_socket;

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
