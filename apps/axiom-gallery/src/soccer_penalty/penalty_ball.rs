//! Pass 5 — the deterministic parametric ball trajectory.
//!
//! After a shot is locked in Pass 4, the ball travels along a fixed parametric
//! 3D arc from the penalty spot to the selected point on the goal plane. This is
//! **not** a physics engine and **not** a projectile framework — it is one
//! closed-form curve sampled at fixed ticks, derived only from the frozen
//! [`PenaltyShotPreview`] and the constants below. No wall-clock time, no
//! randomness, no ambient inputs, and no external sine dependency.
//!
//! ```text
//! t      = elapsed_ticks / total_flight_ticks
//! x      = lerp(start.x, target.x, t) + curve * sin_pi_approx(t)   // curve = 0 in Pass 5
//! z      = lerp(start.z, goal_plane_z, t)                          // monotonic toward the goal
//! base_y = lerp(start.y, target.y, t)
//! y      = base_y + sin_pi_approx(t) * arc_height
//! ```

use axiom_math::Vec3;

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

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
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

/// The fixed parametric curve of one shot.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PenaltyBallTrajectory {
    pub start: Vec3,
    pub target: Vec3,
    pub arc_height: f32,
    pub curve: f32,
    pub total_ticks: u32,
}

impl PenaltyBallTrajectory {
    /// The ball's world position at `elapsed` ticks (clamped to `[0, total]`).
    pub fn position_at(&self, elapsed: u32) -> Vec3 {
        let total = self.total_ticks.max(1) as f32;
        let t = (elapsed as f32 / total).clamp(0.0, 1.0);
        let arc = sin_pi_approx(t);
        Vec3::new(
            lerp(self.start.x, self.target.x, t) + self.curve * arc,
            lerp(self.start.y, self.target.y, t) + arc * self.arc_height,
            lerp(self.start.z, self.target.z, t),
        )
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
    /// Build the descriptor deterministically from a locked shot preview.
    pub fn from_preview(preview: PenaltyShotPreview) -> Self {
        let trajectory = PenaltyBallTrajectory {
            start: penalty_spot(),
            target: world_target(preview.target_x, preview.target_y),
            arc_height: arc_height_for(preview.power),
            curve: CURVE_AMOUNT,
            total_ticks: flight_ticks(preview.power),
        };
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
