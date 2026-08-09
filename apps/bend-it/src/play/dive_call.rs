//! What the player asks for when they are the one in the goal.
//!
//! # The same input, the other way round
//!
//! Taking a penalty, you draw the line you want the ball to take. Keeping one,
//! you draw the line you want your *body* to take — and the whole of the decision
//! is not where, it is **when you let go**.
//!
//! Release before the ball is struck and you have guessed. You get the full dive:
//! the legs are already moving, the momentum is already there, and the corners
//! are reachable. Release after you have seen it leave and you know something —
//! but a penalty is in the net in about a third of a second, and what is left is
//! enough to cover the middle and nothing more.
//!
//! That trade *is* the position. Every keeper in the world stands on that line
//! and makes exactly that bet, and it needs no new simulation to express: the
//! dive is the same integrated body with the same momentum, and letting go
//! earlier simply means it has been accelerating for longer.
//!
//! # Why the screen maps to the body and not to the world
//!
//! In first person the keeper is looking down the pitch at the kicker. Its own
//! goal is *behind* it — so there is no goal mouth on screen to aim at, and
//! casting a ray onto the plane the hands travel in would be casting onto a plane
//! a few centimetres from the camera.
//!
//! So the screen is read as the body, which is also how a keeper thinks: left of
//! centre is dive left, high on the screen is throw the hands up. No unprojection,
//! nothing to misread, and it stays true whichever way the camera is pointed.

use axiom::prelude::{Vec2, Vec3};

use crate::pitch::KEEPER_LINE_Z;
use crate::tuning::KeeperTuning;

/// The dive the player called for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DiveCall {
    /// Where the hands are being thrown, in world space.
    pub hands: Vec3,
    /// Which way the body goes, `-1` to `+1`.
    pub lean: f32,
    /// How high, `-1` low to `+1` high.
    pub height: f32,
}

impl DiveCall {
    /// Read a drawn line as a dive.
    ///
    /// `finish` is where the line ended, in the same pixels the viewport is
    /// measured in. Only the finish matters: a keeper's line is a gesture toward
    /// a corner, not a path the body follows, and asking a player to draw an
    /// accurate arc under this much time pressure would be asking for precision
    /// nobody has at the moment they need it.
    pub fn read(finish: Vec2, viewport: Vec2, standing: Vec3, tuning: &KeeperTuning) -> DiveCall {
        let half = Vec2::new(viewport.x * 0.5, viewport.y * 0.5);
        // Screen-relative, so the gesture means the same on any size of glass.
        let across = ((finish.x - half.x) / half.x.max(1.0)).clamp(-1.0, 1.0);
        // Up the screen is up in the goal, so the sign flips.
        let up = ((half.y - finish.y) / half.y.max(1.0)).clamp(-1.0, 1.0);
        // How far a body can throw its hands: everything the dive covers, which
        // is the same budget the rival keeper works to.
        let span = tuning.dive_distance + crate::figure::stretch_from_hips(tuning);
        DiveCall {
            hands: Vec3::new(
                standing.x + across * span,
                (standing.y + up * (tuning.vertical_reach + 0.75)).clamp(0.05, 2.6),
                KEEPER_LINE_Z + 0.34,
            ),
            lean: across,
            height: up,
        }
    }

    /// The dive a keeper makes when the player never called one: stand up and
    /// hope. It is a real option — a ball hit straight at a standing keeper is
    /// saved by the keeper being there — and it has to be what happens when the
    /// hand does nothing, because doing nothing must never be a crash.
    pub fn standing(standing: Vec3) -> DiveCall {
        DiveCall {
            hands: Vec3::new(standing.x, standing.y + 0.35, KEEPER_LINE_Z + 0.30),
            lean: 0.0,
            height: 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tuning::Tuning;

    fn stood() -> Vec3 {
        Vec3::new(0.0, 0.92, KEEPER_LINE_Z)
    }

    fn call(x: f32, y: f32) -> DiveCall {
        DiveCall::read(
            Vec2::new(x, y),
            Vec2::new(390.0, 844.0),
            stood(),
            &Tuning::DEFAULT.keeper,
        )
    }

    #[test]
    fn the_screen_is_the_body() {
        // Left of centre dives left, right dives right, and dead centre does
        // neither.
        assert!(call(40.0, 422.0).hands.x < -1.0);
        assert!(call(350.0, 422.0).hands.x > 1.0);
        assert!(call(195.0, 422.0).hands.x.abs() < 1.0e-4);
        // High on the screen throws the hands up, low throws them down.
        assert!(call(195.0, 60.0).hands.y > call(195.0, 780.0).hands.y + 1.0);
        // And the hands are always thrown in FRONT of the line, toward the ball.
        assert!(call(40.0, 422.0).hands.z > KEEPER_LINE_Z);
    }

    #[test]
    fn the_same_gesture_means_the_same_on_any_screen() {
        let big = DiveCall::read(
            Vec2::new(900.0, 300.0),
            Vec2::new(1000.0, 1000.0),
            stood(),
            &Tuning::DEFAULT.keeper,
        );
        let small = DiveCall::read(
            Vec2::new(360.0, 120.0),
            Vec2::new(400.0, 400.0),
            stood(),
            &Tuning::DEFAULT.keeper,
        );
        assert!((big.hands.x - small.hands.x).abs() < 1.0e-3);
        assert!((big.hands.y - small.hands.y).abs() < 1.0e-3);
    }

    #[test]
    fn a_wild_gesture_cannot_ask_for_more_than_a_body_has() {
        let tuning = Tuning::DEFAULT;
        let span = tuning.keeper.dive_distance + crate::figure::stretch_from_hips(&tuning.keeper);
        [(-9000.0f32, -9000.0f32), (9000.0, 9000.0)]
            .into_iter()
            .for_each(|(x, y)| {
                let wild = call(x, y);
                assert!(wild.hands.x.abs() <= span + 1.0e-3, "reached {}", wild.hands.x);
                assert!((0.05..=2.6).contains(&wild.hands.y), "reached {}", wild.hands.y);
                assert!((-1.0..=1.0).contains(&wild.lean));
                assert!((-1.0..=1.0).contains(&wild.height));
            });
    }

    #[test]
    fn calling_nothing_is_standing_up() {
        let idle = DiveCall::standing(stood());
        assert_eq!(idle.lean, 0.0);
        assert_eq!(idle.height, 0.0);
        assert!(idle.hands.y > stood().y, "hands up, as a set keeper holds them");
        assert!(idle.hands.z > KEEPER_LINE_Z);
    }
}
