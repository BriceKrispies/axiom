//! The camera director: consumes typed simulation events and the immutable
//! snapshot, picks the mode (or honors a diagnostic override), smooths the
//! base pose through the spring rig, and adds the impulse stack:
//!
//! `final pose = smoothed base pose + impulse sample`

use axiom::prelude::Vec3;

use crate::data::CameraTuning;
use crate::events::{SimEvent, StampedEvent};
use crate::presentation::snapshot::PresentationSnapshot;

use super::impulse::{CameraImpulse, ImpulseStack};
use super::modes::{desired_pose, CameraMode, CameraPose};
use super::rig::CameraRig;

/// The camera director.
#[derive(Debug)]
pub struct CameraDirector {
    tuning: CameraTuning,
    seed: u64,
    mode: CameraMode,
    /// Diagnostic override (keys 1–4); `None` = automatic direction.
    forced: Option<CameraMode>,
    rig: CameraRig,
    impulses: ImpulseStack,
    /// CatchResolve blend progress in ticks.
    catch_ticks: u32,
    /// Impact emphasis: focus point + remaining ticks + the mode to restore.
    impact_focus: Vec3,
    impact_left: u32,
    return_mode: CameraMode,
    /// History of `(tick, mode)` transitions (bounded; replay-compared).
    history: Vec<(u64, CameraMode)>,
}

/// Most transitions remembered for the trace.
const HISTORY_CAP: usize = 64;

impl CameraDirector {
    pub fn new(seed: u64, tuning: CameraTuning) -> Self {
        CameraDirector {
            tuning,
            seed,
            mode: CameraMode::FormationWide,
            forced: None,
            rig: CameraRig::new(),
            impulses: ImpulseStack::new(),
            catch_ticks: 0,
            impact_focus: Vec3::ZERO,
            impact_left: 0,
            return_mode: CameraMode::FormationWide,
            history: Vec::new(),
        }
    }

    /// The current automatic mode.
    pub fn mode(&self) -> CameraMode {
        self.mode
    }

    /// The mode actually framing the scene (override wins).
    pub fn effective_mode(&self) -> CameraMode {
        self.forced.unwrap_or(self.mode)
    }

    /// The smoothed base pose (impulse-free; proven untouched by shake).
    pub fn base_pose(&self) -> CameraPose {
        self.rig.base()
    }

    /// Active impulses (debug overlay row).
    pub fn active_impulses(&self) -> usize {
        self.impulses.active()
    }

    /// The `(tick, mode)` transition history.
    pub fn history(&self) -> &[(u64, CameraMode)] {
        &self.history
    }

    /// Force a mode (diagnostic keys). Mode-specific guards: PassFlight only
    /// while the ball is airborne, carrier-follow only with possession.
    pub fn force_mode(&mut self, mode: CameraMode, snapshot: &PresentationSnapshot) {
        let allowed = match mode {
            CameraMode::PassFlight => snapshot.ball.is_airborne(),
            CameraMode::BallCarrierFollow => snapshot.possession.is_some(),
            _ => true,
        };
        if allowed {
            self.forced = Some(mode);
        }
    }

    /// Return to automatic direction (key 5).
    pub fn automatic(&mut self) {
        self.forced = None;
    }

    /// React to this tick's events, then produce the final pose.
    pub fn step(&mut self, snapshot: &PresentationSnapshot, events: &[StampedEvent]) -> CameraPose {
        for stamped in events {
            self.observe(snapshot, stamped);
        }
        if self.mode == CameraMode::CatchResolve {
            self.catch_ticks = self.catch_ticks.saturating_add(1);
            if self.catch_ticks >= self.tuning.catch_blend_ticks && snapshot.possession.is_some() {
                self.transition(snapshot.tick, CameraMode::BallCarrierFollow);
            }
        }
        if self.mode == CameraMode::Impact {
            self.impact_left = self.impact_left.saturating_sub(1);
            if self.impact_left == 0 {
                let restore = self.return_mode;
                self.transition(snapshot.tick, restore);
            }
        }

        let mode = self.effective_mode();
        let blend =
            (self.catch_ticks as f32 / self.tuning.catch_blend_ticks.max(1) as f32).min(1.0);
        let desired = desired_pose(mode, snapshot, &self.tuning, self.impact_focus, blend);
        let base = self.rig.step(desired, &self.tuning);
        let sample = self.impulses.step();
        CameraPose {
            eye: base.eye.add(sample.eye_offset),
            target: base.target.add(sample.target_offset),
            fov_degrees: base.fov_degrees + sample.fov_kick,
        }
    }

