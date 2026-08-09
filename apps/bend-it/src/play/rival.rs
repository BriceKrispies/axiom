//! The other team's penalty taker.
//!
//! Five of the ten kicks in a shootout are not the player's, and something has to
//! take them. This is that something, and it is deliberately not clever: it picks
//! a corner, shapes the ball a bit, hits it somewhere between firm and hard, and
//! draws it all from the shootout's own seeded generator so a replay is a replay.
//!
//! # What it is for
//!
//! Not difficulty. The rival exists so the player has to **keep**, and keeping is
//! where the player learns what the keeper has been doing to them for the last
//! hundred penalties: that a corner is unreachable if you wait, that waiting is
//! the only way to know, and that everybody in that goal is guessing a little.
//!
//! So it is tuned for *legibility* rather than for cruelty. It favours the
//! corners, because a corner is what a keeper has to commit early against; it
//! mixes in the occasional one down the middle, because a keeper who never has to
//! consider standing still is not really choosing; and it never draws a shape so
//! wild that a save would feel arbitrary.

use axiom_kernel::DeterministicRng;

use crate::shot::{BendCurve, GoalTarget, ShotIntent};
use crate::stroke::Pace;
use crate::tuning::Tuning;

/// A random number in `0..1`.
fn unit(rng: &mut DeterministicRng) -> f32 {
    rng.next_bounded(1_000_001) as f32 / 1_000_000.0
}

/// A random number in `-1..1`.
fn signed(rng: &mut DeterministicRng) -> f32 {
    unit(rng) * 2.0 - 1.0
}

/// Roll the rival's next penalty.
pub fn take(rng: &mut DeterministicRng, tuning: &Tuning) -> ShotIntent {
    // Where. Four times in five it goes to a side; the fifth is straight down the
    // middle, which is the shot that punishes a keeper for always committing.
    let sideways = unit(rng) < 0.80;
    let side = signed(rng).signum();
    let h = [0.0, side * (0.45 + unit(rng) * 0.55)][usize::from(sideways)];
    // How high. Low corners are the bread and butter; the roof is rarer, because
    // a keeper beaten high looks beaten by the shot rather than by itself.
    let high = unit(rng) < 0.35;
    let v = [0.06 + unit(rng) * 0.30, 0.62 + unit(rng) * 0.33][usize::from(high)];

    // Shape. Enough to matter to the flight, never enough to look like a trick.
    let bend = signed(rng) * 0.55 * tuning.bend.max_offset;
    let loft = (0.15 + unit(rng) * 0.55) * tuning.loft.max_offset;
    let breaks = 0.35 + unit(rng) * 0.35;

    ShotIntent::curved(
        GoalTarget::new(h, v),
        BendCurve::through(breaks, bend, tuning.bend.peak_margin),
        BendCurve::through(0.5, loft, tuning.loft.peak_margin),
        Pace {
            speed: 0.45 + unit(rng) * 0.5,
            easing: 0.0,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pitch::GoalMouth;
    use crate::shot::ResolvedShot;

    fn roll(n: u64) -> Vec<ShotIntent> {
        let tuning = Tuning::DEFAULT;
        let mut rng = DeterministicRng::seeded(0xBEEF);
        (0..n).map(|_| take(&mut rng, &tuning)).collect()
    }

    #[test]
    fn the_same_seed_is_the_same_rival() {
        let mut a = DeterministicRng::seeded(7);
        let mut b = DeterministicRng::seeded(7);
        let tuning = Tuning::DEFAULT;
        (0..12).for_each(|_| assert_eq!(take(&mut a, &tuning), take(&mut b, &tuning)));
    }

    #[test]
    fn it_favours_the_corners_but_does_not_only_go_there() {
        let shots = roll(400);
        let middle = shots.iter().filter(|s| s.target.h.abs() < 0.2).count();
        assert!(middle > 40, "it never goes down the middle: {middle}/400");
        assert!(middle < 140, "it goes down the middle too often: {middle}/400");
        // Both sides, roughly evenly — a rival that only ever went one way would
        // be a pattern rather than an opponent.
        let left = shots.iter().filter(|s| s.target.h < -0.2).count();
        let right = shots.iter().filter(|s| s.target.h > 0.2).count();
        assert!(left > 80 && right > 80, "lopsided: {left} left, {right} right");
    }

    #[test]
    fn every_shot_it_takes_is_one_a_kicker_could_strike() {
        let tuning = Tuning::DEFAULT;
        let mouth = GoalMouth::new(tuning.goal.inset);
        let origin = crate::pitch::ball_spot(tuning.flight.ball_radius);
        roll(200).into_iter().for_each(|intent| {
            let shot = ResolvedShot::build(origin, intent, &mouth, &tuning);
            // It finishes inside the goal, above the turf, at a real speed.
            assert!(crate::pitch::inside_mouth(shot.world_target, 0.0));
            assert!(shot
                .trajectory
                .points()
                .iter()
                .all(|p| p.y >= tuning.flight.ball_radius - 1.0e-4));
            let launch = intent.launch_speed(&tuning);
            assert!((tuning.flight.slow_launch..=tuning.flight.fast_launch).contains(&launch));
        });
    }

    #[test]
    fn it_mixes_high_and_low() {
        let shots = roll(300);
        let low = shots.iter().filter(|s| s.target.v < 0.4).count();
        let high = shots.iter().filter(|s| s.target.v > 0.6).count();
        assert!(low > 120, "never low: {low}");
        assert!(high > 60, "never high: {high}");
    }
}
