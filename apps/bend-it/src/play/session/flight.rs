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

use super::{Session, SHADE_MEMORY};

impl Session {
    /// The run-up. The ball is launched on the contact tick, and the kicker's
    /// boot is on it then — the two read the same constant.
    pub(super) fn kicking(&mut self) {
        let contact = self.tuning.kick.contact;
        (self.phase_tick + 1 >= contact).then(|| {
            self.ball.launch(&self.shot.trajectory);
            self.phase = Phase::BallInFlight;
        });
    }

    /// One tick of flight: move the ball, move the keeper, then ask the geometry
    /// what happened. In that order, so the keeper is where it is *this* tick
    /// when the ball's swept segment is tested against it.
    pub(super) fn in_flight(&mut self) {
        let elapsed = self.ball.elapsed().unwrap_or(f32::INFINITY);
        let from = self.ball.advance(&self.shot.trajectory, DT, &self.tuning.flight);
        self.keeper
            .advance(&self.shot.trajectory, elapsed, &self.tuning.keeper);

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
                strength: (self.ball.velocity.length() * 0.028).clamp(0.10, 0.55),
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
        // The keeper watches where this one finished. It is the *authored*
        // finish, not where a deflection ended up, because that is what it was
        // beaten by.
        self.seen.push(self.shot.world_target);
        (self.seen.len() > SHADE_MEMORY).then(|| self.seen.remove(0));
        self.result = Some(result);
        self.tally.record(result);
        self.phase = Phase::Resolution;
        self.phase_tick = 0;
    }
}
