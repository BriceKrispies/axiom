//! The deterministic systems showcase: a controller that (only) triggers the
//! play start, the snap, and the scripted throw at fixed tick offsets — every
//! other behavior emerges from the real systems — plus the headless
//! [`ShowcaseRun`] harness the app, tests, and replay proofs all share.

use crate::camera::{CameraDirector, CameraMode, CameraPose};
use crate::config::EndZoneConfig;
use crate::data::{CameraTuning, JuiceTuning};
use crate::drive::{DriveController, DriveState, HuddleView, DRIVE_START_YARD};
use crate::events::StampedEvent;
use crate::launch::{camera_tuning, juice_tuning, resolve_run, RunConfig};
use crate::presentation::snapshot::{capture, PresentationSnapshot};
use crate::presentation::{JuiceStack, LocomotionAnimator, PlayerPose};
use crate::showcase_controller::ShowcaseController;
use crate::state::{SimCommand, SimState};

/// Ticks after boot before the showcase play starts by itself.
pub const AUTO_START_DELAY: u64 = 100;
/// Ticks between the play start (formation) and the snap.
pub const SNAP_DELAY: u64 = 80;
/// Post-whistle pause before the play resets to formation (~2 s at 60 Hz).
pub const RESET_DELAY: u64 = 120;
/// The tick [`run_trace`] injects its scripted throw press at (the replay
/// harness's stand-in for the user's SNAP·THROW — the QB never throws alone).
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

/// One stepped frame's outputs.
#[derive(Debug, Clone)]
pub struct StepOutput {
    pub snapshot: PresentationSnapshot,
    pub camera: CameraPose,
    pub camera_mode: CameraMode,
    pub events: Vec<StampedEvent>,
    /// This tick's fully composed per-player poses (locomotion or override).
    pub poses: Vec<PlayerPose>,
}

/// The ambient menu showcase or a real score-attack drive.
#[derive(Debug)]
enum RunLoop {
    Ambient(ShowcaseController),
    Drive(DriveController, RunConfig),
}

/// The headless run: simulation + run loop + camera director + juice +
/// locomotion, no engine scene attached. The browser app wraps this same
/// harness; the tests drive it directly. In [`RunLoop::Drive`] mode it also owns
/// the authoritative score-attack state.
#[derive(Debug)]
pub struct ShowcaseRun {
    pub sim: SimState,
    run_loop: RunLoop,
    pub director: CameraDirector,
    pub juice: JuiceStack,
    pub locomotion: LocomotionAnimator,
    pub debug_enabled: bool,
}

impl ShowcaseRun {
    /// The ambient menu showcase (looping one play behind the title).
    pub fn new(config: EndZoneConfig) -> Self {
        ShowcaseRun {
            sim: SimState::new(config),
            run_loop: RunLoop::Ambient(ShowcaseController::new()),
            director: CameraDirector::new(config.seed, CameraTuning::default()),
            juice: JuiceStack::new(config.seed, JuiceTuning::default()),
            locomotion: LocomotionAnimator::new(crate::data::LocomotionTuning::default()),
            debug_enabled: false,
        }
    }

    /// A real score-attack run from one immutable [`RunConfig`]. Restarting
    /// with the same config reproduces the same initial authoritative state.
    pub fn new_run(config: &RunConfig) -> Self {
        let setup = resolve_run(config, config.initial_heat);
        let mut sim = SimState::new_match(&setup);
        // Start the first drive at the offense's own 25 and form up there.
        sim.respot(DRIVE_START_YARD);
        sim.reset_to_formation(false);
        ShowcaseRun {
            sim,
            run_loop: RunLoop::Drive(DriveController::new(config.initial_heat), *config),
            director: CameraDirector::new(config.seed, camera_tuning(config)),
            juice: JuiceStack::new(config.seed, juice_tuning(config)),
            locomotion: LocomotionAnimator::new(crate::data::LocomotionTuning::default()),
            debug_enabled: false,
        }
    }

    /// The authoritative drive state, when this is a real run.
    pub fn drive_state(&self) -> Option<DriveState> {
        match &self.run_loop {
            RunLoop::Drive(controller, _) => Some(controller.state),
            RunLoop::Ambient(_) => None,
        }
    }

