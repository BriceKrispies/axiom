//! [`TimelineReplayState`] — the typed placeholder state of the Timeline / Replay
//! panel: an ordered list of ticks and an optional replay request.
//!
//! Each [`TimelineTick`] pairs a tick with the optional [`StableHash`] of the
//! snapshot taken at it (a diagnostic index over opaque bytes, never a proof).
//! The panel also holds an optional [`ReplayRequest`] so Replay-mode state
//! references a replay request purely **as data** — the panel replays nothing.

use axiom_kernel::{StableHash, Tick};

use crate::replay_request::ReplayRequest;

/// One placeholder timeline entry: a tick and the optional snapshot-hash index
/// taken at it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimelineTick {
    /// The tick this entry marks.
    pub tick: Tick,
    /// The snapshot-hash index at this tick, if a snapshot was taken.
    pub snapshot: Option<StableHash>,
}

impl TimelineTick {
    /// Build a placeholder timeline entry.
    #[must_use]
    pub fn new(tick: Tick, snapshot: Option<StableHash>) -> Self {
        TimelineTick { tick, snapshot }
    }
}

/// The Timeline / Replay panel state: an ordered list of ticks plus an optional
/// replay request held as data. `Default` is empty (no ticks, no replay).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TimelineReplayState {
    ticks: Vec<TimelineTick>,
    replay: Option<ReplayRequest>,
}

impl TimelineReplayState {
    /// Append a timeline entry, preserving tick order exactly.
    pub fn mark_tick(&mut self, tick: TimelineTick) {
        self.ticks.push(tick);
    }

    /// The timeline entries, in the order they were marked.
    #[must_use]
    pub fn ticks(&self) -> &[TimelineTick] {
        &self.ticks
    }

    /// The attached replay request, if any — referenced as data, never run.
    #[must_use]
    pub fn replay(&self) -> Option<&ReplayRequest> {
        self.replay.as_ref()
    }

    /// Attach a replay request as data.
    pub fn set_replay(&mut self, replay: Option<ReplayRequest>) {
        self.replay = replay;
    }
}
