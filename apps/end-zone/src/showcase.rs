//! The headless [`ShowcaseRun`] harness the app, tests, and replay proofs all
//! share. A run is either the AMBIENT loop (one play cycling behind the title,
//! triggered at fixed tick offsets, every other behavior emerging from the real
//! systems) or a real [`AttemptController`] session: the decision-window
//! prototype.

use crate::attempt::{AttemptController, AttemptStep, PlayerChoice};
use crate::camera::{CameraDirector, CameraMode, CameraPose};
use crate::config::EndZoneConfig;
use crate::data::{CameraTuning, JuiceTuning};
use crate::events::StampedEvent;
use crate::launch::{camera_tuning, juice_tuning, resolve_run, RunConfig};
use crate::presentation::snapshot::{capture, PresentationSnapshot};
use crate::presentation::{JuiceStack, LocomotionAnimator, PlayerPose};
use crate::showcase_controller::ShowcaseController;
use crate::state::{SimCommand, SimState};

/// Ambient-showcase beats: boot to play start, play start to snap, and the
/// post-whistle pause before it resets (~2 s at 60 Hz).
pub const AUTO_START_DELAY: u64 = 100;
pub const SNAP_DELAY: u64 = 80;
pub const RESET_DELAY: u64 = 120;
/// The tick [`run_trace`] injects its scripted throw at — the replay harness's
/// stand-in for the user (the QB never throws alone).
pub const TRACE_THROW_TICK: u64 = 258;

pub use crate::controls::DiagnosticCommand;

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

/// The ambient menu showcase or a real decision-window session.
#[derive(Debug)]
enum RunLoop {
    Ambient(ShowcaseController),
    Attempt(Box<AttemptController>, RunConfig),
}

