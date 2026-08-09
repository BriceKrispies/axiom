//! How a session begins.
//!
//! Three ways in, and the difference between them is only ever *luck*: the
//! shipping game, a named seed, and the one that faces the average keeper with
//! no roll in it at all. Kept apart from the machine itself because a
//! constructor that has to spell out every field is a wall of nouns, and the
//! state machine next door is where the reading should be spent.

use axiom_kernel::DeterministicRng;

use crate::figure::{KickDrive, KickPlan, Swing};
use crate::pitch::{ball_spot, GoalMouth};
use crate::shot::{ResolvedShot, ShotIntent};
use crate::tuning::Tuning;

use crate::play::keeper::Keeper;
use crate::play::nerve::KeeperNerve;
use crate::play::phase::Phase;
use crate::play::resolution::Tally;
use crate::play::shootout::{Shootout, Side};
use crate::play::Ball;

use super::Session;

/// The seed a session uses when none is named.
pub const DEFAULT_SEED: u64 = 0x0BE4_D17_5EED;

impl Session {
    /// A fresh session at the default seed.
    pub fn new(tuning: Tuning) -> Session {
        Session::seeded(tuning, DEFAULT_SEED)
    }

    /// A session facing the **average** keeper: no jitter, no guesses, always
    /// corrects.
    ///
    /// This is what the mechanic tests play against, so that "a bent shot beats a
    /// keeper that read it straight" is a claim about the mechanic rather than
    /// about a lucky roll. Nothing a player ever meets is steady.
    pub fn steady(tuning: Tuning) -> Session {
        Session {
            steady: true,
            keeper: Keeper::set(KeeperNerve::steady(&tuning.keeper)),
            ..Session::seeded(tuning, DEFAULT_SEED)
        }
    }

    /// A fresh session on an explicit seed. The same seed is the same shootout.
    pub fn seeded(tuning: Tuning, seed: u64) -> Session {
        let mouth = GoalMouth::new(tuning.goal.inset);
        let origin = ball_spot(tuning.flight.ball_radius);
        let intent = ShotIntent::default();
        let shot = ResolvedShot::build(origin, intent, &mouth, &tuning);
        Session {
            phase: Phase::Ready,
            phase_tick: 0,
            tick: 0,
            intent,
            shot,
            ball: Ball::placed(origin),
            keeper: Keeper::set(KeeperNerve::steady(&tuning.keeper)),
            kick: KickPlan::for_shot(origin, KickDrive::for_shot(&intent, &tuning), &tuning.kick),
            swing: Swing::cocked(&tuning.kick),
            kick_tick: 0,
            struck: None,
            result: None,
            tally: Tally::default(),
            net: None,
            seen: Vec::new(),
            rng: DeterministicRng::seeded(seed),
            steady: false,
            keep_clock: 0.0,
            shootout: Shootout::new(),
            side: Side::You,
            mouth,
            tuning,
        }
        .with_first_nerve()
    }

}
