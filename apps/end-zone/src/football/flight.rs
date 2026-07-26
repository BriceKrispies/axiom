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

/// The launch elevation every pass leaves the hand at, radians (~12°). Fixed on
/// purpose: with the angle constant, **power alone decides range**, which is
/// what makes a charged throw legible. The player learns one relationship
/// instead of two coupled ones.
pub const LAUNCH_ELEVATION: f32 = 0.21;

/// The launch speed that lands a pass `range` yards away, from the level-ground
/// range `R = v²·sin(2θ)/g`.
pub fn speed_for_range(range: f32, gravity: f32) -> f32 {
    let spread = (2.0 * LAUNCH_ELEVATION).sin().max(0.01);
    (range.max(0.0) * gravity.max(0.01) / spread).sqrt()
}

/// How far in front of (or behind) the receiver a full / empty wind-up puts the
/// ball, yards.
///
/// The wind-up is **placement, not range**. Every throw is solved to reach the
/// receiver; power only slides the arrival point along his path. A perfect
/// half-charge hits him in stride, an over-throw leads him further into space,
/// an under-throw arrives behind him. That keeps a mistimed release a
/// *contested* ball rather than a pass that lands twenty yards short of anyone
/// — which is what "throw harder" produced when power drove distance directly.
pub const LEAD_BIAS_YARDS: f32 = 3.2;

/// The aim point and launch velocity for a throw at `power` (`0.5` is on the
/// money). ONE function, shared by the release and the on-field preview, so the
/// arc the player is shown is the arc the ball actually flies.
pub fn aim_and_velocity(
    release: Vec3,
    receiver_pos: Vec3,
    receiver_vel: Vec3,
    power: f32,
    gravity: f32,
    tuning: &BehaviorTuning,
) -> (Vec3, Vec3) {
    // Solve the intercept at a nominal speed first: this is the point that
    // actually meets the receiver, and it is where a perfect throw goes.
    let base = lead_point(release, receiver_pos, receiver_vel, tuning.pass_speed);
    // Bias along his heading — in front on a big wind-up, behind on a rushed
    // one. A stationary receiver has no heading, so the throw simply finds him.
    let heading = Vec3::new(receiver_vel.x, 0.0, receiver_vel.z)
        .normalize()
        .unwrap_or(Vec3::ZERO);
    let bias = (power.clamp(0.0, 1.0) - 0.5) * 2.0 * LEAD_BIAS_YARDS;
    let aim = base.add(heading.mul_scalar(bias));
    let flat = Vec3::new(aim.x - release.x, 0.0, aim.z - release.z);
    let velocity = launch_velocity(release, aim, speed_for_range(flat.length(), gravity));
    (aim, velocity)
}

/// Launch velocity for a throw of `speed` toward `aim`, at [`LAUNCH_ELEVATION`].
pub fn launch_velocity(release: Vec3, aim: Vec3, speed: f32) -> Vec3 {
    let flat = Vec3::new(aim.x - release.x, 0.0, aim.z - release.z);
    let dir = flat.normalize().unwrap_or(Vec3::UNIT_Z);
    let (sin, cos) = (LAUNCH_ELEVATION.sin(), LAUNCH_ELEVATION.cos());
    Vec3::new(
        dir.x * speed * cos,
        speed * sin,
        dir.z * speed * cos,
    )
}

/// Where a ballistic throw actually comes down, and how long it hangs.
///
/// This is the whole point of a charged pass: the ball goes where the physics
/// sends it, not where the receiver happens to be. Under-throw and it lands
/// short; over-throw and it sails. Solving `y(t) = ground` for the descending
/// root gives both the landing spot and the arrival tick the catch and coverage
/// logic key off.
pub fn predict_landing(release: Vec3, velocity: Vec3, gravity: f32, ground: f32) -> (Vec3, u32) {
    let g = gravity.max(0.01);
    let drop = release.y - ground;
    // Descending root of y0 + vy·t − ½g·t² = ground.
    let disc = (velocity.y * velocity.y + 2.0 * g * drop).max(0.0);
    let t = ((velocity.y + disc.sqrt()) / g).clamp(1.0 / 60.0, MAX_HANG_SECONDS);
    let landing = Vec3::new(
        release.x + velocity.x * t,
        ground,
        release.z + velocity.z * t,
    );
    (landing, (t * 60.0).round().max(1.0) as u32)
}

/// The longest a pass may hang before the predictor gives up, seconds.
const MAX_HANG_SECONDS: f32 = 4.0;

/// The charge needed to land a pass `range` yards away — the inverse of
/// [`predict_landing`], so the autopilot (and the balance harness) can aim as
/// well as a player who has learned the arc.
///
/// Uses the level-ground range `R = v²·sin(2θ)/g`; release and catch heights sit
/// within a foot of each other, so the error is far inside the catch volume.
pub fn power_for_range(range: f32, gravity: f32, tuning: &BehaviorTuning) -> f32 {
    let g = gravity.max(0.01);
    let spread = (2.0 * LAUNCH_ELEVATION).sin().max(0.01);
    let speed = (range.max(0.0) * g / spread).sqrt();
    let min = tuning.pass_speed_min.max(1.0);
    let max = tuning.pass_speed.max(min + 0.01);
    ((speed - min) / (max - min)).clamp(0.0, 1.0)
}

/// Sample a launched pass's arc for the on-field preview, ending at the ground.
pub fn arc_samples(
    release: Vec3,
    velocity: Vec3,
    gravity: f32,
    ground: f32,
    count: usize,
) -> Vec<Vec3> {
    let (_, eta) = predict_landing(release, velocity, gravity, ground);
    let total = eta as f32 / 60.0;
    (0..count.max(2))
        .map(|i| {
            let t = total * i as f32 / (count.max(2) - 1) as f32;
            predict_position(release, velocity, gravity, t)
        })
        .collect()
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