    fn observe(&mut self, snapshot: &PresentationSnapshot, stamped: &StampedEvent) {
        let impulse_seed = self.seed ^ stamped.id.0;
        match stamped.event {
            SimEvent::PlayStarted { .. } | SimEvent::PlayReset => {
                self.impulses.clear();
                self.catch_ticks = 0;
                self.impact_left = 0;
                self.transition(stamped.tick, CameraMode::FormationWide);
            }
            SimEvent::Snap { .. } => {
                self.transition(stamped.tick, CameraMode::QuarterbackFollow);
            }
            SimEvent::Throw { .. } => {
                self.impulses.push(CameraImpulse::seeded(
                    impulse_seed,
                    Vec3::new(0.0, 0.4, -snapshot.drive_sign),
                    0.12,
                    1.5,
                    14,
                ));
                self.transition(stamped.tick, CameraMode::PassFlight);
            }
            SimEvent::CatchAttempt { .. } => {
                self.catch_ticks = 0;
                self.transition(stamped.tick, CameraMode::CatchResolve);
            }
            SimEvent::CatchCompleted { .. } => {
                self.impulses.push(CameraImpulse::seeded(
                    impulse_seed,
                    Vec3::new(0.3, 0.5, 0.0),
                    0.10,
                    1.0,
                    12,
                ));
                if self.mode != CameraMode::CatchResolve {
                    self.catch_ticks = 0;
                    self.transition(stamped.tick, CameraMode::CatchResolve);
                }
            }
            SimEvent::PossessionChanged { to, .. } => {
                // Possession resolving to a runner while not already blending
                // (e.g. a scramble) → follow the carrier.
                if to.is_some()
                    && to != Some(snapshot.quarterback)
                    && self.mode != CameraMode::CatchResolve
                    && self.mode != CameraMode::Impact
                {
                    self.transition(stamped.tick, CameraMode::BallCarrierFollow);
                }
            }
            SimEvent::TackleContact {
                contact_point,
                contact_direction,
                strength,
                ..
            } => {
                self.impulses.push(CameraImpulse::seeded(
                    impulse_seed,
                    contact_direction.add(Vec3::new(0.0, 0.8, 0.0)),
                    self.tuning.impact_impulse_scale * strength,
                    5.0 * strength,
                    26,
                ));
                self.begin_impact(snapshot.tick, contact_point, strength);
            }
            SimEvent::PlayerAirborne { .. } => {}
            SimEvent::GroundImpact {
                position, strength, ..
            } => {
                self.impulses.push(CameraImpulse::seeded(
                    impulse_seed,
                    Vec3::new(0.2, 1.0, 0.1),
                    self.tuning.impact_impulse_scale * (0.5 + 0.5 * strength),
                    7.0 * strength,
                    self.tuning.impact_recovery_ticks,
                ));
                self.begin_impact(snapshot.tick, position, strength);
            }
            SimEvent::PlayEnded { .. } => {
                // Once any impact emphasis finishes, settle back on the wide
                // formation view; without an impact, cut there now.
                self.return_mode = CameraMode::FormationWide;
                if self.mode != CameraMode::Impact {
                    self.transition(stamped.tick, CameraMode::FormationWide);
                }
            }
            _ => {}
        }
    }

    fn begin_impact(&mut self, tick: u64, focus: Vec3, strength: f32) {
        self.impact_focus = focus;
        self.impact_left = self
            .tuning
            .impact_recovery_ticks
            .saturating_add((strength * 12.0) as u32);
        if self.mode != CameraMode::Impact {
            self.return_mode = match self.mode {
                CameraMode::CatchResolve | CameraMode::PassFlight => CameraMode::BallCarrierFollow,
                other => other,
            };
            self.transition(tick, CameraMode::Impact);
        }
    }

    fn transition(&mut self, tick: u64, mode: CameraMode) {
        if self.mode != mode {
            self.mode = mode;
            if self.history.len() < HISTORY_CAP {
                self.history.push((tick, mode));
            }
        }
    }
}
