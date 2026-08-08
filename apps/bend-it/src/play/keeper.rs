//! The goalkeeper: a read, a commitment, and a physical attempt.
//!
//! The keeper is the reason the *shape* of a shot matters and not only its
//! endpoint, and it earns that with one honest limitation rather than with a
//! random number: **it only sees the beginning of the flight.**
//!
//! At the end of its reaction time it takes a single reading of where the ball
//! is and how it is moving, extrapolates that forward to the goal plane, and
//! dives for the answer. How much of the ball's *acceleration* it folds into
//! that extrapolation is [`KeeperTuning::read_fidelity`] — at the shipping value
//! it reads pace and direction well and curvature poorly, exactly like a human.
//!
//! So a straight shot is read correctly and the keeper is where the ball is
//! going. The same endpoint reached by a heavy bend is read as arriving
//! somewhere else entirely, and the keeper goes there instead. A lob is read as
//! a shot that is still climbing and the keeper commits underneath it; a dipping
//! trajectory is read as one that is still rising and the keeper commits over
//! it. None of that is scripted per shot — it all falls out of extrapolating a
//! curve from one instant.
//!
//! Nothing here ever touches the ball. The dive produces a body; the body
//! produces capsules; the capsules either intersect the ball's path or they do
//! not.

use axiom::prelude::Vec3;

use crate::figure::{keeper_frame, KeeperFrame, KeeperMotion};
use crate::pitch::{GOAL_HALF_WIDTH, GOAL_HEIGHT, KEEPER_LINE_Z};
use crate::shot::Trajectory;
use crate::tuning::KeeperTuning;

use super::keeper_read::{take_read, take_read_with, KeeperRead};
use super::nerve::KeeperNerve;

/// Standing hip height, metres. Matches the figure the pose module draws.
pub(super) const HIP_HEIGHT: f32 = 0.92;

/// The keeper across one attempt.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Keeper {
    home: Vec3,
    /// What kind of attempt this keeper is having. Drawn once, before the ball
    /// moved; nothing during the flight rolls anything.
    nerve: KeeperNerve,
    /// The height its own memory of recent penalties says to expect, and how far
    /// it trusts that memory over what it sees.
    expects: f32,
    memory_weight: f32,
    read: Option<KeeperRead>,
    /// Whether the one mid-flight correction has been spent.
    adjusted: bool,
    motion: KeeperMotion,
}

impl Keeper {
    /// A keeper set on its line, in the middle of the goal.
    pub fn set(nerve: KeeperNerve) -> Keeper {
        Keeper::shaded(0.0, HIP_HEIGHT, 0.0, nerve)
    }

    /// A keeper set `shade` metres off centre and expecting the ball at
    /// `expects` metres up — where its own reading of the last few penalties has
    /// told it to stand and to look.
    pub fn shaded(shade: f32, expects: f32, memory_weight: f32, nerve: KeeperNerve) -> Keeper {
        let hips = Vec3::new(shade, HIP_HEIGHT, KEEPER_LINE_Z);
        Keeper {
            home: hips,
            nerve,
            expects,
            memory_weight: memory_weight.clamp(0.0, 1.0),
            read: None,
            adjusted: false,
            motion: KeeperMotion {
                hips,
                lean: 0.0,
                extend: 0.0,
                height_bias: 0.0,
            },
        }
    }

    /// What it committed to, once it has.
    pub fn read(&self) -> Option<KeeperRead> {
        self.read
    }

    /// Its current motion state.
    pub fn motion(&self) -> KeeperMotion {
        self.motion
    }

    /// Its body — pose and capsules — for this tick.
    pub fn frame(&self, tuning: &KeeperTuning) -> KeeperFrame {
        keeper_frame(self.motion, tuning)
    }

    /// The nerve it is playing this penalty with.
    pub fn nerve(&self) -> KeeperNerve {
        self.nerve
    }

    /// What its memory says about arrival height, as `(height, weight)`.
    fn expectation(&self) -> (f32, f32) {
        (self.expects, self.memory_weight)
    }

