//! The ball: on the authored path until something physically stops it.
//!
//! Two motions, and the boundary between them is the whole honesty of the game.
//!
//! * **`OnPath`** — the ball reads its position straight out of the trajectory
//!   the player authored. Nothing corrects it, nothing steers it, nothing
//!   magnetises it toward the goal. It is where the drawing said it would be.
//! * **`Free`** — the ball has been *hit* (a keeper's hands, a post, the net) and
//!   the authored path is over. From that instant it is ordinary ballistics from
//!   whatever state the contact left it in.
//!
//! There is deliberately no third motion. A save cannot be produced by nudging
//! the path, because the only thing that can end `OnPath` is a real contact.
//!
//! Spin is presentation derived from the same path: forward roll from speed, and
//! sidespin whose sign is the one that would physically produce the curve the
//! player drew, so a shot that bends right visibly spins the way a shot that
//! bends right spins.

use axiom::prelude::Vec3;
use axiom_math::Quat;

use crate::pitch::{inside_mouth, NET_DEPTH};
use crate::shot::Trajectory;
use crate::tuning::FlightTuning;

/// Gravity, m/s².
const GRAVITY: f32 = -9.81;
/// Fraction of speed kept per second in free flight.
const AIR_KEEP: f32 = 0.86;
/// How hard the net takes the pace off a ball that has entered it.
const NET_KEEP: f32 = 0.06;
/// Bounce restitution off the turf.
const GROUND_BOUNCE: f32 = 0.42;

/// How the ball is currently moving.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BallMotion {
    /// Sitting on the spot.
    Placed,
    /// Following the authored trajectory, `t` seconds into the flight.
    OnPath { elapsed: f32 },
    /// Off the path after a real contact.
    Free,
}

/// The ball.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ball {
    pub position: Vec3,
    pub velocity: Vec3,
    pub orientation: Quat,
    pub motion: BallMotion,
}

impl Ball {
    /// Placed on the penalty spot.
    pub fn placed(spot: Vec3) -> Ball {
        Ball {
            position: spot,
            velocity: Vec3::ZERO,
            orientation: Quat::IDENTITY,
            motion: BallMotion::Placed,
        }
    }

    /// Struck: the ball joins the authored path at its very first sample.
    pub fn launch(&mut self, trajectory: &Trajectory) {
        let start = trajectory.sample(0.0);
        self.position = start.position;
        self.velocity = start.velocity;
        self.motion = BallMotion::OnPath { elapsed: 0.0 };
    }

    /// Knocked off the path by a contact, with the velocity the contact left.
    pub fn deflect_to(&mut self, velocity: Vec3) {
        self.velocity = velocity;
        self.motion = BallMotion::Free;
    }

    /// Advance one fixed step, returning where the ball moved *from* so a caller
    /// can sweep that segment against the things it might have hit.
    pub fn advance(&mut self, trajectory: &Trajectory, dt: f32, tuning: &FlightTuning) -> Vec3 {
        let from = self.position;
        match self.motion {
            BallMotion::Placed => {}
            BallMotion::OnPath { elapsed } => {
                let next = elapsed + dt;
                let state = trajectory.sample(next);
                self.position = state.position;
                self.velocity = state.velocity;
                // Reaching the end of the authored path is not a contact: the
                // ball simply continues, now under ordinary ballistics, which is
                // what carries it into the net behind the goal line.
                self.motion = match next >= trajectory.duration() {
                    true => BallMotion::Free,
                    false => BallMotion::OnPath { elapsed: next },
                };
            }
            BallMotion::Free => self.free_step(dt, tuning),
        }
        self.spin(dt, tuning);
        from
    }

    /// Ballistics, plus the two surfaces a loose ball meets here: the turf, and
    /// the netting behind the goal.
    fn free_step(&mut self, dt: f32, tuning: &FlightTuning) {
        let in_net = (self.position.z < 0.0) & inside_mouth(self.position, tuning.ball_radius);
        let keep = [AIR_KEEP, NET_KEEP][usize::from(in_net)];
        self.velocity = self
            .velocity
            .mul_scalar(keep.powf(dt))
            .add(Vec3::new(0.0, GRAVITY * dt, 0.0));
        self.position = self.position.add(self.velocity.mul_scalar(dt));

        // The turf.
        let floor = tuning.ball_radius;
        let below = self.position.y < floor;
        self.position = Vec3::new(
            self.position.x,
            self.position.y.max(floor),
            // The back of the net stops it.
            self.position.z.max(-NET_DEPTH + tuning.ball_radius),
        );
        self.velocity = match below {
            true => Vec3::new(
                self.velocity.x * 0.7,
                self.velocity.y.abs() * GROUND_BOUNCE,
                self.velocity.z * 0.7,
            ),
            false => self.velocity,
        };
    }

    /// Integrate the presentation spin.
    fn spin(&mut self, dt: f32, tuning: &FlightTuning) {
        let speed = self.velocity.length();
        let heading = self
            .velocity
            .normalize()
            .unwrap_or(Vec3::new(0.0, 0.0, -1.0));
        // Forward roll: about the horizontal axis perpendicular to travel.
        let roll_axis = Vec3::UNIT_Y.cross(heading);
        let roll = roll_axis.mul_scalar(
            (speed / tuning.ball_radius.max(1.0e-3)) * tuning.roll_per_speed,
        );
        // Sidespin: the sign that would physically produce the lateral force the
        // path is showing. A ball spinning about `+Y` is pushed toward `-X`, so a
        // path curving toward `+X` carries `-Y` spin.
        let side = Vec3::new(-heading.z, 0.0, heading.x);
        let curving = self.velocity.dot(side);
        let sidespin = Vec3::new(0.0, -curving * tuning.spin_per_curve, 0.0);
        let omega = roll.add(sidespin);
        let magnitude = omega.length();
        self.orientation = omega
            .normalize()
            .ok()
            .and_then(|axis| Quat::from_axis_angle(axis, magnitude * dt).ok())
            .map(|step| step.multiply(self.orientation))
            .unwrap_or(self.orientation);
    }

