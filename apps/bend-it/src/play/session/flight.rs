//! The flight stage of an attempt: the run-up, the tick the ball leaves, and the
//! geometry that decides what happened to it.
//!
//! A child module of the session rather than a sibling, because it is the same
//! machine — it reads and writes the attempt's own state — and because keeping
//! it here is what lets the state stay private to the session instead of being
//! opened up to the whole crate to satisfy a file split.
//!
//! The order inside [`Session::in_flight`] is the load-bearing part: move the
//! ball, move the keeper, *then* ask the geometry. The keeper is therefore
//! wherever it actually is on this tick when the ball's swept segment is tested
//! against it, and a save is a capsule contact rather than a decision.

use axiom::prelude::Vec3;

use crate::contact::{deflect, sweep};
use crate::pitch::{frame_hit, inside_mouth, NetImpulse, NET_DEPTH};
use crate::play::ball::BallMotion;
use crate::play::phase::Phase;
use crate::play::resolution::ShotResult;
use crate::tuning::DT;

use super::Session;

impl Session {
    /// The run-up, and the swing at the end of it.
    ///
    /// The ball leaves on the tick the **integrated swing** reaches it. There is
    /// no contact constant any more: a harder drawing produces more torque, the
    /// leg arrives sooner, and the ball goes earlier — the animation and the
    /// launch cannot drift apart because they are the same number.
    pub(super) fn kicking(&mut self) {
        self.advance_swing();
        self.swing.struck_at().map(|_| {
            self.ball.launch(&self.shot.trajectory);
            // The keeper's clock does not restart here; it is simply told when
            // the ball left, so its reaction is measured from that moment while
            // the body it drives has been on one clock throughout.
            self.keeper.ball_struck(self.keep_clock);
            self.struck = Some(self.ball.velocity.length());
            self.phase = Phase::BallInFlight;
        });
    }

    /// One step of the leg. Before the plant lands it is simply carried through
    /// the run-up; after that the hip is driving it.
    pub(super) fn advance_swing(&mut self) {
        let released = self.kick_tick >= self.kick.release_tick(&self.tuning.kick);
        let contact_angle = self.kick.contact_angle();
        released.then(|| {
            self.swing
                .step(&self.kick.drive, contact_angle, &self.tuning.kick)
        });
        self.kick_tick += 1;
    }

    /// One tick of flight: move the ball, move the keeper, then ask the geometry
    /// what happened. In that order, so the keeper is where it is *this* tick
    /// when the ball's swept segment is tested against it.
    pub(super) fn in_flight(&mut self) {
        let from = self.ball.advance(&self.shot.trajectory, DT, &self.tuning.flight);
        // Keeping, the keeper does not think — it only executes what the player
        // called. The same body, the same momentum, the same capsules, the same
        // clock; the only difference is where the commitment came from.
        self.keep_clock += DT;
        let clock = self.keep_clock;
        match self.keeping() {
            true => self
                .keeper
                .advance_called(&self.shot.trajectory, clock, &self.tuning.keeper),
            false => self
                .keeper
                .advance(&self.shot.trajectory, clock, &self.tuning.keeper),
        }

        let radius = self.tuning.flight.ball_radius;
        let to = self.ball.position;

        // The keeper. Its reach and its body are the same capsules the figure is
        // drawn from, so a save is visibly a save.
        let keeper_frame = self.keeper.frame(&self.tuning.keeper);
        let saved = keeper_frame
            .obstacles()
            .into_iter()
            .find_map(|capsule| sweep(from, to, radius, capsule));
        if let Some(hit) = saved {
            let bounced = deflect(self.ball.velocity, hit.normal, 0.55, 0.35);
            self.ball.deflect_to(bounced);
            self.finish(ShotResult::Save);
            return;
        }

        // The frame.
        if let Some(hit) = frame_hit(from, to, radius) {
            let bounced = deflect(self.ball.velocity, hit.contact.normal, 0.72, 0.6);
            self.ball.deflect_to(bounced);
            self.finish(ShotResult::Frame(hit.member));
            return;
        }

        // Crossing the plane untouched.
        let crossed = (from.z > 0.0) & (to.z <= 0.0);
        if crossed {
            let travel = (from.z / (from.z - to.z).max(1.0e-6)).clamp(0.0, 1.0);
            let at = from.add(to.subtract(from).mul_scalar(travel));
            let scored = inside_mouth(at, radius);
            self.net = Some(NetImpulse {
                point: Vec3::new(at.x, at.y, -NET_DEPTH),
                strength: (self.ball.velocity.length() * 0.011).clamp(0.10, 0.55),
                age: 0.0,
            });
            self.finish([ShotResult::Miss, ShotResult::Goal][usize::from(scored)]);
            return;
        }

        // A ball that has run out of authored path and gone nowhere near the goal
        // still has to end the attempt.
        let stalled = (self.ball.motion == BallMotion::Free) & (self.ball.velocity.length() < 0.6);
        stalled.then(|| self.finish(ShotResult::Miss));
    }

    pub(super) fn finish(&mut self, result: ShotResult) {
        // The keeper watches where this one finished. Only the shots the player
        // TAKES, though: its memory is a memory of the person in front of it, and
        // shading toward its own team-mate's corners would be a keeper learning
        // the wrong opponent.
        (!self.keeping()).then(|| self.remember(self.shot.world_target));
        self.result = Some(result);
        self.tally.record(result);
        self.shootout.record(self.side, result);
        self.phase = Phase::Resolution;
        self.phase_tick = 0;
    }
}
