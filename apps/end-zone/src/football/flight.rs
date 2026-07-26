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

/// The point a pass should be thrown AT so it meets a moving receiver — the
/// classic intercept solve, done in closed form.
///
/// Aiming at where a receiver *is* always throws behind him; the fix is to
/// solve for the time `t` at which the ball and the receiver occupy the same
/// spot. On the ground plane, with `D` the receiver's offset from the release
/// point, `V` his velocity and `s` the ball's horizontal speed, that is
/// `|D + V·t| = s·t`, which squares to a plain quadratic:
///
/// ```text
///   (V·V − s²)·t² + 2(D·V)·t + (D·D) = 0
/// ```
///
/// The smallest positive root is the first moment the ball can arrive. This
/// replaces a two-round fixed-point guess that never fully converged — and
/// converged worst exactly where it mattered most, on long throws to a
/// sprinting receiver, because a longer flight makes the guess's error larger.
///
/// Degenerate cases are handled rather than branched around blindly: a receiver
/// running away at (or beyond) ball speed has no intercept at all, and a
/// stationary receiver reduces to `t = |D| / s`. Both fall back to a
/// straight-line time, so the quarterback still releases the ball.
pub fn lead_point(release: Vec3, position: Vec3, velocity: Vec3, speed: f32) -> Vec3 {
    let d = Vec3::new(position.x - release.x, 0.0, position.z - release.z);
    let v = Vec3::new(velocity.x, 0.0, velocity.z);
    let s = speed.max(1.0);
    let a = v.dot(v) - s * s;
    let b = 2.0 * d.dot(v);
    let c = d.dot(d);
    let straight = c.sqrt() / s;

    // |a| ~ 0 means the receiver is running at exactly ball speed: the quadratic
    // collapses to a linear one.
    let linear = (b.abs() > 1.0e-4).then(|| -c / b);
    let quadratic = || {
        let disc = b * b - 4.0 * a * c;
        (disc >= 0.0)
            .then(|| {
                let root = disc.sqrt();
                let (t0, t1) = ((-b + root) / (2.0 * a), (-b - root) / (2.0 * a));
                // Smallest strictly-positive root; `None` when the receiver
                // simply cannot be caught up with.
                [t0.min(t1), t0.max(t1)]
                    .into_iter()
                    .find(|t| *t > 1.0e-3)
            })
            .flatten()
    };
    let t = match a.abs() < 1.0e-4 {
        true => linear,
        false => quadratic(),
    }
    .filter(|t| t.is_finite() && *t > 0.0)
    .unwrap_or(straight)
    // A bounded flight: a degenerate solve can never launch the ball into orbit.
    .clamp(0.0, MAX_LEAD_SECONDS);
    position.add(v.mul_scalar(t))
}

/// The longest lead the solver will ever produce, seconds.
const MAX_LEAD_SECONDS: f32 = 2.5;

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
