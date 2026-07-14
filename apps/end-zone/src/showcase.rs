//! The deterministic systems showcase: a controller that (only) triggers the
//! play start, the snap, and the scripted throw at fixed tick offsets — every
//! other behavior (routes, blocking, flight, catch, pursuit, contact, camera,
//! juice) emerges from the real systems — plus the headless [`ShowcaseRun`]
//! harness the app, the tests, and the replay proofs all share.

use crate::camera::{CameraDirector, CameraMode, CameraPose};
use crate::config::EndZoneConfig;
use crate::data::{CameraTuning, JuiceTuning};
use crate::events::StampedEvent;
use crate::presentation::snapshot::{capture, PresentationSnapshot};
use crate::presentation::JuiceStack;
use crate::state::{PlayPhase, SimCommand, SimState};

/// Ticks after boot before the showcase play starts by itself.
pub const AUTO_START_DELAY: u64 = 100;
/// Ticks between the play start (formation) and the snap.
pub const SNAP_DELAY: u64 = 80;
/// Post-whistle pause before the showcase resets itself to formation
/// (~5 seconds at 60 Hz).
pub const RESET_DELAY: u64 = 300;
/// The tick [`run_trace`] injects its scripted throw press at (the replay
/// harness's stand-in for the user's SNAP·THROW — the quarterback NEVER
/// throws on his own).
pub const TRACE_THROW_TICK: u64 = 258;

/// Diagnostic + touch input commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticCommand {
    /// Space: start the play, or restart it after completion.
    StartPlay,
    /// R: reset all showcase state to formation (idle until started).
    ResetAll,
    /// The contextual action button (touch A / Enter): snap the ball
    /// pre-snap, order the throw while the quarterback holds it, restart
    /// after the play ends.
    PrimaryAction,
    /// 1–4: force a camera mode; 5: return to automatic direction.
    ForceFormationCamera,
    ForceQuarterbackCamera,
    ForceFlightCamera,
    ForceCarrierCamera,
    AutomaticCamera,
    /// F1: toggle the diagnostic overlays.
    ToggleDebug,
}

/// The scripted timeline stage. The controller only ever starts the play and
/// snaps the ball — the THROW is always the user's (SNAP·THROW / Enter); a
/// quarterback left holding the ball simply gets sacked.
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

/// One stepped frame's outputs.
#[derive(Debug, Clone)]
pub struct StepOutput {
    pub snapshot: PresentationSnapshot,
    pub camera: CameraPose,
    pub camera_mode: CameraMode,
    pub events: Vec<StampedEvent>,
}

/// The headless showcase: simulation + controller + camera director + juice,
/// with no engine scene attached. The browser app wraps this same harness;
/// the determinism tests drive it directly.
#[derive(Debug)]
pub struct ShowcaseRun {
    pub sim: SimState,
    pub controller: ShowcaseController,
    pub director: CameraDirector,
    pub juice: JuiceStack,
    pub debug_enabled: bool,
}

impl ShowcaseRun {
    pub fn new(config: EndZoneConfig) -> Self {
        ShowcaseRun {
            sim: SimState::new(config),
            controller: ShowcaseController::new(),
            director: CameraDirector::new(config.seed, CameraTuning::default()),
            juice: JuiceStack::new(config.seed, JuiceTuning::default()),
            debug_enabled: false,
        }
    }

    /// A match run from one immutable launch configuration: rosters from the
    /// selected league teams, the difficulty profile applied to the opponent,
    /// the named camera/effects profiles applied to presentation. Restarting
    /// with the same config reproduces the same initial authoritative state.
    pub fn new_match(launch: &crate::launch::MatchLaunchConfig) -> Self {
        let setup = crate::launch::resolve_launch(launch);
        ShowcaseRun {
            sim: SimState::new_match(&setup),
            controller: ShowcaseController::new(),
            director: CameraDirector::new(
                launch.seed,
                crate::launch::camera_profile(
                    launch.camera_style,
                    launch.presentation.screen_shake,
                ),
            ),
            juice: JuiceStack::new(
                launch.seed,
                crate::launch::juice_profile(
                    launch.presentation.effects,
                    launch.presentation.flash,
                ),
            ),
            debug_enabled: false,
        }
    }

    /// Advance one fixed tick under the diagnostic commands.
    pub fn step(&mut self, diagnostics: &[DiagnosticCommand]) -> StepOutput {
        let tick = self.sim.tick;
        let mut user_commands: Vec<SimCommand> = Vec::new();
        for command in diagnostics {
            match command {
                DiagnosticCommand::StartPlay => self.controller.request_start(tick),
                DiagnosticCommand::ResetAll => self.controller.request_reset(),
                DiagnosticCommand::ToggleDebug => self.debug_enabled = !self.debug_enabled,
                DiagnosticCommand::PrimaryAction => {
                    // Contextual on the PRE-step state: snap → throw → restart.
                    match self.sim.phase {
                        crate::state::PlayPhase::PreSnap => {
                            user_commands.push(SimCommand::Snap);
                            self.controller.notify_user_snap(tick);
                        }
                        crate::state::PlayPhase::Live => {
                            if self.sim.possession == Some(self.sim.quarterback) {
                                user_commands.push(SimCommand::ThrowNow);
                            }
                        }
                        crate::state::PlayPhase::Ended => self.controller.request_start(tick),
                    }
                }
                _ => {}
            }
        }
        let mut sim_commands = self.controller.step(tick, self.sim.phase);
        sim_commands.extend(user_commands);
        // R additionally puts the sim itself back in formation right away.
        if diagnostics.contains(&DiagnosticCommand::ResetAll) {
            sim_commands.insert(0, SimCommand::ResetPlay);
        }
        let events: Vec<StampedEvent> = self.sim.step(&sim_commands).to_vec();
        let snapshot = capture(&self.sim);
        self.juice.step(&snapshot, &events);
        for command in diagnostics {
            match command {
                DiagnosticCommand::ForceFormationCamera => {
                    self.director
                        .force_mode(CameraMode::FormationWide, &snapshot);
                }
                DiagnosticCommand::ForceQuarterbackCamera => {
                    self.director
                        .force_mode(CameraMode::QuarterbackFollow, &snapshot);
                }
                DiagnosticCommand::ForceFlightCamera => {
                    self.director.force_mode(CameraMode::PassFlight, &snapshot);
                }
                DiagnosticCommand::ForceCarrierCamera => {
                    self.director
                        .force_mode(CameraMode::BallCarrierFollow, &snapshot);
                }
                DiagnosticCommand::AutomaticCamera => self.director.automatic(),
                _ => {}
            }
        }
        let camera = self.director.step(&snapshot, &events);
        StepOutput {
            camera_mode: self.director.effective_mode(),
            snapshot,
            camera,
            events,
        }
    }
}

// The replay artifacts (`ShowcaseTrace`, `run_trace`, the state digest) live
// in `crate::trace`; re-exported here so harnesses keep one import path.
pub use crate::trace::{run_trace, ShowcaseTrace};
