//! The score-attack drive: the app-local gameplay layer that turns the
//! deterministic play simulation into a survival run. It owns the authoritative
//! down / line-to-gain / score / heat bookkeeping and the inter-play loop, and
//! ends the run on a failed fourth-down conversion. Pure gameplay tuning over
//! the existing simulation — it adds no football rules the sim does not produce.

use crate::ai::{compile_assignments, AssignmentKind};
use crate::data::player::RosterDefinition;
use crate::data::BehaviorTuning;
use crate::field::{z_to_yards_from_own_goal, OffenseFrame};
use crate::identity::PlayerId;
use crate::launch::{resolve_defense, RunConfig, MAX_HEAT};
use crate::showcase::{AUTO_START_DELAY, RESET_DELAY, SNAP_DELAY};
use crate::state::{PlayPhase, SimCommand, SimState};

/// Where a fresh drive starts (yards from the offense's own goal).
pub const DRIVE_START_YARD: f32 = 25.0;
/// Yards to advance for a first down.
pub const YARDS_TO_GAIN: f32 = 10.0;
/// The opponent goal line (a first down there is 1st & goal).
pub const GOAL_YARD: f32 = 100.0;
/// The dead-ball play clock: a play running this long without ending is blown
/// dead as a sack, so the drive always advances (~10 s at 60 Hz).
pub const MAX_PLAY_TICKS: u64 = 600;

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

/// The drive-support mutators the controller drives the simulation with.
impl SimState {
    /// Move the line of scrimmage to `yards_from_own_goal` (`1..=99`) and
    /// recompile the play for the new frame.
    pub fn respot(&mut self, yards_from_own_goal: f32) {
        let clamped = yards_from_own_goal.clamp(1.0, 99.0);
        self.frame = OffenseFrame::at_yard_line(clamped, self.frame.direction);
        self.assignments = compile_assignments(&self.play, &self.frame);
        self.quarterback = self
            .assignments
            .iter()
            .enumerate()
            .find(|(_, a)| matches!(a.kind, AssignmentKind::Quarterback { .. }))
            .map(|(i, _)| PlayerId(i as u8))
            .unwrap_or(PlayerId(0));
    }

    /// Replace the defense roster and shared contact tuning (heat escalation).
    pub fn reload_defense(&mut self, defense: RosterDefinition, tuning: BehaviorTuning) {
        self.rosters.1 = defense;
        self.tuning = tuning;
    }

    /// Blow the play dead where the ball currently is (the sack / dead-ball
    /// path the play clock uses when a held ball never resolves).
    pub fn blow_dead(&mut self) {
        self.end_play(crate::events::PlayEndReason::Tackled);
    }

    /// How far the ball currently sits from the offense's own goal, in yards:
    /// the live carrier's spot, else the ball's resting spot.
    pub fn ball_yard_line(&self) -> f32 {
        let world = self
            .ball
            .carrier()
            .map(|c| self.players[c.index()].pos)
            .unwrap_or(self.ball.pos);
        z_to_yards_from_own_goal(world.z, self.frame.direction)
    }
}

/// The drive loop stage (mirrors the showcase timing, plus resolution + end).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    /// Waiting to kick off the first play.
    Kickoff { start_at: u64 },
    /// Formed up; the snap is scheduled (the player may snap sooner).
    Armed { snap_at: u64 },
    /// The play is live; `dead_at` is the dead-ball deadline.
    Running { dead_at: u64 },
    /// The play ended and the drive resolved; holding the post-whistle beat.
    Whistle { since: u64 },
    /// The run is over — no more plays.
    Over,
}

/// The drive controller: owns the [`DriveState`] and the inter-play loop for a
/// real run. Constructed by the run bootstrap; stepped once per tick.
#[derive(Debug)]
pub struct DriveController {
    stage: Stage,
    pub state: DriveState,
    /// The last resolved play's event, cleared when a new play starts.
    pub last_event: Option<DriveEvent>,
}

impl DriveController {
    /// A controller for a run beginning at `initial_heat`.
    pub fn new(initial_heat: u8) -> Self {
        DriveController {
            stage: Stage::Kickoff {
                start_at: AUTO_START_DELAY,
            },
            state: DriveState::new(initial_heat),
            last_event: None,
        }
    }

    /// The player snapped the ball themselves — cancel the scheduled snap and
    /// arm the dead-ball clock from `tick`.
    pub fn notify_user_snap(&mut self, tick: u64) {
        if matches!(self.stage, Stage::Armed { .. }) {
            self.stage = Stage::Running {
                dead_at: tick + MAX_PLAY_TICKS,
            };
        }
    }

    /// This tick's simulation commands. Reads `sim.phase`/`sim.tick`, resolves
    /// the drive when a play ends, and repositions the offense (heat-scaled
    /// defense included) for the next play — or ends the run.
    pub fn step(&mut self, sim: &mut SimState, config: &RunConfig) -> Vec<SimCommand> {
        let tick = sim.tick;
        let mut commands = Vec::new();
        self.stage = match self.stage {
            Stage::Kickoff { start_at } if tick >= start_at => {
                commands.push(SimCommand::BeginPlay);
                self.last_event = None;
                Stage::Armed {
                    snap_at: tick + SNAP_DELAY,
                }
            }
            Stage::Kickoff { start_at } => Stage::Kickoff { start_at },
            Stage::Armed { snap_at } if tick >= snap_at => {
                commands.push(SimCommand::Snap);
                Stage::Running {
                    dead_at: tick + MAX_PLAY_TICKS,
                }
            }
            Stage::Armed { snap_at } => Stage::Armed { snap_at },
            Stage::Running { dead_at } => {
                let timed_out = tick >= dead_at;
                if sim.phase == PlayPhase::Ended || timed_out {
                    // A held ball that never resolves is blown dead as a sack.
                    if timed_out && sim.phase != PlayPhase::Ended {
                        sim.blow_dead();
                    }
                    let event = self.state.resolve(sim.ball_yard_line());
                    self.last_event = Some(event);
                    if self.state.over {
                        Stage::Over
                    } else {
                        Stage::Whistle { since: tick }
                    }
                } else {
                    Stage::Running { dead_at }
                }
            }
            Stage::Whistle { since } if tick >= since + RESET_DELAY => {
                sim.respot(self.state.los_yard);
                let (defense, tuning) = resolve_defense(config, self.state.heat);
                sim.reload_defense(defense, tuning);
                commands.push(SimCommand::BeginPlay);
                self.last_event = None;
                Stage::Armed {
                    snap_at: tick + SNAP_DELAY,
                }
            }
            Stage::Whistle { since } => Stage::Whistle { since },
            Stage::Over => Stage::Over,
        };
        commands
    }
}