/// The headless run: simulation + run loop + camera director + juice +
/// locomotion, no engine scene attached. The browser app wraps this same
/// harness; the tests drive it directly, and in attempt mode it owns the
/// authoritative attempt state.
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

    /// A real decision-window session from one immutable [`RunConfig`].
    /// Restarting with the same config reproduces the same initial state.
    pub fn new_run(config: &RunConfig) -> Self {
        let setup = resolve_run(config, crate::attempt::PROTOTYPE_HEAT);
        let mut sim = SimState::new_match(&setup);
        sim.install_play(crate::data::prototype::prototype_play());
        let mut controller = AttemptController::new();
        controller.arm(&mut sim, config);
        sim.reset_to_formation(true);
        ShowcaseRun {
            sim,
            run_loop: RunLoop::Attempt(Box::new(controller), *config),
            director: CameraDirector::new(config.seed, camera_tuning(config)),
            juice: JuiceStack::new(config.seed, juice_tuning(config)),
            locomotion: LocomotionAnimator::new(crate::data::LocomotionTuning::default()),
            debug_enabled: false,
        }
    }

    /// The attempt loop's view for this tick, when this is a real session.
    pub fn attempt(&self) -> Option<AttemptStep> {
        match &self.run_loop {
            RunLoop::Attempt(controller, _) => controller.view(self.sim.tick),
            RunLoop::Ambient(_) => None,
        }
    }

    /// Whether the offense is at the line waiting for a play to be called,
    /// where the number keys mean plays rather than reads.
    fn awaiting_call(&self) -> bool {
        self.attempt()
            .is_some_and(|step| matches!(step.phase, crate::attempt::AttemptPhase::PlayCall))
    }

    /// The running session totals, when this is a real session.
    pub fn ledger(&self) -> Option<crate::attempt::AttemptLedger> {
        match &self.run_loop {
            RunLoop::Attempt(controller, _) => Some(*controller.ledger()),
            RunLoop::Ambient(_) => None,
        }
    }

    /// How fast the simulation should advance relative to real time — the
    /// decision window's slow motion, without the simulation knowing about it.
    pub fn time_scale(&self) -> f32 {
        match &self.run_loop {
            RunLoop::Attempt(controller, _) => controller.time_scale(),
            RunLoop::Ambient(_) => 1.0,
        }
    }

    /// The defensive playbook index the current attempt lined up in.
    pub fn last_defense_index(&self) -> Option<usize> {
        match &self.run_loop {
            RunLoop::Attempt(controller, _) => Some(controller.last_defense_index),
            RunLoop::Ambient(_) => None,
        }
    }

    /// Feed the movement stick. Only a committed decision hands the player a
    /// body; while the play develops the simulation owns everyone.
    pub fn set_user_stick(&mut self, stick: axiom::prelude::Vec2) {
        let allowed = match &self.run_loop {
            RunLoop::Ambient(_) => true,
            RunLoop::Attempt(controller, _) => controller.phase().steerable(),
        };
        self.sim.user_stick = match allowed {
            true => stick,
            false => axiom::prelude::Vec2::ZERO,
        };
    }

    /// Pick the offensive concept for the coming snap; false outside pre-snap.
    pub fn select_concept(&mut self, index: usize) -> bool {
        match &mut self.run_loop {
            RunLoop::Attempt(controller, _) => controller.select_concept(index),
            RunLoop::Ambient(_) => false,
        }
    }

    /// Offer a decision to the loop; false if it was stale and dropped.
    pub fn choose(&mut self, choice: PlayerChoice) -> bool {
        match &mut self.run_loop {
            RunLoop::Attempt(controller, _) => controller.choose(choice),
            RunLoop::Ambient(_) => false,
        }
    }

    /// Advance one fixed tick under the diagnostic commands.
    pub fn step(&mut self, diagnostics: &[DiagnosticCommand]) -> StepOutput {
        let tick = self.sim.tick;
        let mut user_commands: Vec<SimCommand> = Vec::new();
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
                // One key, two meanings, split by phase: while the offense is
                // waiting on a call the number picks the PLAY; once the ball is
                // live it picks the READ. Same keys, one mental model.
                DiagnosticCommand::SelectRead(read) if self.awaiting_call() => {
                    self.select_concept(*read);
                }
                DiagnosticCommand::SelectRead(read) => {
                    self.choose(PlayerChoice::Throw(*read));
                }
                DiagnosticCommand::Scramble => {
                    self.choose(PlayerChoice::Scramble);
                }
                DiagnosticCommand::PrimaryAction => self.primary_action(tick, &mut user_commands),
                _ => {}
            }
        }
        let mut sim_commands = match &mut self.run_loop {
            RunLoop::Ambient(controller) => controller.step(tick, self.sim.phase),
            RunLoop::Attempt(controller, config) => controller.step(&mut self.sim, config),
        };
        sim_commands.extend(user_commands);
        // R additionally puts the sim itself back in formation right away.
        if diagnostics.contains(&DiagnosticCommand::ResetAll) {
            sim_commands.insert(0, SimCommand::ResetPlay);
        }
        let events: Vec<StampedEvent> = self.sim.step(&sim_commands).to_vec();
        let mut snapshot = capture(&self.sim);
        snapshot.attempt = self.attempt();
        self.juice.step(&snapshot, &events);
        // Locomotion advances once per TICK, never per render frame.
        let poses = self.locomotion.step(&snapshot, &events);
        self.apply_camera_overrides(diagnostics, &snapshot);
        let camera = self.director.step(&snapshot, &events);
        StepOutput {
            camera_mode: self.director.effective_mode(),
            snapshot,
            camera,
            events,
            poses,
        }
    }

    /// The contextual action button, resolved against the PRE-step state.
    fn primary_action(&mut self, tick: u64, user_commands: &mut Vec<SimCommand>) {
        // In a real session this is the touch twin of the numbered keys.
        if let Some(step) = self.attempt() {
            if step.phase.accepts_choice() {
                self.choose(PlayerChoice::Throw(step.read.best));
                return;
            }
        }
        match self.sim.phase {
            crate::state::PlayPhase::PreSnap => {
                if matches!(self.run_loop, RunLoop::Ambient(_)) {
                    user_commands.push(SimCommand::Snap);
                }
            }
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

    fn apply_camera_overrides(
        &mut self,
        diagnostics: &[DiagnosticCommand],
        snapshot: &PresentationSnapshot,
    ) {
        for command in diagnostics {
            match command {
                DiagnosticCommand::ForceFormationCamera => {
                    self.director
                        .force_mode(CameraMode::FormationWide, snapshot);
                }
                DiagnosticCommand::ForceQuarterbackCamera => {
                    self.director
                        .force_mode(CameraMode::QuarterbackFollow, snapshot);
                }
                DiagnosticCommand::ForceFlightCamera => {
                    self.director.force_mode(CameraMode::PassFlight, snapshot);
                }
                DiagnosticCommand::ForceCarrierCamera => {
                    self.director
                        .force_mode(CameraMode::BallCarrierFollow, snapshot);
                }
                DiagnosticCommand::AutomaticCamera => self.director.automatic(),
                _ => {}
            }
        }
    }
}

// The replay artifacts (`ShowcaseTrace`, `run_trace`, the state digest) live
// in `crate::trace`; re-exported here so harnesses keep one import path.
pub use crate::trace::{run_trace, ShowcaseTrace};
