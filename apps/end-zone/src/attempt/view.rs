//! The attempt loop's **presentation view** — the one-way window the HUD, the
//! camera and the debug overlay read the loop through, and the same window the
//! agent's observation is built from.
//!
//! Presentation never touches [`AttemptController`] itself. It gets a plain
//! `Copy` snapshot of what the loop is doing, captured once per tick and hung on
//! the immutable [`crate::presentation::snapshot::PresentationSnapshot`]
//! alongside every other presentation input. That is what keeps the app's
//! "presentation cannot mutate simulation" boundary intact now that there is a
//! gameplay layer worth looking at.

use crate::runback::RunbackStatus;

use super::controller::AttemptController;
use super::ledger::AttemptRecord;
use super::phase::AttemptPhase;

/// Everything the presentation layer needs about the attempt in progress, and
/// nothing the simulation owns.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AttemptStep {
    pub phase: AttemptPhase,
    /// 1-based number of the attempt in progress.
    pub attempt: u32,
    /// The offensive concept the attempt is lined up in.
    pub concept: usize,
    /// The running back's live move state — what he is doing, whether he is in
    /// the air, and whether the leap is ready. The HUD's jump pip and the
    /// agent's observation are both built from this one struct, so they can
    /// never disagree about what the player is allowed to do.
    pub runback: RunbackStatus,
    /// The most recently resolved attempt (drives the result card).
    pub last: Option<AttemptRecord>,
}

impl AttemptController {
    /// This tick's presentation view.
    pub fn view(&self, runback: RunbackStatus) -> AttemptStep {
        AttemptStep {
            phase: self.phase,
            attempt: self.attempt_index.max(1),
            concept: self.concept,
            runback,
            last: self.ledger.last,
        }
    }
}
