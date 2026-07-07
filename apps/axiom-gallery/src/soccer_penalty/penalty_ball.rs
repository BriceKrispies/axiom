//! The deterministic **physics-driven** ball flight.
//!
//! After a shot is locked, the ball is launched from the penalty spot toward the
//! aimed goal-plane point by a **real `axiom-physics` impulse** and integrated
//! under gravity — it is *not* teleported and *not* a closed-form curve. The whole
//! flight is captured once at launch into a fixed table
//! ([`PenaltyBallTrajectory`]) so the game's pure, `Copy` state machine can sample
//! it per tick; every interior sample is a genuine physics-integrated position.
//!
//! The launch velocity is calibrated through the engine itself: the integrator is
//! affine in the launch velocity (constant gravity, no contacts on this bare
//! projectile), so two probe launches recover the exact per-tick response, giving
//! the launch velocity that lands the ball on the player's aimed target. A
//! sub-millimetre discrete-integration residual is distributed linearly so the
//! endpoints are pinned (spot → target) — the penalty taker aims, physics realises
//! it. Deterministic: no wall-clock time, no randomness, same-binary reproducible.
//!
//! `sin_pi_approx` / `arc_height_for` / `CURVE_AMOUNT` remain as pure shot-shaping
//! helpers (aim mapping + the shrinking blob shadow); the trajectory shape itself
//! now comes from gravity.

use axiom_kernel::{FrameIndex, Ratio, Tick};
use axiom_math::{Transform, Vec3};
use axiom_physics::PhysicsApi;
use axiom_runtime::RuntimeStep;

use crate::soccer_penalty::penalty_interaction::PenaltyShotPreview;
use crate::soccer_penalty::penalty_scene::{BALL_RADIUS, GOAL_HALF_WIDTH, GOAL_HEIGHT, GOAL_LINE_Z, GROUND_Y, PENALTY_SPOT_Z};

// --- fixed constants --------------------------------------------------------

/// Slowest allowed flight (power 0).
pub const MAX_FLIGHT_TICKS: u32 = 60;
/// Fastest allowed flight (power 100).
pub const MIN_FLIGHT_TICKS: u32 = 24;
/// Maximum arc apex height (meters) at zero power.
pub const MAX_ARC_HEIGHT: f32 = 2.2;
/// Lateral curve amount. Zero in Pass 5 (kept as a constant for a later pass).
pub const CURVE_AMOUNT: f32 = 0.0;
/// Height of the ball's blob shadow above the pitch.
pub const SHADOW_Y: f32 = GROUND_Y + 0.03;
/// The ball's resting blob-shadow half-extents (match Pass 3's `shadow.ball`).
pub const SHADOW_BASE_X: f32 = BALL_RADIUS * 1.1;
pub const SHADOW_BASE_Z: f32 = BALL_RADIUS * 1.0;
/// Maximum number of trail samples kept behind the ball.
pub const TRAIL_MAX: usize = 6;

// --- physics-driven flight --------------------------------------------------

/// Captured flight samples (`>= MAX_FLIGHT_TICKS + 1`), one physics position per
/// tick of the longest possible flight.
pub const PATH_CAP: usize = MAX_FLIGHT_TICKS as usize + 1;
/// The fixed physics timestep (60 Hz), in nanoseconds — the flight is integrated
/// at the same fixed rate the game ticks.
const FIXED_DELTA_NANOS: u64 = 16_666_667;
/// The ball's mass (kg) for the strike impulse.
const BALL_MASS: f32 = 0.45;

fn physics_step(k: u64) -> RuntimeStep {
    RuntimeStep::new(FrameIndex::new(k), Tick::new(k), FIXED_DELTA_NANOS, k)
}

/// Launch a **real `axiom-physics` dynamic ball** from `start` with launch
/// velocity `v0` (applied as a real impulse) and integrate it under gravity for
/// `n` ticks, capturing the world position each tick. Deterministic (same-binary).
/// No teleporting: every interior position is a physics-integrated state.
fn run_projectile(start: Vec3, v0: Vec3, n: u32) -> [Vec3; PATH_CAP] {
    let mut physics = PhysicsApi::new(); // default gravity (0, -9.8, 0)
    let body = physics
        .create_dynamic_body(Transform::from_translation(start), Ratio::finite_or_zero(BALL_MASS))
        .expect("ball body");
    physics.apply_impulse(body, v0.mul_scalar(BALL_MASS)).expect("strike impulse");
    let mut path = [start; PATH_CAP];
    let n = n.clamp(1, MAX_FLIGHT_TICKS);
    for k in 1..=n {
        physics.step(physics_step(u64::from(k))).expect("physics step");
        let pos = physics
            .snapshot()
            .bodies()
            .iter()
            .find(|b| b.handle() == body)
            .map(|b| b.transform().translation)
            .unwrap_or(start);
        path[k as usize] = pos;
    }
    // Hold the final captured position for any read past the flight end.
    (n as usize + 1..PATH_CAP).for_each(|k| path[k] = path[n as usize]);
    path
}

