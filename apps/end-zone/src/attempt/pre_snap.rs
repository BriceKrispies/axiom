//! The held beat before the snap: the play picker's whole life.
//!
//! Two decisions live here, and they are the reason the pre-snap is a *phase*
//! rather than a delay. **When to snap** — a called play snaps the moment the
//! offense finishes shifting into it, so pressing a play is the snap count and
//! the player is never waiting on a timer they cannot influence; an uncalled
//! one snaps on the deadline. **What a call does** — it re-installs the play,
//! which is what recompiles the routes and the alignment.

use crate::launch::RunConfig;
use crate::state::SimState;

use super::controller::AttemptController;
use super::phase::AttemptPhase;
use super::setup;
use super::SHIFT_TICKS;

impl AttemptController {
    /// Whether the ball goes this tick.
    pub(super) fn ready_to_snap(&self, sim: &SimState, tick: u64, snap_at: u64) -> bool {
        tick >= snap_at || (self.called && setup::offense_is_set(sim))
    }

    /// Hold at the line for one tick, applying a play call if one landed.
    ///
    /// Deliberately issues no `BeginPlay`: that re-lines both teams up
    /// instantly, and the point of the hold is to watch the offense SHIFT into
    /// the new formation on its own feet.
    pub(super) fn hold(
        &mut self,
        sim: &mut SimState,
        config: &RunConfig,
        snap_at: u64,
    ) -> AttemptPhase {
        let called = self.pending_concept.take();
        called.into_iter().for_each(|next| {
            self.concept = next;
            self.called = true;
            self.last_defense_index = setup::install(sim, config, self.attempt_index, self.concept);
        });
        // A call replaces the hold's deadline with the (much shorter) shift
        // budget, so pressing a play is always answered promptly even when the
        // offense is set too slowly to trigger the snap on its own.
        let deadline = called
            .map(|_| snap_at.min(sim.tick + SHIFT_TICKS))
            .unwrap_or(snap_at);
        AttemptPhase::PreSnap { snap_at: deadline }
    }
}
