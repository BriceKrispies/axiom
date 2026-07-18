//! The ambient menu showcase's scripted timeline controller: it only ever
//! starts the play, snaps the ball, and (after the whistle) auto-resets to
//! formation at fixed tick offsets. The THROW is always the user's — a
//! quarterback left holding the ball simply gets sacked. Every other behavior
//! emerges from the real systems. Split out of [`crate::showcase`] so the run
//! harness stays narrowly owned.

use crate::showcase::{AUTO_START_DELAY, RESET_DELAY, SNAP_DELAY};
use crate::state::{PlayPhase, SimCommand};

/// The scripted timeline stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    /// Waiting for the auto-start (or Space).
    Idle { start_at: Option<u64> },
    /// Play begun; snap scheduled.
    Armed { snap_at: u64 },
    /// Snapped; play running to its end.
    Running,
    /// Play over; holding the post-whistle beat, then auto-resetting.
    Done { since: u64 },
}

/// The deterministic showcase controller.
#[derive(Debug)]
pub struct ShowcaseController {
    stage: Stage,
}

impl ShowcaseController {
    pub fn new() -> Self {
        ShowcaseController {
            stage: Stage::Idle {
                start_at: Some(AUTO_START_DELAY),
            },
        }
    }

    /// Space pressed: start now (idle) or restart (done/running).
    pub fn request_start(&mut self, tick: u64) {
        self.stage = Stage::Idle {
            start_at: Some(tick),
        };
    }

    /// R pressed: back to formation and idle (no schedule until Space).
    pub fn request_reset(&mut self) {
        self.stage = Stage::Idle { start_at: None };
    }

    /// The user snapped the ball themselves: cancel any pending auto
    /// start/snap — the play is running.
    pub fn notify_user_snap(&mut self, _tick: u64) {
        self.stage = Stage::Running;
    }

    /// The sim commands for this tick.
    pub fn step(&mut self, tick: u64, phase: PlayPhase) -> Vec<SimCommand> {
        let mut commands = Vec::new();
        self.stage = match self.stage {
            Stage::Idle { start_at: Some(at) } if tick >= at => {
                commands.push(SimCommand::BeginPlay);
                Stage::Armed {
                    snap_at: tick + SNAP_DELAY,
                }
            }
            Stage::Idle { start_at } => Stage::Idle { start_at },
            Stage::Armed { snap_at } if tick >= snap_at => {
                commands.push(SimCommand::Snap);
                Stage::Running
            }
            Stage::Armed { snap_at } => Stage::Armed { snap_at },
            Stage::Running if phase == PlayPhase::Ended => Stage::Done { since: tick },
            Stage::Running => Stage::Running,
            Stage::Done { since } if tick >= since + RESET_DELAY => {
                // The post-whistle beat is over: back to formation, with the
                // next snap scheduled exactly like the boot sequence.
                commands.push(SimCommand::BeginPlay);
                Stage::Armed {
                    snap_at: tick + SNAP_DELAY,
                }
            }
            Stage::Done { since } => Stage::Done { since },
        };
        commands
    }
}

impl Default for ShowcaseController {
    fn default() -> Self {
        ShowcaseController::new()
    }
}
