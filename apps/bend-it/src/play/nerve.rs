//! The keeper's nerve: everything about one penalty that is not the same twice.
//!
//! All of a keeper's variation is drawn **once, up front**, from the session's
//! seeded generator — before the ball is struck, which is also when a real keeper
//! settles what kind of attempt this is going to be. Nothing during the flight
//! rolls anything.
//!
//! That is deliberate and it buys three things. The tick loop stays a pure
//! function of `(nerve, trajectory, t)`, so it is testable at a fixed nerve. A
//! recorded penalty replays exactly, because the whole of its luck is five
//! numbers. And the variation is *inspectable* — the debug view can print what
//! kind of keeper you are actually facing, instead of leaving you to wonder
//! whether it read you or guessed.
//!
//! The randomness is the kernel's own seeded generator, which reads no entropy,
//! no clock and no global state. Same seed, same shootout, on any machine.

use axiom_kernel::DeterministicRng;

use crate::tuning::KeeperTuning;

/// What kind of attempt this keeper is having.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KeeperNerve {
    /// How long it actually takes to move, seconds.
    pub reaction: f32,
    /// How far out its judgement is, metres across and metres up.
    pub read_error_across: f32,
    pub read_error_up: f32,
    /// How completely it follows through on what it committed to, `0..1`.
    pub execution: f32,
    /// `Some(side)` when it abandoned the read and simply picked a side before
    /// the ball was struck. `-1` its right as the shooter sees it, `+1` the
    /// other. A guess is total: it commits and takes no correction.
    pub guess: Option<f32>,
    /// Whether it gets its one mid-flight correction this time.
    pub corrects: bool,
}

impl KeeperNerve {
    /// The average keeper: no jitter, no guess, always corrects.
    ///
    /// This is the keeper the *mechanic* tests face, so that "a bent shot beats
    /// a keeper that reads straight" is a statement about the mechanic and not
    /// about a lucky roll. The keeper a player faces is [`Self::roll`].
    pub fn steady(tuning: &KeeperTuning) -> KeeperNerve {
        KeeperNerve {
            reaction: tuning.reaction,
            read_error_across: 0.0,
            read_error_up: 0.0,
            execution: tuning.execution,
            guess: None,
            corrects: true,
        }
    }

    /// Roll one penalty's worth of nerve.
    pub fn roll(rng: &mut DeterministicRng, tuning: &KeeperTuning) -> KeeperNerve {
        let symmetric = |rng: &mut DeterministicRng, spread: f32| {
            (rng.next_bounded(2001) as f32 / 1000.0 - 1.0) * spread
        };
        let reaction = (tuning.reaction + symmetric(rng, tuning.reaction_jitter)).max(0.02);
        let read_error_across = symmetric(rng, tuning.read_error_across);
        let read_error_up = symmetric(rng, tuning.read_error_up);
        let execution =
            (tuning.execution + symmetric(rng, tuning.execution_spread)).clamp(0.35, 1.0);
        let guesses = rng.next_bool_in_thousand((tuning.guess_chance * 1000.0) as u32);
        let side = [(-1.0f32), 1.0][rng.next_bounded(2) as usize];
        let corrects = rng.next_bool_in_thousand((tuning.correction_chance * 1000.0) as u32);
        KeeperNerve {
            reaction,
            read_error_across,
            read_error_up,
            execution,
            // A keeper that has already guessed has nothing left to correct.
            guess: guesses.then_some(side),
            corrects: corrects & !guesses,
        }
    }

    /// A one-line description, for the debug view.
    pub fn describe(&self) -> String {
        match self.guess {
            Some(side) => format!(
                "guessed {} (react {:.2}s, exec {:.2})",
                ["left", "right"][usize::from(side > 0.0)],
                self.reaction,
                self.execution
            ),
            None => format!(
                "read (react {:.2}s, err {:+.2}/{:+.2} m, exec {:.2}, {})",
                self.reaction,
                self.read_error_across,
                self.read_error_up,
                self.execution,
                ["no correction", "corrects"][usize::from(self.corrects)]
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tuning::Tuning;

    #[test]
    fn a_steady_keeper_has_no_luck_in_it_at_all() {
        let tuning = Tuning::DEFAULT;
        let steady = KeeperNerve::steady(&tuning.keeper);
        assert_eq!(steady.reaction, tuning.keeper.reaction);
        assert_eq!(steady.execution, tuning.keeper.execution);
        assert_eq!(steady.read_error_across, 0.0);
        assert_eq!(steady.read_error_up, 0.0);
        assert_eq!(steady.guess, None);
        assert!(steady.corrects);
        assert!(steady.describe().contains("read"));
    }

    #[test]
    fn every_roll_stays_inside_the_bounds_it_was_given() {
        let t = Tuning::DEFAULT;
        let mut rng = DeterministicRng::seeded(0xB0A7);
        (0..2000).for_each(|_| {
            let n = KeeperNerve::roll(&mut rng, &t.keeper);
            assert!((n.reaction - t.keeper.reaction).abs() <= t.keeper.reaction_jitter + 1.0e-4);
            assert!(n.reaction > 0.0);
            assert!(n.read_error_across.abs() <= t.keeper.read_error_across + 1.0e-4);
            assert!(n.read_error_up.abs() <= t.keeper.read_error_up + 1.0e-4);
            assert!(
                (n.execution - t.keeper.execution).abs() <= t.keeper.execution_spread + 1.0e-4
            );
            assert!((0.35..=1.0).contains(&n.execution));
            n.guess
                .into_iter()
                .for_each(|side| assert!(side == -1.0 || side == 1.0));
            // A keeper that guessed has nothing left to correct.
            assert!(!(n.guess.is_some() & n.corrects));
        });
    }

    #[test]
    fn the_same_seed_is_the_same_keeper_and_a_different_seed_is_not() {
        let t = Tuning::DEFAULT;
        let run = |seed: u64| {
            let mut rng = DeterministicRng::seeded(seed);
            (0..24)
                .map(|_| KeeperNerve::roll(&mut rng, &t.keeper))
                .collect::<Vec<_>>()
        };
        assert_eq!(run(7), run(7), "a seed is a shootout");
        assert_ne!(run(7), run(8), "and two seeds are two shootouts");
    }

    #[test]
    fn it_guesses_about_as_often_as_it_was_told_to() {
        let t = Tuning::DEFAULT;
        let mut rng = DeterministicRng::seeded(99);
        let rolls = 4000;
        let guesses = (0..rolls)
            .filter(|_| KeeperNerve::roll(&mut rng, &t.keeper).guess.is_some())
            .count();
        let rate = guesses as f32 / rolls as f32;
        assert!(
            (rate - t.keeper.guess_chance).abs() < 0.03,
            "guessed {rate:.3} of the time, asked for {}",
            t.keeper.guess_chance
        );
        // Both sides get picked.
        let mut rng = DeterministicRng::seeded(5);
        let sides: Vec<f32> = (0..600)
            .filter_map(|_| KeeperNerve::roll(&mut rng, &t.keeper).guess)
            .collect();
        assert!(sides.iter().any(|s| *s < 0.0) && sides.iter().any(|s| *s > 0.0));
        assert!(KeeperNerve::roll(&mut rng, &t.keeper).describe().len() > 8);
    }
}
