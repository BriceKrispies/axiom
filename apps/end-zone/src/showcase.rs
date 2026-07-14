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
/// Ticks between the snap and the throw order.
pub const THROW_DELAY: u64 = 78;

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

/// The scripted timeline stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    /// Waiting for the auto-start (or Space).
    Idle { start_at: Option<u64> },
    /// Play begun; snap scheduled.
    Armed { snap_at: u64 },
    /// Snapped; throw order scheduled.
    Snapped { throw_at: u64 },
    /// Ball is out (or thrown order given); play running to its end.
    Running,
    /// Play over; waiting for Space.
    Done,
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
    /// start/snap and schedule only the scripted throw order.
    pub fn notify_user_snap(&mut self, tick: u64) {
        self.stage = Stage::Snapped {
            throw_at: tick + THROW_DELAY,
        };
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
                Stage::Snapped {
                    throw_at: tick + THROW_DELAY,
                }
            }
            Stage::Armed { snap_at } => Stage::Armed { snap_at },
            Stage::Snapped { throw_at } if tick >= throw_at => {
                commands.push(SimCommand::ThrowNow);
                Stage::Running
            }
            Stage::Snapped { throw_at } => Stage::Snapped { throw_at },
            Stage::Running if phase == PlayPhase::Ended => Stage::Done,
            Stage::Running => Stage::Running,
            Stage::Done => Stage::Done,
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

/// Deterministic artifacts of one showcase run — what the replay tests
/// compare bit-for-bit.
#[derive(Debug, Clone, PartialEq)]
pub struct ShowcaseTrace {
    pub events: Vec<StampedEvent>,
    /// Ball position each tick.
    pub ball_samples: Vec<axiom::prelude::Vec3>,
    /// Possession transitions `(tick, holder)`.
    pub possession: Vec<(u64, Option<crate::identity::PlayerId>)>,
    /// Every player's intent, every tick.
    pub intents: Vec<Vec<crate::ai::PlayerIntent>>,
    /// Camera mode transitions `(tick, mode)`.
    pub camera_modes: Vec<(u64, CameraMode)>,
    /// The final camera pose each tick.
    pub camera_poses: Vec<CameraPose>,
    pub final_digest: Vec<u32>,
}

/// Run the whole showcase for `ticks` fixed steps with no diagnostic input
/// and collect the deterministic artifacts.
pub fn run_trace(config: EndZoneConfig, ticks: u64) -> ShowcaseTrace {
    let mut run = ShowcaseRun::new(config);
    let mut trace = ShowcaseTrace {
        events: Vec::new(),
        ball_samples: Vec::new(),
        possession: Vec::new(),
        intents: Vec::new(),
        camera_modes: Vec::new(),
        camera_poses: Vec::new(),
        final_digest: Vec::new(),
    };
    let mut last_possession = None;
    for _ in 0..ticks {
        let output = run.step(&[]);
        trace.events.extend_from_slice(&output.events);
        trace.ball_samples.push(output.snapshot.ball.pos);
        if output.snapshot.possession != last_possession {
            last_possession = output.snapshot.possession;
            trace
                .possession
                .push((output.snapshot.tick, last_possession));
        }
        trace
            .intents
            .push(output.snapshot.players.iter().map(|p| p.intent).collect());
        trace.camera_poses.push(output.camera);
    }
    trace.camera_modes = run.director.history().to_vec();
    trace.final_digest = run.sim.digest();
    trace
}