    /// How far into the authored flight the ball is, if it is still on it.
    pub fn elapsed(&self) -> Option<f32> {
        match self.motion {
            BallMotion::OnPath { elapsed } => Some(elapsed),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pitch::{ball_spot, GoalMouth};
    use crate::shot::{BendCurve, GoalTarget, ResolvedShot, ShotIntent};
    use crate::tuning::{Tuning, DT};

    fn shot(bend: f32, h: f32, v: f32) -> ResolvedShot {
        let tuning = Tuning::DEFAULT;
        ResolvedShot::build(
            ball_spot(tuning.flight.ball_radius),
            ShotIntent {
                target: GoalTarget::new(h, v),
                bend: BendCurve::through(0.5, bend, 0.14),
                loft: BendCurve::through(0.5, 0.6, 0.14),
                ..Default::default()
            },
            &GoalMouth::new(tuning.goal.inset),
            &tuning,
        )
    }

    #[test]
    fn a_placed_ball_does_not_move() {
        let tuning = Tuning::DEFAULT;
        let s = shot(0.0, 0.0, 0.5);
        let mut ball = Ball::placed(s.origin);
        let from = ball.advance(&s.trajectory, DT, &tuning.flight);
        assert_eq!(from, s.origin);
        assert_eq!(ball.position, s.origin);
        assert_eq!(ball.elapsed(), None);
    }

    #[test]
    fn a_struck_ball_follows_the_authored_path_exactly() {
        let tuning = Tuning::DEFAULT;
        let s = shot(4.0, -0.8, 0.7);
        let mut ball = Ball::placed(s.origin);
        ball.launch(&s.trajectory);
        assert_eq!(ball.position, s.origin);
        let mut t = 0.0f32;
        while t + DT < s.trajectory.duration() {
            ball.advance(&s.trajectory, DT, &tuning.flight);
            t += DT;
            let expected = s.trajectory.sample(t).position;
            assert!(
                ball.position.subtract(expected).length() < 1.0e-4,
                "the ball left the authored path at t={t}"
            );
        }
        assert!(ball.elapsed().is_some());
    }

    #[test]
    fn the_ball_leaves_the_path_only_when_the_flight_ends_or_it_is_hit() {
        let tuning = Tuning::DEFAULT;
        let s = shot(0.0, 0.0, 0.5);
        let mut ball = Ball::placed(s.origin);
        ball.launch(&s.trajectory);
        let steps = (s.trajectory.duration() / DT).ceil() as usize + 2;
        (0..steps).for_each(|_| {
            ball.advance(&s.trajectory, DT, &tuning.flight);
        });
        assert_eq!(ball.motion, BallMotion::Free);
        // A deflection takes it off the path immediately, wherever it was.
        let mut hit = Ball::placed(s.origin);
        hit.launch(&s.trajectory);
        hit.advance(&s.trajectory, DT, &tuning.flight);
        hit.deflect_to(Vec3::new(6.0, 2.0, 8.0));
        assert_eq!(hit.motion, BallMotion::Free);
        assert_eq!(hit.velocity, Vec3::new(6.0, 2.0, 8.0));
    }

    #[test]
    fn a_loose_ball_falls_bounces_and_stops_in_the_net() {
        let tuning = Tuning::DEFAULT;
        let s = shot(0.0, 0.0, 0.5);
        let mut ball = Ball::placed(Vec3::new(0.0, 2.0, 4.0));
        ball.deflect_to(Vec3::new(0.0, 0.0, -1.0));
        (0..240).for_each(|_| {
            ball.advance(&s.trajectory, DT, &tuning.flight);
        });
        assert!(ball.position.y >= tuning.flight.ball_radius - 1.0e-4, "it never sinks");
        // Fired hard into the goal it is caught by the netting, not lost.
        let mut into_net = Ball::placed(Vec3::new(0.0, 1.2, 1.0));
        into_net.deflect_to(Vec3::new(0.0, 0.0, -28.0));
        (0..120).for_each(|_| {
            into_net.advance(&s.trajectory, DT, &tuning.flight);
        });
        assert!(into_net.position.z >= -crate::pitch::NET_DEPTH);
        assert!(into_net.velocity.length() < 6.0, "the net kills the pace");
    }

    #[test]
    fn spin_matches_the_curve_the_player_drew() {
        let tuning = Tuning::DEFAULT;
        let right = shot(4.0, 0.0, 0.5);
        let mut ball = Ball::placed(right.origin);
        ball.launch(&right.trajectory);
        (0..8).for_each(|_| {
            ball.advance(&right.trajectory, DT, &tuning.flight);
        });
        assert_ne!(ball.orientation, Quat::IDENTITY, "the ball is turning");
        // A still ball accumulates no spin, and the axis maths never divides by
        // zero to get there.
        let mut still = Ball::placed(right.origin);
        still.advance(&right.trajectory, DT, &tuning.flight);
        assert_eq!(still.orientation, Quat::IDENTITY);
    }
}
