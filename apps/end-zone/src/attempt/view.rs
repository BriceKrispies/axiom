//! The attempt loop's **presentation view** — the one-way window the HUD, the
//! target markers and the debug overlay read the loop through.
//!
//! Presentation never touches [`AttemptController`] itself. It gets a plain
//! `Copy` snapshot of what the loop is doing, captured once per tick and hung
//! on the immutable [`crate::presentation::snapshot::PresentationSnapshot`]
//! alongside every other presentation input. That is what keeps the app's
//! "presentation cannot mutate simulation" boundary intact now that there is a
//! gameplay layer worth looking at.

use super::controller::AttemptController;
use super::ledger::AttemptRecord;
use super::phase::AttemptPhase;
use super::read::PlayRead;

/// Everything the presentation layer needs about the attempt in progress, and
/// nothing the simulation owns.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AttemptStep {
    pub phase: AttemptPhase,
    /// This tick's read — who the three targets are and where they stand.
    pub read: PlayRead,
    /// 1-based number of the attempt in progress.
    pub attempt: u32,
    /// Decision windows offered so far this attempt.
    pub windows: u32,
    /// Simulation ticks left in the open window (`0` outside one).
    pub window_left: u64,
    /// The most recently resolved attempt (drives the result card).
    pub last: Option<AttemptRecord>,
    /// The read whose wind-up is being held, and how far it has charged. The
    /// wind-up lives in the simulation (it is measured in ticks and it decides
    /// the throw), so the loop carries it through rather than owning it.
    pub charging: Option<usize>,
    pub charge: f32,
    /// The offensive concept the attempt is set in.
    pub concept: usize,
}

impl AttemptController {
    /// This tick's presentation view. `None` before the loop has read the field
    /// even once — there is genuinely nothing to draw yet.
    pub fn view(&self, tick: u64) -> Option<AttemptStep> {
        self.view_charging(tick, None, 0.0)
    }

    /// The same view, told which read the simulation is winding up on.
    pub fn view_charging(
        &self,
        tick: u64,
        charging: Option<usize>,
        charge: f32,
    ) -> Option<AttemptStep> {
        self.read.map(|read| AttemptStep {
            charging,
            charge,
            concept: self.concept,
            phase: self.phase,
            read,
            attempt: self.attempt_index.max(1),
            windows: self.windows,
            window_left: match self.phase {
                AttemptPhase::DecisionWindow { closes_at, .. } => closes_at.saturating_sub(tick),
                _ => 0,
            },
            last: self.ledger.last,
        })
    }
}
