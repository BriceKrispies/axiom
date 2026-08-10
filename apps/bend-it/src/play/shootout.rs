//! The shootout: your five, then their five, then sudden death.
//!
//! # Why the game needed a frame
//!
//! Everything below this file was already true — a real ball at a real speed,
//! struck by a body that obeys its own joints, against a keeper that reads,
//! commits and remembers. And it was not a game. It was a *practice net*: you
//! took a penalty, a number changed, and you took another one. `X / Y` forever
//! is a log file. Nothing was ever won, so nothing could ever be lost, and a
//! player has no reason to feel anything about penalty eight that they did not
//! feel about penalty seven.
//!
//! Tension and release is the whole emotional engine of a sport, and neither is
//! available without stakes. So: **five kicks each**. Every mechanic underneath
//! suddenly means something it did not mean before — the keeper's memory becomes
//! frightening, because it is learning your corners and you only have five; the
//! choice between placing a shot and hitting it hard becomes a decision rather
//! than a slider.
//!
//! Nothing new is simulated here. This is a *frame* around the attempt machine,
//! and it is deliberately the whole of the rules and none of the play.
//!
//! # The rules, exactly
//!
//! **Two halves, not ten alternating kicks.** The player takes all five of their
//! penalties first, and only then goes in goal for all five of the rival's. The
//! real-life order alternates, and this deliberately does not, because the two
//! ends of this game are two different *skills* — drawing a line and reading a
//! run-up — and swapping between them every ninety seconds means never settling
//! into either. Taking five in a row is a set: the keeper's memory has five kicks
//! to learn your corners and you can feel it closing in on you. Keeping five in a
//! row is a second set, played against a number you already know you have to
//! defend. It also puts the game's shape where a player can hold it: score five,
//! *then* find out what five has to survive.
//!
//! It still stops the moment it is **decided**, not when the ten kicks have been
//! taken. If the rival cannot catch you with two still to come, nobody takes
//! them. That early finish is not an optimisation, it is where the tension lives.
//! Note what the halves do to it: nothing can be decided while the player is
//! taking — the rival has all five in hand — so the player always gets their
//! whole set, and every early finish happens with the player in the goal, which
//! is exactly where it should be felt.
//!
//! After five each, sudden death: back to one apiece, the player first, and the
//! first pair that differ ends it.

use super::resolution::ShotResult;

/// How many kicks each side takes before sudden death.
pub const ROUNDS: usize = 5;

/// Whose kick it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// The player takes; they are the kicker.
    You,
    /// The rival takes; the player is the keeper.
    Them,
}

impl Side {
    pub fn other(self) -> Side {
        [Side::You, Side::Them][usize::from(self == Side::You)]
    }
}

/// How the shootout ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Won,
    Lost,
}

/// The score, the order, and the rules.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Shootout {
    taken: Vec<(Side, bool)>,
}

impl Shootout {
    pub fn new() -> Shootout {
        Shootout::default()
    }

    /// Record a finished penalty.
    pub fn record(&mut self, side: Side, result: ShotResult) {
        self.taken.push((side, result.scored()));
    }

    /// How many each side has scored.
    pub fn score(&self) -> (u32, u32) {
        (self.scored(Side::You), self.scored(Side::Them))
    }

    /// How many each side has taken.
    pub fn taken_by(&self, side: Side) -> usize {
        self.taken.iter().filter(|(s, _)| *s == side).count()
    }

    fn scored(&self, side: Side) -> u32 {
        self.taken
            .iter()
            .filter(|(s, ok)| (*s == side) & *ok)
            .count() as u32
    }

    /// Whose kick is next.
    ///
    /// In regulation the sides come in whole sets: the player takes until their
    /// five are gone, then the rival takes until theirs are. In sudden death
    /// there is no set left to take, so it falls back to one apiece with the
    /// player first — whoever has taken fewer, and the player on a tie.
    pub fn turn(&self) -> Side {
        let (you, them) = (self.taken_by(Side::You), self.taken_by(Side::Them));
        let theirs = ((you >= ROUNDS) & (them < ROUNDS)) | (self.sudden_death() & (you > them));
        [Side::You, Side::Them][usize::from(theirs)]
    }

    /// Which kick of this side's set it is, from 1 — what the scoreboard counts.
    pub fn round(&self) -> usize {
        self.taken_by(self.turn()) + 1
    }

    /// Whether this is sudden death: both sides have had their five.
    pub fn sudden_death(&self) -> bool {
        self.taken_by(Side::You).min(self.taken_by(Side::Them)) >= ROUNDS
    }

    /// How it finished, if it has.
    ///
    /// Two ways to end. In the regulation five, the moment one side cannot be
    /// caught even if every remaining kick went the other way. In sudden death,
    /// the moment a completed pair differs.
    pub fn outcome(&self) -> Option<Outcome> {
        let (you, them) = self.score();
        let (took_you, took_them) = (self.taken_by(Side::You), self.taken_by(Side::Them));
        let left = |took: usize| ROUNDS.saturating_sub(took) as u32;
        // Sudden death: only ever decided on a completed pair.
        let paired = took_you == took_them;
        let decided_late = self.sudden_death() & paired & (you != them);
        // Regulation: unreachable even with everything still to come. Only
        // while there IS something still to come — in sudden death the same sum
        // says every one-goal lead is unassailable, which is how you end a
        // shootout halfway through a pair.
        let decided_early = !self.sudden_death()
            & ((you > them + left(took_them)) | (them > you + left(took_you)));
        (decided_late | decided_early).then(|| {
            [Outcome::Lost, Outcome::Won][usize::from(you > them)]
        })
    }