    /// The open pre-snap huddle, when a real run is waiting for a play call.
    pub fn huddle(&self) -> Option<HuddleView> {
        match &self.run_loop {
            RunLoop::Drive(controller, _) => controller.huddle(),
            RunLoop::Ambient(_) => None,
        }
    }

    /// Call offensive play `index` for the open huddle (no-op otherwise).
    pub fn call_play(&mut self, index: usize) {
        if let RunLoop::Drive(controller, _) = &mut self.run_loop {
            controller.call_play(index);
        }
    }

    /// The defensive playbook index the last snap lined up in, when this is a
    /// real run.
    pub fn last_defense_index(&self) -> Option<usize> {
        match &self.run_loop {
            RunLoop::Drive(controller, _) => Some(controller.last_defense_index),
            RunLoop::Ambient(_) => None,
        }
    }

    /// Advance one fixed tick under the diagnostic commands.
    pub fn step(&mut self, diagnostics: &[DiagnosticCommand]) -> StepOutput {
        let tick = self.sim.tick;
        let mut user_commands: Vec<SimCommand> = Vec::new();
        let mut user_snapped = false;
        // A manual snap is only honored once a play has been called: in a real
        // run the drive must be armed; the ambient showcase snaps on its own beat.
        let snap_allowed = match &self.run_loop {
            RunLoop::Ambient(_) => true,
            RunLoop::Drive(controller, _) => controller.armed(),
        };
        for command in diagnostics {
            match command {
                DiagnosticCommand::ToggleDebug => self.debug_enabled = !self.debug_enabled,
                DiagnosticCommand::StartPlay => {
                    if let RunLoop::Ambient(controller) = &mut self.run_loop {
                        controller.request_start(tick);
                    }
                }
                DiagnosticCommand::ResetAll => {
                    if let RunLoop::Ambient(controller) = &mut self.run_loop {
                        controller.request_reset();
                    }
                }
                DiagnosticCommand::PrimaryAction => {
                    // Contextual on the PRE-step state: snap → throw → restart.
                    match self.sim.phase {
                        crate::state::PlayPhase::PreSnap if snap_allowed => {
                            user_commands.push(SimCommand::Snap);
                            user_snapped = true;
                        }
                        crate::state::PlayPhase::PreSnap => {}
                        crate::state::PlayPhase::Live => {
                            if self.sim.possession == Some(self.sim.quarterback) {
                                user_commands.push(SimCommand::ThrowNow);
                            }
                        }
                        crate::state::PlayPhase::Ended => {
                            if let RunLoop::Ambient(controller) = &mut self.run_loop {
                                controller.request_start(tick);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        let mut sim_commands = match &mut self.run_loop {
            RunLoop::Ambient(controller) => controller.step(tick, self.sim.phase),
            RunLoop::Drive(controller, config) => {
                if user_snapped {
                    controller.notify_user_snap(self.sim.tick);
                }
                controller.step(&mut self.sim, config)
            }
        };
        sim_commands.extend(user_commands);
        // R additionally puts the sim itself back in formation right away.
        if diagnostics.contains(&DiagnosticCommand::ResetAll) {
            sim_commands.insert(0, SimCommand::ResetPlay);
        }
        let events: Vec<StampedEvent> = self.sim.step(&sim_commands).to_vec();
        let mut snapshot = capture(&self.sim);
        if let RunLoop::Drive(controller, _) = &self.run_loop {
            snapshot.drive = Some(controller.state);
            snapshot.to_gain_z = Some(crate::field::yard_line_to_z(
                controller.state.first_down_yard,
                self.sim.frame.direction,
            ));
        }
        self.juice.step(&snapshot, &events);
        // Advance locomotion once per tick (never per render frame) so a paused
        // frame re-presents the same poses; feeds off the resolved snapshot.
        let poses = self.locomotion.step(&snapshot, &events);
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
            poses,
        }
    }
}

// The replay artifacts (`ShowcaseTrace`, `run_trace`, the state digest) live
// in `crate::trace`; re-exported here so harnesses keep one import path.
pub use crate::trace::{run_trace, ShowcaseTrace};