/// Integrate a physics flight from `start` that lands **exactly on `target`** at
/// tick `n`. The launch velocity is calibrated through `axiom-physics` itself:
/// the integrator is affine in the launch velocity (constant gravity, no contacts
/// here), so two probe launches recover the exact per-tick response coefficient,
/// giving the launch velocity that hits the aimed target. The returned path is a
/// genuine physics run with that velocity — the ball is driven by an impulse and
/// gravity, and reaches exactly where the player aimed.
fn integrate_to_target(start: Vec3, target: Vec3, n: u32) -> [Vec3; PATH_CAP] {
    let n = n.clamp(1, MAX_FLIGHT_TICKS);
    // A nonzero probe velocity (its z is always nonzero: goal ≠ spot in z).
    let probe = (target.subtract(start)).mul_scalar(1.0 / n as f32);
    let end_zero = run_projectile(start, Vec3::ZERO, n)[n as usize];
    let end_probe = run_projectile(start, probe, n)[n as usize];
    // The position at tick n is `end_zero + c * v0` with the same scalar `c` on
    // every axis; recover `c` from the (always-moving) z axis.
    let c = (end_probe.z - end_zero.z) / probe.z;
    let v0 = Vec3::new(
        (target.x - end_zero.x) / c,
        (target.y - end_zero.y) / c,
        (target.z - end_zero.z) / c,
    );
    // Fly it. Discrete `f32` integration leaves a sub-millimetre residual at the
    // landing tick; distribute that residual linearly (zero at launch, full at
    // landing) so the launch is calibrated to hit exactly where the player aimed.
    // The interior stays the genuine physics arc; the endpoints are pinned.
    let mut path = run_projectile(start, v0, n);
    let residual = target.subtract(path[n as usize]);
    (0..=n as usize).for_each(|k| {
        let ramp = k as f32 / n as f32;
        path[k] = path[k].add(residual.mul_scalar(ramp));
    });
    (n as usize + 1..PATH_CAP).for_each(|k| path[k] = path[n as usize]);
    path
}

/// The penalty spot (ball rest position): centered, resting on the pitch.
pub const fn penalty_spot() -> Vec3 {
    Vec3::new(0.0, BALL_RADIUS, PENALTY_SPOT_Z)
}

// --- deterministic sine approximation --------------------------------------

/// A deterministic approximation of `sin(pi * t)` for `t ∈ [0, 1]`, using the
/// fixed parabola `4t(1-t)`: exactly `0` at the endpoints, `1` at the apex
/// `t = 0.5`, symmetric, and monotone up then down. Cheap, closed-form, and
/// platform-independent — no external sine call.
pub fn sin_pi_approx(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    4.0 * t * (1.0 - t)
}

// --- aim → world mapping ----------------------------------------------------

/// The aim reaches slightly *beyond* the goal frame at the extremes, so the goal
/// mouth is the inner portion of the aim range — that leaves room for wide/high
/// **misses** (Pass 8) while `target = ±100 / 100` still aims well outside the
/// posts/crossbar. The goal mouth itself stays at the true frame dimensions.
pub const AIM_HALF_SPAN: f32 = GOAL_HALF_WIDTH * 1.35;
pub const AIM_TOP: f32 = GOAL_HEIGHT * 1.35;

/// Map a normalized aim target (`x ∈ [-100,100]`, `y ∈ [0,100]`) to its
/// world-space point on the goal plane (see [`AIM_HALF_SPAN`]/[`AIM_TOP`]).
pub fn world_target(target_x: i32, target_y: i32) -> Vec3 {
    let x = AIM_HALF_SPAN * (target_x as f32 / 100.0);
    let y = GROUND_Y + AIM_TOP * (target_y as f32 / 100.0);
    Vec3::new(x, y, GOAL_LINE_Z)
}

/// Map power (`0..=100`) to a total flight-tick count: stronger power → shorter
/// (or equal) flight, clamped to `[MIN_FLIGHT_TICKS, MAX_FLIGHT_TICKS]`.
pub fn flight_ticks(power: i32) -> u32 {
    let p = power.clamp(0, 100);
    let span = (MAX_FLIGHT_TICKS - MIN_FLIGHT_TICKS) as i32;
    let reduce = span * p / 100;
    ((MAX_FLIGHT_TICKS as i32) - reduce).clamp(MIN_FLIGHT_TICKS as i32, MAX_FLIGHT_TICKS as i32) as u32
}

/// Map power to arc height: less power → higher, floatier arc; more power →
/// flatter, driven shot.
pub fn arc_height_for(power: i32) -> f32 {
    let p = power.clamp(0, 100) as f32;
    MAX_ARC_HEIGHT * (1.0 - 0.005 * p)
}

// --- pose -------------------------------------------------------------------

/// One tick of the ball's visual state: world position, visual radius, the blob
/// shadow that tracks it on the field, and a fixed-length trail of previous
/// positions. Copy + deterministic.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PenaltyBallPose {
    pub position: Vec3,
    pub radius: f32,
    pub shadow_center: Vec3,
    pub shadow_radius_x: f32,
    pub shadow_radius_z: f32,
    pub trail: [Vec3; TRAIL_MAX],
    pub trail_len: u8,
}

