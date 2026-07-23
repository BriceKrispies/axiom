//! The drive controller: the inter-play loop over a real run. It owns the
//! [`DriveState`], the pre-snap huddle where the player calls a play, and the
//! deterministic composition of that call against a chosen defensive answer.

use crate::ai::{select_defense, variation_key};
use crate::data::{offensive_playbook, PlayDefinition};
use crate::events::PlayEndReason;
use crate::identity::TeamId;
use crate::launch::{resolve_defense, RunConfig};
use crate::showcase::{RESET_DELAY, SNAP_DELAY};
use crate::state::{PlayPhase, SimCommand, SimState};

use super::{DriveEvent, DriveState, HUDDLE_AUTO_DELAY, MAX_PLAY_TICKS};

/// The pre-snap huddle the shell surfaces to the player: the situation to call a
/// play against. The play list itself is the static [`offensive_playbook`]; this
/// carries only the down/distance so the frontend never queries the simulation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HuddleView {
    pub down: u8,
    pub yards_to_go: f32,
    pub goal_to_go: bool,
    /// The number of selectable offensive plays.
    pub play_count: usize,
    /// The play the offense breaks with if the player does not choose.
    pub default_index: usize,
}

/// The drive loop stage (mirrors the showcase timing, plus the huddle,
/// resolution, and end).
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
    /// The huddle is open: waiting for the player's play call, or for the
    /// auto-break deadline that keeps a hands-off run moving.
    Huddle { auto_at: u64 },
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
    /// The offensive play the player called this huddle, if any.
    pending_call: Option<usize>,
    /// Monotonic snap counter — part of the defensive variation key so the
    /// defense's look changes snap to snap even for a repeated play call.
    snap_index: u64,
    /// The defensive playbook index the last snap lined up in (inspection/HUD).
    pub last_defense_index: usize,
}

impl DriveController {
    /// A controller for a run beginning at `initial_heat`. The huddle opens on
    /// the very first step (`start_at: 0`): a run begins at the play-call, never
    /// with a live, snappable field the player never asked for.
    pub fn new(initial_heat: u8) -> Self {
        DriveController {
            stage: Stage::Kickoff { start_at: 0 },
            state: DriveState::new(initial_heat),
            last_event: None,
            pending_call: None,
            snap_index: 0,
            last_defense_index: 0,
        }
    }

    /// Whether the ball may be snapped: only once a play has been called and the
    /// offense is armed at the line. Before that (kickoff or an open huddle) a
    /// snap press is ignored, so gameplay can never start behind the huddle.
    pub fn armed(&self) -> bool {
        matches!(self.stage, Stage::Armed { .. })
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

    /// The open huddle, if the drive is currently waiting for a play call.
    pub fn huddle(&self) -> Option<HuddleView> {
        matches!(self.stage, Stage::Huddle { .. }).then(|| HuddleView {
            down: self.state.down,
            yards_to_go: self.state.yards_to_go(),
            goal_to_go: self.state.goal_to_go(),
            play_count: offensive_playbook().len(),
            default_index: 0,
        })
    }

    /// The player called offensive play `index`. Recorded for the open huddle;
    /// ignored (harmlessly) once the play is live. Clamped to the playbook.
    pub fn call_play(&mut self, index: usize) {
        if matches!(self.stage, Stage::Huddle { .. }) {
            self.pending_call = Some(index.min(offensive_playbook().len() - 1));
        }
    }

    /// Compose and install the called (or default) play against a deterministic
    /// defensive answer, then reload the heat-scaled defensive roster.
    fn break_huddle(&mut self, sim: &mut SimState, config: &RunConfig, index: usize) {
        let playbook = offensive_playbook();
        let offense = &playbook[index.min(playbook.len() - 1)];
        let key = variation_key(config.seed, self.snap_index, self.state.down);
        let selection = select_defense(
            offense.tag,
            self.state.down,
            self.state.yards_to_go(),
            self.state.heat,
            key,
        );
        self.last_defense_index = selection.index;
        let play = PlayDefinition::compose(
            offense,
            &selection.call,
            TeamId(0),
            sim.frame.direction,
            self.state.los_yard,
        );
        sim.install_play(play);
        let (defense, tuning) = resolve_defense(config, self.state.heat);
        sim.reload_defense(defense, tuning);
        self.pending_call = None;
        self.snap_index += 1;
    }

    /// This tick's simulation commands. Reads `sim.phase`/`sim.tick`, resolves
    /// the drive when a play ends, and repositions the offense (heat-scaled
    /// defense included) for the next play — or ends the run.
    pub fn step(&mut self, sim: &mut SimState, config: &RunConfig) -> Vec<SimCommand> {
        let tick = sim.tick;
        let mut commands = Vec::new();
        self.stage = match self.stage {
            Stage::Kickoff { start_at } if tick >= start_at => {
                self.last_event = None;
                self.open_huddle(tick)
            }
            Stage::Kickoff { start_at } => Stage::Kickoff { start_at },
            Stage::Huddle { auto_at } => {
                // Break the huddle on the player's call, or on the deadline that
                // keeps a hands-off run moving with the default play.
                match self.pending_call.or((tick >= auto_at).then_some(0)) {
                    Some(index) => {
                        self.break_huddle(sim, config, index);
                        commands.push(SimCommand::BeginPlay);
                        Stage::Armed {
                            snap_at: tick + SNAP_DELAY,
                        }
                    }
                    None => Stage::Huddle { auto_at },
                }
            }
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
                    // A turnover ends the run for now. The possession-flip
                    // alternative lands HERE: instead of ending, re-spot the
                    // drive with the intercepting team on offense.
                    let event = if sim.end_reason == Some(PlayEndReason::Intercepted) {
                        self.state.end_on_turnover();
                        DriveEvent::Intercepted
                    } else {
                        self.state.resolve(sim.ball_yard_line())
                    };
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
                self.last_event = None;
                self.open_huddle(tick)
            }
            Stage::Whistle { since } => Stage::Whistle { since },
            Stage::Over => Stage::Over,
        };
        commands
    }

    /// Open the huddle from `tick`, clearing any stale play call.
    fn open_huddle(&mut self, tick: u64) -> Stage {
        self.pending_call = None;
        Stage::Huddle {
            auto_at: tick + HUDDLE_AUTO_DELAY,
        }
    }
}