    /// Every kick taken, in order — what the scoreboard is drawn from.
    pub fn taken(&self) -> &[(Side, bool)] {
        &self.taken
    }

    /// The row of marks for one side: `Some(true)` scored, `Some(false)` missed,
    /// `None` not taken yet. Always at least [`ROUNDS`] long, and longer once
    /// sudden death has begun.
    pub fn marks(&self, side: Side) -> Vec<Option<bool>> {
        let mine: Vec<Option<bool>> = self
            .taken
            .iter()
            .filter(|(s, _)| *s == side)
            .map(|(_, ok)| Some(*ok))
            .collect();
        let width = ROUNDS.max(self.taken_by(Side::You).max(self.taken_by(Side::Them)));
        let pad = width.saturating_sub(mine.len());
        mine.into_iter().chain((0..pad).map(|_| None)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn play(marks: &[bool]) -> Shootout {
        let mut s = Shootout::new();
        marks.iter().for_each(|scored| {
            let side = s.turn();
            s.record(
                side,
                [ShotResult::Save, ShotResult::Goal][usize::from(*scored)],
            );
        });
        s
    }

    #[test]
    fn the_player_takes_their_whole_set_before_the_rival_takes_any() {
        let mut s = Shootout::new();
        assert_eq!(s.turn(), Side::You);
        assert_eq!(s.round(), 1);
        // Four kicks in, it is still the player's ball.
        (0..4).for_each(|_| s.record(Side::You, ShotResult::Goal));
        assert_eq!(s.turn(), Side::You, "the set is not finished");
        assert_eq!(s.round(), 5, "the fifth of five");
        // The fifth ends the set, and only then do the ends change.
        s.record(Side::You, ShotResult::Goal);
        assert_eq!(s.turn(), Side::Them);
        assert_eq!(s.round(), 1, "their set counts from one again");
        (0..4).for_each(|_| s.record(Side::Them, ShotResult::Goal));
        assert_eq!(s.turn(), Side::Them, "still theirs, four in");
        assert_eq!(Side::You.other(), Side::Them);
        assert_eq!(Side::Them.other(), Side::You);
    }

    #[test]
    fn a_shootout_in_progress_has_no_outcome() {
        assert_eq!(Shootout::new().outcome(), None);
        // 5-3 with two of theirs to come is wide open: they can still tie it.
        let s = play(&[true, true, true, true, true, true, true, true]);
        assert_eq!(s.score(), (5, 3));
        assert_eq!(s.outcome(), None);
    }

    #[test]
    fn nothing_can_be_decided_while_the_player_is_still_taking() {
        // The worst possible set — five misses — and it is still not over,
        // because the rival has not taken a kick yet. The player always gets
        // their whole five.
        let s = play(&[false; 5]);
        assert_eq!(s.score(), (0, 0));
        assert_eq!(s.outcome(), None);
        assert_eq!(s.turn(), Side::Them, "and now they answer it");
    }

    #[test]
    fn it_ends_the_moment_it_is_decided_and_not_a_kick_later() {
        // Five out of five, and they miss the first: 5-0 with four to come is
        // uncatchable. Nobody takes the rest.
        let s = play(&[true, true, true, true, true, false]);
        assert_eq!(s.score(), (5, 0));
        assert_eq!(s.outcome(), Some(Outcome::Won));
        assert!(s.taken_by(Side::Them) < ROUNDS, "it stopped early");
        // The mirror: nought from five, and their first kick settles it.
        let s = play(&[false, false, false, false, false, true]);
        assert_eq!(s.score(), (0, 1));
        assert_eq!(s.outcome(), Some(Outcome::Lost));
        assert_eq!(s.taken_by(Side::Them), 1);
    }

    #[test]
    fn five_each_all_scored_goes_to_sudden_death() {
        let s = play(&[true; 10]);
        assert_eq!(s.score(), (5, 5));
        assert_eq!(s.outcome(), None, "5-5 is not a result");
        assert!(s.sudden_death());
        assert_eq!(s.turn(), Side::You);
        // You score, they have not answered yet: still not decided.
        let mut s = s;
        s.record(Side::You, ShotResult::Goal);
        assert_eq!(s.outcome(), None, "a lead mid-pair decides nothing");
        // They miss: that pair differs, and it is over.
        s.record(Side::Them, ShotResult::Save);
        assert_eq!(s.outcome(), Some(Outcome::Won));
    }

    #[test]
    fn sudden_death_can_run_as_long_as_it_likes() {
        let mut s = play(&[true; 10]);
        (0..6).for_each(|_| {
            let side = s.turn();
            s.record(side, ShotResult::Goal);
        });
        assert_eq!(s.score(), (8, 8));
        assert_eq!(s.outcome(), None);
        assert_eq!(s.marks(Side::You).len(), 8, "the row grows with it");
    }

    #[test]
    fn the_marks_are_a_scoreboard_a_person_can_read() {
        let s = play(&[true, false, false]);
        assert_eq!(
            s.marks(Side::You),
            vec![Some(true), Some(false), Some(false), None, None]
        );
        assert_eq!(s.marks(Side::Them), vec![None; 5], "they have not been up yet");
        assert_eq!(s.taken().len(), 3);
    }

    #[test]
    fn a_shootout_can_be_lost_without_the_last_kicks_being_taken() {
        // Two from five, and they answer with three: 2-3 with two still in hand
        // is uncatchable, and their last two are never taken.
        let s = play(&[true, true, false, false, false, true, true, true]);
        assert_eq!(s.score(), (2, 3));
        assert_eq!(s.outcome(), Some(Outcome::Lost));
        assert_eq!(s.taken_by(Side::Them), 3);
    }
}
