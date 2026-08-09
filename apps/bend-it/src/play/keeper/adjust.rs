//! The one correction a keeper gets.
//!
//! Split from the commitment next door because it is the opposite decision. That
//! file is about *going*; this is about the single chance to have been wrong
//! about it — and the asymmetry between them is the whole reason a shot's shape
//! matters. Movement the keeper sees before its correction it can answer;
//! movement after it, it cannot.

use axiom::prelude::Vec3;

use crate::shot::Trajectory;
use crate::tuning::KeeperTuning;

use crate::play::keeper_read::{take_read_with, KeeperRead};

use super::Keeper;

impl Keeper {
    /// The single mid-flight correction.
    ///
    /// This is the pressure that makes *where* a curve breaks the whole
    /// decision. A shot that does its moving early is still moving in front of a
    /// keeper who has not committed yet, and the correction answers it. A shot
    /// that holds its line and breaks late is corrected onto a path it is about
    /// to leave — and the keeper has no second correction left.
    ///
    /// It is bounded by physics, not by generosity: the keeper can only redirect
    /// as far as its own speed carries it in the time the ball has left, and
    /// never past the reach it had to begin with.
    pub(super) fn adjust(&mut self, trajectory: &Trajectory, t: f32, tuning: &KeeperTuning) {
        let due = self
            .read
            .filter(|_| !self.adjusted)
            // A keeper who guessed has nothing to correct, and some attempts it
            // simply does not get a second look in.
            .filter(|_| self.nerve.corrects)
            .filter(|read| t >= read.at + tuning.adjust_delay);
        let Some(previous) = due else {
            return;
        };
        self.adjusted = true;
        let fresh = take_read_with(
            self.home,
            self.expectation(),
            &self.nerve,
            trajectory,
            t,
            tuning,
            tuning.adjust_fidelity,
        );
        // How much further it can still get, from where its hips are now.
        let remaining = (trajectory.duration() - t).max(0.0);
        let budget = tuning.dive_speed * remaining;
        let from = self.motion.hips.x;
        let aim_x = fresh
            .aim
            .x
            .clamp(from - budget, from + budget)
            .clamp(-tuning.dive_distance, tuning.dive_distance);
        // The correction is LATERAL ONLY, and that asymmetry is deliberate: a
        // keeper already in the air can still adjust its line, but it cannot
        // un-commit its height. Whatever the first read decided about how high to
        // throw its hands, it now has to live with — which is what makes the
        // vertical editor a real commitment rather than a second bend.
        self.read = Some(KeeperRead {
            predicted: Vec3::new(fresh.predicted.x, previous.predicted.y, 0.0),
            aim: Vec3::new(aim_x, previous.aim.y, previous.aim.z),
            lean: ((aim_x - self.home.x) / tuning.dive_distance.max(1.0e-3)).clamp(-1.0, 1.0),
            height_bias: previous.height_bias,
            extend_time: previous.extend_time,
            at: t,
        });
    }

}