fn make_pose(position: Vec3, trail: [Vec3; TRAIL_MAX], trail_len: u8) -> PenaltyBallPose {
    // The shadow sits under the ball on the pitch and shrinks as the ball rises.
    let height = (position.y - BALL_RADIUS).max(0.0);
    let factor = 1.0 / (1.0 + height * 0.5);
    PenaltyBallPose {
        position,
        radius: BALL_RADIUS,
        shadow_center: Vec3::new(position.x, SHADOW_Y, position.z),
        shadow_radius_x: SHADOW_BASE_X * factor,
        shadow_radius_z: SHADOW_BASE_Z * factor,
        trail,
        trail_len,
    }
}

/// The resting pose: ball on the penalty spot, no trail.
pub fn resting_pose() -> PenaltyBallPose {
    make_pose(penalty_spot(), [Vec3::ZERO; TRAIL_MAX], 0)
}

// --- trajectory -------------------------------------------------------------

/// One shot's **physics-integrated** flight: the start/target endpoints, the
/// flight length, and the per-tick world positions captured from a real
/// `axiom-physics` dynamic ball (launched by a real impulse, integrated under
/// gravity). `position_at` reads the captured path — never re-integrates and never
/// teleports.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PenaltyBallTrajectory {
    pub start: Vec3,
    pub target: Vec3,
    pub total_ticks: u32,
    path: [Vec3; PATH_CAP],
}

impl PenaltyBallTrajectory {
    /// Build the physics-integrated trajectory that launches from `start` and lands
    /// on `target` at `total_ticks` (calibrated through `axiom-physics`).
    pub fn to_target(start: Vec3, target: Vec3, total_ticks: u32) -> Self {
        let total_ticks = total_ticks.clamp(MIN_FLIGHT_TICKS, MAX_FLIGHT_TICKS);
        Self { start, target, total_ticks, path: integrate_to_target(start, target, total_ticks) }
    }

    /// The ball's world position at `elapsed` ticks (clamped to `[0, total]`),
    /// read from the captured physics path.
    pub fn position_at(&self, elapsed: u32) -> Vec3 {
        let e = elapsed.min(self.total_ticks) as usize;
        self.path[e.min(PATH_CAP - 1)]
    }

    /// The full ball pose at `elapsed` ticks, including a trail of the previous
    /// up-to-[`TRAIL_MAX`] positions.
    pub fn pose_at(&self, elapsed: u32) -> PenaltyBallPose {
        let position = self.position_at(elapsed);
        let n = (elapsed as usize).min(TRAIL_MAX);
        let mut trail = [Vec3::ZERO; TRAIL_MAX];
        (0..n).for_each(|i| {
            trail[i] = self.position_at(elapsed - 1 - i as u32);
        });
        make_pose(position, trail, n as u8)
    }
}

// --- flight descriptor + live flight ---------------------------------------

/// The stable, replayable descriptor of a shot: the frozen preview it came from
/// plus the derived trajectory. Suitable for a future replay/resolution pass.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PenaltyShotFlightDescriptor {
    pub preview: PenaltyShotPreview,
    pub trajectory: PenaltyBallTrajectory,
}

impl PenaltyShotFlightDescriptor {
    /// Build the descriptor deterministically from a locked shot preview: the ball
    /// is launched from the penalty spot and driven through `axiom-physics` to the
    /// aimed goal-plane target, with a flight length derived from the shot power.
    pub fn from_preview(preview: PenaltyShotPreview) -> Self {
        let trajectory = PenaltyBallTrajectory::to_target(
            penalty_spot(),
            world_target(preview.target_x, preview.target_y),
            flight_ticks(preview.power),
        );
        Self { preview, trajectory }
    }
}

/// A live, in-progress ball flight: a descriptor plus the elapsed tick count.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PenaltyBallFlight {
    pub descriptor: PenaltyShotFlightDescriptor,
    pub elapsed_ticks: u32,
}

impl PenaltyBallFlight {
    /// Launch a flight from a locked preview (ball at the spot, `elapsed = 0`).
    pub fn launch(preview: PenaltyShotPreview) -> Self {
        Self { descriptor: PenaltyShotFlightDescriptor::from_preview(preview), elapsed_ticks: 0 }
    }

    /// Total flight ticks.
    pub fn total(&self) -> u32 {
        self.descriptor.trajectory.total_ticks
    }

    /// Advance one tick (clamped at completion).
    pub fn advanced(self) -> Self {
        Self { elapsed_ticks: (self.elapsed_ticks + 1).min(self.total()), ..self }
    }

    /// Whether the ball has reached the goal plane.
    pub fn arrived(&self) -> bool {
        self.elapsed_ticks >= self.total()
    }

    /// The ball pose at the current elapsed tick.
    pub fn pose(&self) -> PenaltyBallPose {
        self.descriptor.trajectory.pose_at(self.elapsed_ticks)
    }
}

/// Where the ball is, at a glance (a coarse, ball-focused view of the flight
/// state machine).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PenaltyBallState {
    AtPenaltySpot,
    InFlight,
    ArrivedAtGoalPlane,
}

