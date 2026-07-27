//! Calling the play: the pre-snap concept picker.
//!
//! Separate from the loop because it is the one decision made BEFORE the ball
//! is live, and it has a different rule from every other input — it is accepted
//! only at the line, and it changes what the offense IS rather than what the
//! quarterback does with it.

use crate::data::prototype::CONCEPT_COUNT;

use super::controller::AttemptController;
use super::phase::AttemptPhase;

impl AttemptController {
    /// Pick the concept for the next snap. Accepted only PRE-SNAP: once the
    /// ball is live the number keys mean reads, not plays.
    pub fn select_concept(&mut self, index: usize) -> bool {
        let accepted = matches!(self.phase, AttemptPhase::PreSnap { .. });
        self.pending_concept = accepted
            .then(|| index.min(CONCEPT_COUNT - 1))
            .or(self.pending_concept);
        accepted
    }

    /// The concept the offense is set in.
    pub fn concept(&self) -> usize {
        self.concept
    }

}
