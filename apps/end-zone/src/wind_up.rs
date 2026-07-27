//! The WIND-UP bridge: turning a held read into charge in the simulation, and
//! a release into a committed decision.
//!
//! It sits between the input map and the loop because it is the one place that
//! needs both: which receiver a read number currently means (the loop) and how
//! much a tick of holding is worth (the time scale). Kept out of
//! [`crate::showcase`] so that file stays a description of stepping a run.

use crate::attempt::PlayerChoice;
use crate::showcase::ShowcaseRun;

impl ShowcaseRun {
    /// Whether the offense is still at the line, where the number keys pick a
    /// concept rather than a read.
    pub(crate) fn pre_snap(&self) -> bool {
        self.attempt()
            .map(|s| matches!(s.phase, crate::attempt::AttemptPhase::PreSnap { .. }))
            .unwrap_or(false)
    }


    /// The player a held read aims at, while the reads are live and the
    /// quarterback still has the ball.
    pub(crate) fn charge_target(&self, read: usize) -> Option<crate::identity::PlayerId> {
        let step = self.attempt().filter(|s| s.phase.accepts_choice())?;
        (self.sim.possession == Some(self.sim.quarterback)).then(|| step.read.target(read))
    }

    /// Tell the loop the wind-up was let go: it leaves the window and records
    /// WHICH read was taken. The simulation owns the throw itself.
    pub(crate) fn note_charged_choice(&mut self) {
        let Some(step) = self.attempt() else { return };
        let target = self.sim.charge_target();
        let read =
            (0..crate::data::prototype::READ_COUNT).find(|r| Some(step.read.target(*r)) == target);
        if let Some(read) = read {
            self.choose(PlayerChoice::ThrowCharged(read));
        }
    }


    /// Charge per dilated tick, so the meter fills at a constant rate on the
    /// PLAYER's clock instead of crawling through a decision window.
    pub(crate) fn charge_gain(&self) -> u32 {
        ((1.0 / self.time_scale().max(0.02)).round() as u32).clamp(1, 16)
    }
}
