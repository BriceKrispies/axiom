//! The two beats before the snap: **calling a play**, and **getting set**.
//!
//! They are separate phases because they ask separate questions and they end on
//! separate kinds of fact. The call ends on a *player decision* and has no clock
//! at all — an attempt never runs a play nobody chose, so there is nothing to
//! time out to. The shift ends on a *fact about the field* — every offensive
//! player standing on his spot — so the ball goes because the offense is ready,
//! not because a timer expired.
//!
//! That ordering is the whole point: call, watch it install, snap. Nothing here
//! issues `BeginPlay`, because that re-lines both teams up instantly and the
//! shift is precisely what the player is meant to see happen.

use crate::launch::RunConfig;
use crate::state::SimState;

use super::controller::AttemptController;
use super::phase::AttemptPhase;
use super::setup;
use super::SHIFT_STALL_TICKS;

impl AttemptController {
    /// Hold at the line until a play is called.
    ///
    /// Applying the call is what re-installs the play — which is what
    /// recompiles the routes and the alignment, and therefore what gives the
    /// offense somewhere new to run to.
    pub(super) fn await_call(
        &mut self,
        sim: &mut SimState,
        config: &RunConfig,
        tick: u64,
    ) -> AttemptPhase {
        self.pending_concept
            .take()
            .map(|called| {
                self.concept = called;
                self.last_defense_index =
                    setup::install(sim, config, self.attempt_index, self.concept);
                AttemptPhase::Shifting {
                    stalled_at: tick + SHIFT_STALL_TICKS,
                }
            })
            .unwrap_or(AttemptPhase::PlayCall)
    }

    /// Whether the ball goes this tick: the offense is on its spots, or the
    /// stall guard has run out (see [`SHIFT_STALL_TICKS`]).
    pub(super) fn ready_to_snap(&self, sim: &SimState, tick: u64, stalled_at: u64) -> bool {
        setup::offense_is_set(sim) || tick >= stalled_at
    }
}
