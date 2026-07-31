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

/// The horizontal distance from `release` to `aim`, yards.
fn flat_range(release: Vec3, aim: Vec3) -> f32 {
    Vec3::new(aim.x - release.x, 0.0, aim.z - release.z).length()
}

/// The speed a pass covering `range` yards leaves the hand at, yd/s.
///
/// As hard as the passer can throw, with one cap: a very short pass is slowed
/// just enough to stay airborne for `min_flight_ticks`, because the catch
/// pipeline needs a few ticks to contest and resolve a ball. That cap is why a
/// five-yard slant is not a bullet that arrives the tick it is released — it is
/// a floor on the *catch*, not a punishment for throwing short.
fn throw_speed(range: f32, tuning: &BehaviorTuning) -> f32 {
    let min_flight = (tuning.min_flight_ticks.max(1) as f32) / 60.0;
    let ceiling = tuning.pass_speed.max(1.0);
    (range.max(0.0) / min_flight).clamp(tuning.pass_speed_min.clamp(1.0, ceiling), ceiling)
}

/// The launch elevation that carries a pass `range` yards to a point sitting
/// `rise` yards above the release, radians.
///
/// **`rise` is normally negative**, and it is not a rounding error. The ball
/// leaves the hand at throwing height (1.95 yd) and is caught at chest height
/// (1.45 yd), so it is falling half a yard over the throw. The level-ground
/// range equation `R = v²·sin(2θ)/g` assumes those heights are equal; using it
/// here threw every pass long — about three yards long on a twenty-yard throw,
/// which is a completion turned into an overthrow.
///
/// Solving the trajectory `y(R) = rise` for `u = tan θ` gives a quadratic:
///
/// ```text
///   k·u² − R·u + (rise + k) = 0,      k = g·R²/(2v²)
/// ```
///
/// taking the **smaller** root — the rope rather than the moon-ball, since the
/// same distance can be covered by either and a quarterback throws the rope.
/// It is evaluated as `2(rise + k) / (R + √disc)` rather than the textbook
/// `(R − √disc)/2k`: the two are equal, but on a short throw `k` is tiny and
/// the textbook form is a small difference of two near-equal numbers divided by
/// something near zero. A negative discriminant means the target is out of
/// range at this speed, and the vertex `R/2k` is then the furthest it can throw.
fn launch_angle(range: f32, rise: f32, speed: f32, gravity: f32) -> f32 {
    let r = range.max(0.01);
    let k = gravity.max(0.01) * r * r / (2.0 * speed.max(1.0).powi(2));
    let disc = r * r - 4.0 * k * (rise + k);
    let u = match disc >= 0.0 {
        true => 2.0 * (rise + k) / (r + disc.sqrt()),
        false => r / (2.0 * k),
    };
    u.atan()
}

/// The aim point and launch velocity for a throw to a receiver.
///
/// Always a perfect ball: the intercept solve puts it where the receiver *will
/// be*, and nothing biases it off that point. The passer's arm is not one of
/// the things this game asks the player to get right — the read is — so a throw
/// only fails because the receiver was covered, never because the pass was.
///
/// **The lead and the launch must agree on how fast the ball flies.** Solving
/// the intercept against one speed and then throwing at another is what puts a
/// nominally perfect pass behind a sprinting receiver: he keeps running for the
/// difference between the two flight times. Both the speed and the elevation
/// are functions of the range, and the range is a function of the lead, so this
/// closes the loop by refining it — the first pass leads at full speed, and the
/// correction accounts for the fraction of that speed the arc spends climbing.
/// The second refinement moves the aim by inches; it is there so a deep ball to
/// a receiver at full stride lands in his hands rather than a stride behind.
pub fn aim_and_velocity(
    release: Vec3,
    receiver_pos: Vec3,
    receiver_vel: Vec3,
    gravity: f32,
    tuning: &BehaviorTuning,
) -> (Vec3, Vec3) {
    // Speed and elevation both follow from the range, and the ball is caught
    // BELOW the height it was thrown from — see `launch_angle`.
    let solve = |aim: Vec3| {
        let range = flat_range(release, aim);
        let speed = throw_speed(range, tuning);
        let rise = super::catch_point(aim).y - release.y;
        (speed, launch_angle(range, rise, speed, gravity))
    };
    // The ball's HORIZONTAL speed is what an intercept is solved against — the
    // vertical component carries the arc, not the receiver.
    let aim = (0..2).fold(
        lead_point(release, receiver_pos, receiver_vel, tuning.pass_speed),
        |aim, _| {
            let (speed, elevation) = solve(aim);
            lead_point(release, receiver_pos, receiver_vel, speed * elevation.cos())
        },
    );
    let (speed, elevation) = solve(aim);
    (aim, launch_velocity(release, aim, speed, elevation))
}

/// Launch velocity for a throw of `speed` toward `aim` at `elevation`.
pub fn launch_velocity(release: Vec3, aim: Vec3, speed: f32, elevation: f32) -> Vec3 {
    let flat = Vec3::new(aim.x - release.x, 0.0, aim.z - release.z);
    let dir = flat.normalize().unwrap_or(Vec3::UNIT_Z);
    let (sin, cos) = (elevation.sin(), elevation.cos());
    Vec3::new(dir.x * speed * cos, speed * sin, dir.z * speed * cos)
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
