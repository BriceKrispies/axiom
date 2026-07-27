//! The in-flight guard: a committed throw that never actually left the hand.
//!
//! Small, but load-bearing enough to name. `commit` moves the loop to
//! `PassInFlight` optimistically, before the simulation has had its wind-up
//! ticks; the release can still ABORT if nobody is legally throwable, leaving
//! the ball with the quarterback. Believing a pass is airborne then does real
//! damage — the phase is steerable, so the player silently gets the stick, and
//! the outcome classifies off the quarterback still carrying, which reported
//! completions as SACKS for positive yards.

use crate::ai::RoleState;
use crate::state::SimState;

use super::controller::AttemptController;
use super::phase::AttemptPhase;

impl AttemptController {
    /// Resolve one tick of a pass in flight, reverting to  when the
    /// release aborted and the ball is still in the quarterback's hand.
    pub(super) fn pass_in_flight(&mut self, sim: &SimState, read: usize) -> AttemptPhase {
        let winding = matches!(
            sim.roles[sim.quarterback.index()],
            RoleState::QbWindup { .. }
        );
        let aborted =
            !winding && !sim.ball.is_airborne() && sim.possession == Some(sim.quarterback);
        self.choice = match aborted {
            true => None,
            false => self.choice,
        };
        match aborted {
            true => AttemptPhase::Developing,
            false => AttemptPhase::PassInFlight { read },
        }
    }

}
