//! The "virtual muscle": deterministic, fixed-step damped springs that let each
//! animated body region *chase* its target pose instead of snapping onto it.
//!
//! Every spring integrates with semi-implicit Euler at the simulation's fixed
//! [`crate::config::DT`] — never at a wall-clock or frame delta — so the
//! response is exactly reproducible from the tick stream. That fixed step is
//! also why this is a real spring and not a frame-rate-dependent
//! `lerp(current, target, k)`.
//!
//! Everything here is bounded by construction: stiffness is clamped below the
//! fixed-step stability limit, velocity and per-tick travel are clamped, and
//! every non-finite input is replaced. A spring can therefore never introduce a
//! NaN or infinity into a joint transform, which is the invariant
//! `tests/biomech.rs` pins.

use crate::config::DT;

/// Stiffness ceiling (1/s²). Semi-implicit Euler at a 60 Hz step is stable
/// while `k·dt² < 4` (k < 14_400); this sits well inside that bound while
/// still allowing a settling time of ~3 ticks. The pelvis needs that: a full
/// sprint's ground contact is only ~3 ticks long, and a softer ceiling made
/// the weight-acceptance sink physically untrackable at speed.
const MAX_STIFFNESS: f32 = 3_000.0;

/// Velocity ceiling (units/s) — a hard backstop so a pathological target can
/// never launch a region across the field.
const MAX_VELOCITY: f32 = 80.0;

/// Replace a non-finite value with a safe fallback.
fn finite(value: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value
    } else {
        fallback
    }
}

/// One scalar damped spring: a value chasing a target with momentum.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spring {
    /// The current (rendered) value.
    pub value: f32,
    /// The current rate of change, units/s.
    pub velocity: f32,
}

impl Spring {
    /// A spring at rest on `value`.
    pub const fn at(value: f32) -> Self {
        Spring {
            value,
            velocity: 0.0,
        }
    }

    /// Snap to `value` and kill all momentum — used on a teleport, a play
    /// reset, or when an override pose takes the body, so no stale motion
    /// survives the discontinuity.
    pub fn reset(&mut self, value: f32) {
        self.value = finite(value, 0.0);
        self.velocity = 0.0;
    }

    /// Advance one fixed tick toward `target` and return the new value.
    ///
    /// `stiffness` is the spring constant (1/s²), `damping` the damping ratio
    /// (1.0 = critically damped, below = a little overshoot, above =
    /// sluggish), and `max_step` the largest correction allowed this tick.
    pub fn step(&mut self, target: f32, stiffness: f32, damping: f32, max_step: f32) -> f32 {
        let target = finite(target, self.value);
        let k = finite(stiffness, 0.0).clamp(0.0, MAX_STIFFNESS);
        // Critical damping coefficient for this stiffness, scaled by the ratio.
        let c = 2.0 * finite(damping, 1.0).clamp(0.0, 4.0) * k.sqrt();
        let accel = -k * (self.value - target) - c * self.velocity;
        let velocity = finite(self.velocity + accel * DT, 0.0).clamp(-MAX_VELOCITY, MAX_VELOCITY);
        let step = finite(velocity * DT, 0.0).clamp(-max_step.abs(), max_step.abs());
        self.velocity = velocity;
        self.value = finite(self.value + step, target);
        self.value
    }
}

impl Default for Spring {
    fn default() -> Self {
        Spring::at(0.0)
    }
}

/// The per-player bank of body-region springs. One bank persists per player
/// slot for the life of the animator, exactly like the gait state.
///
/// The regions are grouped by how tightly they should track: the pelvis and the
/// stance-bearing root translation are stiff (weight must look controlled), the
/// spine sits in the middle, and the arms are loosest so they trail the torso.
#[derive(Debug, Clone, Copy, Default)]
pub struct BodySprings {
    /// Visual body root: vertical (pelvis rise/fall) and lateral weight shift.
    pub root_lift: Spring,
    pub root_lateral: Spring,
    /// Visual body root attitude.
    pub root_pitch: Spring,
    pub root_roll: Spring,
    /// Pelvis joint.
    pub pelvis_yaw: Spring,
    pub pelvis_roll: Spring,
    pub pelvis_pitch: Spring,
    /// Lower spine (torso joint).
    pub spine_yaw: Spring,
    pub spine_roll: Spring,
    pub spine_pitch: Spring,
    /// Ribcage / shoulder girdle (pad joint) — shoulders and head hang here.
    pub ribcage_yaw: Spring,
    pub ribcage_pitch: Spring,
    /// Arm swing driver (shared by both arms, mirrored).
    pub arm_swing: Spring,
    /// Head.
    pub head_pitch: Spring,
    pub head_yaw: Spring,
}

impl BodySprings {
    /// A fresh bank, every region at rest on zero.
    pub fn new() -> Self {
        BodySprings::default()
    }

    /// Drop all momentum and snap every region to neutral. Called whenever the
    /// gait re-anchors (teleport, play reset, override pose) so the carriage
    /// never springs across a discontinuity.
    pub fn reset(&mut self) {
        *self = BodySprings::default();
    }

    /// True when every spring in the bank holds a finite value and velocity —
    /// the invariant the tests assert and the pose pass relies on.
    pub fn is_finite(&self) -> bool {
        self.all().iter().all(|s| {
            let s: &Spring = s;
            s.value.is_finite() && s.velocity.is_finite()
        })
    }

    /// Every spring in the bank, for bulk assertions and diagnostics.
    fn all(&self) -> [Spring; 15] {
        [
            self.root_lift,
            self.root_lateral,
            self.root_pitch,
            self.root_roll,
            self.pelvis_yaw,
            self.pelvis_roll,
            self.pelvis_pitch,
            self.spine_yaw,
            self.spine_roll,
            self.spine_pitch,
            self.ribcage_yaw,
            self.ribcage_pitch,
            self.arm_swing,
            self.head_pitch,
            self.head_yaw,
        ]
    }
}