    /// Advance the keeper to flight time `t`.
    ///
    /// Three things happen, once each: it is still until its reaction has
    /// elapsed; then it reads and commits; then — once, a beat later — it takes
    /// the one correction a real keeper gets. After that it is executing, and
    /// whatever the ball does next it does unopposed.
    pub fn advance(&mut self, trajectory: &Trajectory, t: f32, tuning: &KeeperTuning) {
        self.read = self
            .read
            .or_else(|| {
                (t >= self.nerve.reaction).then(|| {
                    take_read(
                        self.home,
                        self.expectation(),
                        &self.nerve,
                        trajectory,
                        t,
                        tuning,
                    )
                })
            });
        self.adjust(trajectory, t, tuning);
        self.motion = match self.read {
            None => self.set_stance(t),
            Some(read) => self.diving(read, t),
        };
    }

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
    fn adjust(&mut self, trajectory: &Trajectory, t: f32, tuning: &KeeperTuning) {
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
            // The correction re-times the dive from here, but it inherits how far
            // through the first commitment the body already is.
            extend_time: (remaining.max(1.0e-3)).min(previous.extend_time),
            at: t,
        });
        // The body carries its momentum: the dive restarts from where it is, not
        // from the line.
        self.home = Vec3::new(from, self.motion.hips.y, self.home.z);
    }

    /// The tiny weight-shifting bounce a keeper does on the line before a
    /// penalty. It is presentation, but it comes out of the same motion value the
    /// capsules are built from, so a keeper who has drifted is genuinely there.
    fn set_stance(&self, t: f32) -> KeeperMotion {
        let bounce = (t * 9.0).sin();
        KeeperMotion {
            hips: Vec3::new(
                self.home.x + bounce * 0.06,
                self.home.y - bounce.abs() * 0.04,
                self.home.z,
            ),
            lean: bounce * 0.08,
            extend: 0.0,
            height_bias: 0.0,
        }
    }

    /// Execute the committed dive: the hips travel from where they were when the
    /// commitment was made to where it aims, on an eased curve whose duration is
    /// the keeper's own speed over that distance.
    fn diving(&self, read: KeeperRead, t: f32) -> KeeperMotion {
        let elapsed = (t - read.at).max(0.0);
        let extend = (elapsed / read.extend_time.max(1.0e-3)).clamp(0.0, 1.0);
        let eased = extend * extend * (3.0 - 2.0 * extend);
        // A dive for a low ball drops the hips toward the turf; a dive for a high
        // one drives them up. Both are absolute targets, so a correction mid-dive
        // moves toward the new one from wherever the body already is.
        let target = Vec3::new(
            read.aim.x,
            HIP_HEIGHT + (read.aim.y - HIP_HEIGHT).max(0.0)
                - (HIP_HEIGHT - read.aim.y).max(0.0) * 0.55,
            self.home.z - 0.16,
        );
        KeeperMotion {
            hips: self.home.add(target.subtract(self.home).mul_scalar(eased)),
            lean: read.lean,
            extend: extend.max(self.motion.extend * (1.0 - eased)),
            height_bias: read.height_bias,
        }
    }
}

