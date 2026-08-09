//! The shootout, from the attempt machine's side of the fence.
//!
//! [`super`] owns one penalty. This owns the fact that a penalty belongs to a
//! *sequence* — whose kick it is, who steps up when the player is not taking it,
//! and when there is nothing left to take. It is a small file on purpose: the
//! rules themselves live in [`crate::play::shootout`], which knows nothing about
//! phases or clocks, and this is only the wiring between the two.

use crate::tuning::DT;

use crate::pitch::{GoalMouth, NetImpulse};
use crate::play::shootout::{Outcome, Shootout, Side};
use super::{Phase, Session, Tally};

impl Session {
    /// Whether the player is the one in the goal for this attempt.
    pub fn keeping(&self) -> bool {
        self.side == Side::Them
    }
    /// The score, the order and the rules of the shootout this attempt is in.
    pub fn shootout(&self) -> &Shootout {
        &self.shootout
    }
    /// Whose kick this is — and therefore which end of the game the player is at.
    pub fn side(&self) -> Side {
        self.side
    }
    /// How the shootout finished, if it has. While this is `Some`, nothing is
    /// taken: the game is over and waiting to be started again.
    pub fn outcome(&self) -> Option<Outcome> {
        self.shootout.outcome()
    }
    /// Begin a fresh shootout, keeping the seed's own thread of luck running so
    /// two shootouts in a row are not the same shootout.
    pub fn restart_shootout(&mut self) {
        self.shootout = Shootout::new();
        self.tally = Tally::default();
        self.seen.clear();
        self.reset();
    }
    pub fn mouth(&self) -> &GoalMouth {
        &self.mouth
    }
    pub fn net_impulse(&self) -> Option<NetImpulse> {
        self.net
    }

    /// One tick of the keeper, before the ball has left.
    ///
    /// Nothing to test the ball against yet — it is on the spot — so this is the
    /// body moving and only the body moving. It runs for **both** keepers: the
    /// player's, because their whole decision is when to let go, and the rival's,
    /// because a keeper that has already guessed goes before the ball moves and
    /// there would otherwise be nowhere for it to do that.
    pub(super) fn keep_step(&mut self) {
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
    }

    /// The rival steps up and takes its kick, without being asked.
    pub(super) fn take_for_rival(&mut self) {
        self.intent = crate::play::rival::take(&mut self.rng, &self.tuning);
        self.rebuild();
        self.phase = Phase::ShotReady;
        self.phase_tick = 0;
    }

}
