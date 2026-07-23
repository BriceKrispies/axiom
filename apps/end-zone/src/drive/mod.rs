//! The score-attack drive: the app-local gameplay layer that turns the
//! deterministic play simulation into a survival run. It owns the authoritative
//! down / line-to-gain / score / heat bookkeeping and the inter-play loop, and
//! ends the run on a failed fourth-down conversion. Pure gameplay tuning over
//! the existing simulation — it adds no football rules the sim does not produce.
//!
//! Three owners: [`DriveState`] (the bookkeeping, here), [`DriveController`] (the
//! inter-play loop + huddle, in [`controller`]), and the `SimState` drive-support
//! mutators (respot / install-play / reload-defense, in [`sim_support`]).

mod controller;
mod sim_support;

pub use controller::{DriveController, HuddleView};

use crate::launch::MAX_HEAT;

/// Where a fresh drive starts (yards from the offense's own goal).
pub const DRIVE_START_YARD: f32 = 25.0;
/// Yards to advance for a first down.
pub const YARDS_TO_GAIN: f32 = 10.0;
/// The opponent goal line (a first down there is 1st & goal).
pub const GOAL_YARD: f32 = 100.0;
/// The dead-ball play clock: a play running this long without ending is blown
/// dead as a sack, so the drive always advances (~10 s at 60 Hz).
pub const MAX_PLAY_TICKS: u64 = 600;
/// How long the huddle stays open waiting for a play call before the offense
/// breaks with its default play — so a hands-off run always advances (~6 s).
pub const HUDDLE_AUTO_DELAY: u64 = 360;

/// The authoritative score-attack state. Every field is derived from resolved
/// play outcomes — there is no duplicate counter anywhere else.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DriveState {
    /// The current down, `1..=4`.
    pub down: u8,
    /// The line of scrimmage for the current down (yards from own goal).
    pub los_yard: f32,
    /// The line to gain: reaching it awards a first down (capped at the goal).
    pub first_down_yard: f32,
    /// Arcade score.
    pub score: u32,
    /// Touchdowns scored this run.
    pub touchdowns: u32,
    /// First downs earned this run.
    pub first_downs: u32,
    /// The longest single gain this run, yards.
    pub longest_play: f32,
    /// The current defensive heat level, `1..=MAX_HEAT`.
    pub heat: u8,
    /// Whether the run has ended (failed fourth-down conversion).
    pub over: bool,
}

/// What a resolved play did to the drive (drives presentation + tests).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriveEvent {
    /// A first down was earned; the down count reset.
    FirstDown,
    /// A touchdown was scored; a new drive begins.
    Touchdown,
    /// No conversion; the next down begins.
    NextDown,
    /// Fourth-down conversion failed; the run is over.
    RunOver,
    /// The defense intercepted the pass — a turnover. The run ends on it for now
    /// (the possession-flip alternative would continue with the defense on
    /// offense from the interception spot).
    Intercepted,
}

/// The final summary shown on the game-over screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunSummary {
    pub score: u32,
    pub touchdowns: u32,
    pub first_downs: u32,
    /// Longest gain, whole yards.
    pub longest_play: u32,
}

impl DriveState {
    /// A fresh drive at a run's initial heat.
    pub fn new(initial_heat: u8) -> Self {
        DriveState {
            down: 1,
            los_yard: DRIVE_START_YARD,
            first_down_yard: (DRIVE_START_YARD + YARDS_TO_GAIN).min(GOAL_YARD),
            score: 0,
            touchdowns: 0,
            first_downs: 0,
            longest_play: 0.0,
            heat: initial_heat.clamp(1, MAX_HEAT),
            over: false,
        }
    }

    /// Yards still needed for a first down on the current down.
    pub fn yards_to_go(&self) -> f32 {
        (self.first_down_yard - self.los_yard).max(0.0)
    }

    /// Whether the line to gain is the goal line (1st/2nd… & GOAL).
    pub fn goal_to_go(&self) -> bool {
        self.first_down_yard >= GOAL_YARD - 0.001
    }

    /// End the run on a turnover (an interception). Counters freeze where they
    /// stand. This is the seam for changing possession later: instead of ending,
    /// the drive would re-spot for the intercepting team on offense.
    pub fn end_on_turnover(&mut self) {
        self.over = true;
    }

    /// The run summary snapshot.
    pub fn summary(&self) -> RunSummary {
        RunSummary {
            score: self.score,
            touchdowns: self.touchdowns,
            first_downs: self.first_downs,
            longest_play: self.longest_play.max(0.0).round() as u32,
        }
    }

    /// Resolve a play that ended with the ball spotted at `ball_yard` (yards
    /// from the offense's own goal). Updates every counter and returns what
    /// happened. Heat is always re-derived from the run's progress.
    pub fn resolve(&mut self, ball_yard: f32) -> DriveEvent {
        let spot = ball_yard.clamp(1.0, GOAL_YARD + 10.0);
        let gained = spot - self.los_yard;
        self.longest_play = self.longest_play.max(gained.max(0.0));
        self.score += (gained.max(0.0).round() as u32) * 10;

        let event = if spot >= GOAL_YARD {
            // Touchdown: six points, a big arcade bonus, and a fresh drive.
            self.touchdowns += 1;
            self.score += 700;
            self.los_yard = DRIVE_START_YARD;
            self.first_down_yard = (DRIVE_START_YARD + YARDS_TO_GAIN).min(GOAL_YARD);
            self.down = 1;
            DriveEvent::Touchdown
        } else if spot >= self.first_down_yard - 0.001 {
            // First down: reset the chains at the new spot.
            self.first_downs += 1;
            self.score += 250;
            self.los_yard = spot;
            self.first_down_yard = (spot + YARDS_TO_GAIN).min(GOAL_YARD);
            self.down = 1;
            DriveEvent::FirstDown
        } else if self.down >= 4 {
            // Failed the fourth-down conversion — the run ends.
            self.los_yard = spot.max(1.0);
            self.over = true;
            DriveEvent::RunOver
        } else {
            // Short of the line: next down from the new spot.
            self.los_yard = spot.max(1.0);
            self.down += 1;
            DriveEvent::NextDown
        };

        self.heat = heat_for(self.touchdowns, self.first_downs);
        event
    }
}

/// The heat a run has reached from its progress (bounded to `1..=MAX_HEAT`).
fn heat_for(touchdowns: u32, first_downs: u32) -> u8 {
    (1 + touchdowns + first_downs / 3).min(u32::from(MAX_HEAT)) as u8
}