/// Clamp a predicted crossing to somewhere a keeper would plausibly believe —
/// used by the debug view so an off-target read is still drawable.
pub fn drawable_prediction(predicted: Vec3) -> Vec3 {
    Vec3::new(
        predicted.x.clamp(-GOAL_HALF_WIDTH * 1.6, GOAL_HALF_WIDTH * 1.6),
        predicted.y.clamp(0.0, GOAL_HEIGHT * 1.8),
        0.0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pitch::{ball_spot, GoalMouth};
    use crate::shot::{BendCurve, GoalTarget, ResolvedShot, ShotIntent};
    use crate::tuning::Tuning;

    /// The mechanic tests face the average keeper: what is being tested is the
    /// dive, not the dice.
    fn steady() -> KeeperNerve {
        KeeperNerve::steady(&Tuning::DEFAULT.keeper)
    }

    fn shot(bend: f32, loft: f32, h: f32, v: f32) -> ResolvedShot {
        shaped(bend, 0.5, loft, 0.5, h, v)
    }

    /// A shot with explicit peak positions, so a test can say "this one breaks
    /// late" rather than only "this one breaks".
    fn shaped(bend: f32, bend_at: f32, loft: f32, loft_at: f32, h: f32, v: f32) -> ResolvedShot {
        let tuning = Tuning::DEFAULT;
        ResolvedShot::build(
            ball_spot(tuning.flight.ball_radius),
            ShotIntent {
                target: GoalTarget::new(h, v),
                bend: BendCurve::through(bend_at, bend, 0.14),
                loft: BendCurve::through(loft_at, loft, 0.14),
                ..Default::default()
            },
            &GoalMouth::new(tuning.goal.inset),
            &tuning,
        )
    }

    #[test]
    fn the_keeper_is_still_until_it_reacts_then_commits_once() {
        let tuning = Tuning::DEFAULT;
        let s = shot(0.0, 0.6, 0.8, 0.4);
        let mut keeper = Keeper::set(steady());
        keeper.advance(&s.trajectory, 0.0, &tuning.keeper);
        assert!(keeper.read().is_none());
        assert!(keeper.motion().extend == 0.0);
        assert!(keeper.motion().hips.x.abs() < 0.1, "still on its line");

        keeper.advance(&s.trajectory, tuning.keeper.reaction, &tuning.keeper);
        let first = keeper.read().expect("it has committed");
        assert!(first.lean > 0.0, "the shot is to its left, so it dives left");
        // A later tick executes the same commitment; it never re-decides.
        keeper.advance(
            &s.trajectory,
            tuning.keeper.reaction + tuning.keeper.adjust_delay * 0.9,
            &tuning.keeper,
        );
        assert_eq!(keeper.read(), Some(first));
        assert!(keeper.motion().extend > 0.0);
        assert!(keeper.motion().hips.x > 0.2, "it is on its way");
    }

    #[test]
    fn the_one_correction_answers_the_line_but_never_the_height() {
        let tuning = Tuning::DEFAULT;
        // A shot that swings out early and comes back: the first read is wrong.
        let s = shaped(-2.0, 0.28, 2.6, 0.28, 0.55, 0.30);
        let mut keeper = Keeper::set(steady());
        keeper.advance(&s.trajectory, tuning.keeper.reaction, &tuning.keeper);
        let first = keeper.read().expect("committed");
        let after = tuning.keeper.reaction + tuning.keeper.adjust_delay;
        keeper.advance(&s.trajectory, after, &tuning.keeper);
        let corrected = keeper.read().expect("still committed");
        assert_ne!(corrected.aim.x, first.aim.x, "the line is corrected");
        assert_eq!(
            corrected.aim.y, first.aim.y,
            "a keeper in the air cannot un-commit its height"
        );
        assert_eq!(corrected.height_bias, first.height_bias);
        assert!(
            (corrected.predicted.x - s.world_target.x).abs()
                < (first.predicted.x - s.world_target.x).abs(),
            "and the correction is a better read than the first look"
        );
        // It only ever gets one.
        keeper.advance(&s.trajectory, after + 0.2, &tuning.keeper);
        assert_eq!(keeper.read().map(|r| r.at), Some(corrected.at));
    }

    #[test]
    fn a_dive_can_never_exceed_the_keepers_own_reach() {
        let tuning = Tuning::DEFAULT;
        // A shot into the very corner: the keeper wants more than it has.
        let s = shot(0.0, 0.2, 1.0, 0.0);
        let mut keeper = Keeper::set(steady());
        (0..40).for_each(|i| {
            keeper.advance(&s.trajectory, i as f32 / 60.0, &tuning.keeper);
        });
        let read = keeper.read().expect("committed");
        assert!(read.aim.x.abs() <= tuning.keeper.dive_distance + 1.0e-4);
        assert!(read.aim.y <= HIP_HEIGHT + tuning.keeper.vertical_reach + 1.0e-4);
        assert!(keeper.motion().hips.x.abs() <= tuning.keeper.dive_distance + 1.0e-4);
        // A long dive takes longer than a short one — there is a speed here, and
        // both are measured on the FIRST read so the comparison is dive against
        // dive rather than dive against mid-flight correction.
        let first = |target_h: f32| {
            let s = shot(0.0, 0.2, target_h, 0.3);
            let mut k = Keeper::set(steady());
            k.advance(&s.trajectory, tuning.keeper.reaction + 1.0e-3, &tuning.keeper);
            k.read().expect("committed").extend_time
        };
        assert!(
            first(0.05) <= first(1.0),
            "a short dive is not slower than a long one"
        );
    }

    #[test]
    fn a_high_read_throws_the_hands_up_and_a_low_read_throws_them_down() {
        let tuning = Tuning::DEFAULT;
        let high = shot(0.0, 0.4, 0.6, 1.0);
        let low = shot(0.0, 0.0, 0.6, 0.0);
        let commit = |s: &ResolvedShot| {
            let mut k = Keeper::set(steady());
            k.advance(&s.trajectory, tuning.keeper.reaction, &tuning.keeper);
            k.read().expect("committed")
        };
        assert!(commit(&high).height_bias > commit(&low).height_bias);
        assert!(commit(&low).height_bias < 0.0);
        // The body it produces answers the same way.
        let mut k = Keeper::set(steady());
        (0..30).for_each(|i| k.advance(&low.trajectory, i as f32 / 60.0, &tuning.keeper));
        let frame = k.frame(&tuning.keeper);
        assert!(frame.reach.a.y < HIP_HEIGHT + 0.6);
        assert_eq!(drawable_prediction(Vec3::new(99.0, 99.0, 5.0)).z, 0.0);
    }
}
