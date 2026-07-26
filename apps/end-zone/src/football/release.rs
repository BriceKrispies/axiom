//! The RELEASE: turning a commanded throw into a ball in the air.
//!
//! Split out of [`super::sim`] because it is now the most decision-dense step
//! in the football subsystem — it resolves WHO the pass is for, HOW HARD it
//! was thrown, and WHERE that actually puts it — while  stays a readable
//! list of the ball state machine.

use axiom::prelude::Vec3;

use crate::ai::RoleState;
use crate::events::SimEvent;
use crate::field::OffenseFrame;
use crate::identity::PlayerId;
use crate::player::AnimState;
use crate::state::SimState;

use super::possession::catch_point;
use super::state::BallState;
use super::targeting;
use super::{carry_socket, FlightInfo};

impl SimState {
    /// Release the scripted pass once the quarterback's wind-up completes:
    /// deterministic release point + velocity, real ballistic flight through
    /// the physics body — never a teleport.
    pub(crate) fn maybe_release(&mut self, carrier: PlayerId) {
        let RoleState::QbWindup { since } = self.roles[carrier.index()] else {
            return;
        };
        // Lock the target on the first tick of the wind-up: the pass commits to
        // whoever the quarterback was aiming at when the player pressed throw,
        // so a defender crossing the cone mid-wind-up cannot steal the read.
        //
        // A NAMED target (the decision window's read) wins over the cone: the
        // player chose that receiver by number, and silently redirecting the
        // ball to whoever drifted in front of the passer would make the read
        // meaningless. The cone still decides when nobody was named.
        if self.throw_target.is_none() {
            let declared = self.declared_target;
            let named = declared.filter(|id| {
                targeting::is_legal_target(carrier, *id, &self.players, &self.assignments)
            });
            let target = named.or_else(|| {
                let qb = &self.players[carrier.index()];
                let picks =
                    targeting::candidates(qb, &self.players, &self.assignments, &self.tuning);
                targeting::best(&picks)
            });
            let Some(target) = target else {
                // Nobody to throw to. Drop out of the wind-up so the quarterback
                // keeps scanning (and stays sackable) instead of freezing
                // mid-throw with no receiver.
                self.roles[carrier.index()] = RoleState::QbScan;
                self.declared_target = None;
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
        // Throw to where the receiver WILL be: a closed-form intercept solve
        // (see `flight::lead_point`), then keep the aim inbounds so leading a
        // receiver down the sideline never throws the ball away.
        let receiver = &self.players[throw_to.index()];
        // A negative power means "you decide": a programmatic throw (the
        // autopilot, the harness, the cone-aimed ambient pass) is always on the
        // money. Only a human wind-up names a power, and only a human can
        // therefore throw in front of or behind the receiver.
        let power = match self.throw_power < 0.0 {
            true => 0.5,
            false => self.throw_power,
        };
        let (aim, velocity) = super::flight::aim_and_velocity(
            release,
            receiver.pos,
            receiver.vel,
            power,
            self.tuning.gravity,
            &self.tuning,
        );
        let _ = OffenseFrame::clamp_in_bounds(aim, self.tuning.bounds_margin);
        let (landing, eta_ticks) = super::flight::predict_landing(
            release,
            velocity,
            self.tuning.gravity,
            catch_point(Vec3::ZERO).y,
        );
        let target = landing;
        let flight = FlightInfo {
            intended: throw_to,
            release,
            velocity,
            target,
            release_tick: self.tick,
            eta_ticks,
        };
        let axis = velocity.normalize().unwrap_or(Vec3::UNIT_Z);
        // Turn the passer onto the ball he is actually throwing. A named read
        // can sit outside the throwing cone, and a quarterback releasing a pass
        // over his own shoulder reads as a bug even when the flight is right.
        self.players[carrier.index()].facing = velocity.x.atan2(velocity.z);
        self.throw_target = None;
        self.declared_target = None;
        self.charge_target = None;
        self.charge_ticks = 0;
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

}
