//! Deterministic throw solving and trajectory prediction. The solve produces
//! the release velocity the physics body is handed; prediction is the
//! closed-form ballistic estimate the camera and AI read (`FlightInfo`) — the
//! authoritative flight itself is integrated by the physics facade.

use axiom::prelude::Vec3;

use crate::data::BehaviorTuning;
use crate::identity::PlayerId;

/// Everything downstream systems need to know about a live pass.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlightInfo {
    /// Who the pass is intended for.
    pub intended: PlayerId,
    /// Release position, world yards.
    pub release: Vec3,
    /// Release velocity, yd/s.
    pub velocity: Vec3,
    /// Predicted arrival point (catch height), world yards.
    pub target: Vec3,
    /// Tick the ball was released.
    pub release_tick: u64,
    /// Predicted flight time, ticks.
    pub eta_ticks: u32,
}

impl FlightInfo {
    /// The predicted arrival tick.
    pub fn arrival_tick(&self) -> u64 {
        self.release_tick + u64::from(self.eta_ticks)
    }
}

/// Solve a throw from `release` to `target`: flight time from horizontal
/// distance at the tuned pass speed (clamped to a minimum), horizontal
/// velocity to cover it exactly, vertical velocity to meet the target height
/// under gravity. Deterministic: same inputs, same release state.
pub fn solve_throw(release: Vec3, target: Vec3, tuning: &BehaviorTuning) -> (Vec3, u32) {
    let flat = Vec3::new(target.x - release.x, 0.0, target.z - release.z);
    let distance = flat.length();
    let seconds = (distance / tuning.pass_speed).max(tuning.min_flight_ticks as f32 / 60.0);
    let eta_ticks = (seconds * 60.0).round().max(1.0) as u32;
    let t = eta_ticks as f32 / 60.0;
    let vy = (target.y - release.y + 0.5 * tuning.gravity * t * t) / t;
    let v = Vec3::new(flat.x / t, vy, flat.z / t);
    (v, eta_ticks)
}

/// Closed-form ballistic position `seconds` after release (the prediction the
/// camera and debug trajectory read; the physics body is the authority).
pub fn predict_position(release: Vec3, velocity: Vec3, gravity: f32, seconds: f32) -> Vec3 {
    Vec3::new(
        release.x + velocity.x * seconds,
        release.y + velocity.y * seconds - 0.5 * gravity * seconds * seconds,
        release.z + velocity.z * seconds,
    )
}
