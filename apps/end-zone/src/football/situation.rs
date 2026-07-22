//! The football's AI-facing *situation*: a per-tick derived view over the
//! authoritative [`BallState`] that the whole defense (and offense) reads so
//! every player reacts to the same play. This is a **derivation, not a second
//! state machine** — every authoritative ball transition still lives in
//! [`crate::football::sim`]; `BallSituation` only classifies what that machine
//! has already produced, adding the two facts the raw ball state cannot carry:
//! a quarterback who has *committed to run* and a *contested* catch window.

use crate::football::state::{BallSim, BallState};
use crate::identity::PlayerId;
use crate::state::PlayPhase;

/// The football play situation every brain keys off this tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BallSituation {
    /// Pre-play: the ball is spotted and dead, waiting on the snap.
    #[default]
    PreSnap,
    /// The quarterback holds a live ball in (or near) the pocket, scanning.
    HeldByQb,
    /// The quarterback has left the pocket and committed to running — a live
    /// ball carrier the defense must rally to.
    QbScramble,
    /// The quarterback is winding up a throw; the release is imminent.
    ThrowWindup,
    /// A forward pass is in ballistic flight toward a predicted catch point.
    InFlight,
    /// A pass is in the air and a defender is inside the catch window — a
    /// contested ball, catchable or breakable this beat.
    Contested,
    /// A receiver holds a live ball after the catch — a runner in the open.
    Caught,
    /// The ball is live on the turf with no possessor.
    LooseBall,
    /// A pass hit the ground uncaught; the play is over.
    Incomplete,
    /// The play is dead (tackled, out of bounds, or scored).
    Dead,
}

impl BallSituation {
    /// Whether a live forward pass is in the air (flight or contested).
    pub fn ball_in_air(self) -> bool {
        matches!(self, BallSituation::InFlight | BallSituation::Contested)
    }

    /// Whether there is a live ground ball carrier to rally to (a scrambling
    /// quarterback or a receiver running after the catch).
    pub fn has_runner(self) -> bool {
        matches!(self, BallSituation::QbScramble | BallSituation::Caught)
    }

    /// Whether the ball is live and unpossessed on the ground.
    pub fn is_loose(self) -> bool {
        matches!(self, BallSituation::LooseBall)
    }

    /// Whether the play is still live (worth acting on).
    pub fn is_live(self) -> bool {
        !matches!(
            self,
            BallSituation::PreSnap | BallSituation::Incomplete | BallSituation::Dead
        )
    }

    /// A short debug label.
    pub fn label(self) -> &'static str {
        match self {
            BallSituation::PreSnap => "pre-snap",
            BallSituation::HeldByQb => "held-by-qb",
            BallSituation::QbScramble => "qb-scramble",
            BallSituation::ThrowWindup => "throw-windup",
            BallSituation::InFlight => "in-flight",
            BallSituation::Contested => "contested",
            BallSituation::Caught => "caught",
            BallSituation::LooseBall => "loose",
            BallSituation::Incomplete => "incomplete",
            BallSituation::Dead => "dead",
        }
    }
}

/// Classify the situation from the authoritative ball state plus the two
/// derived facts the raw state cannot carry (`qb_windup` from the quarterback's
/// role, `qb_run` from the deterministic scramble detector, `contested` from
/// the catch-window test). Pure function — same inputs, same answer.
pub fn classify(
    ball: &BallSim,
    quarterback: PlayerId,
    phase: PlayPhase,
    qb_windup: bool,
    qb_run: bool,
    contested: bool,
) -> BallSituation {
    match ball.state {
        BallState::Dead => match phase {
            PlayPhase::PreSnap => BallSituation::PreSnap,
            _ => BallSituation::Dead,
        },
        BallState::Snap { .. } => BallSituation::HeldByQb,
        BallState::Held { carrier } if carrier == quarterback => {
            if qb_windup {
                BallSituation::ThrowWindup
            } else if qb_run {
                BallSituation::QbScramble
            } else {
                BallSituation::HeldByQb
            }
        }
        BallState::Held { .. } => BallSituation::Caught,
        BallState::Airborne { .. } => {
            if contested {
                BallSituation::Contested
            } else {
                BallSituation::InFlight
            }
        }
        BallState::Loose => BallSituation::LooseBall,
        BallState::Grounded => BallSituation::Incomplete,
    }
}
