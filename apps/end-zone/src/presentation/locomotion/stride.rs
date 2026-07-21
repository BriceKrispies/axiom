//! Where the body is inside its current step, and the shaping primitives every
//! gait-driven component is built from.
//!
//! Split out of [`super::carriage`] so the *timing* of a stride (stance vs
//! flight, progress through each) stays separate from the *carriage* decisions
//! layered on top of it. Pure geometry and pure curves — no state, no
//! simulation access.

use super::foot;
use super::gait::{GaitState, PlantedFoot};

pub const PI: f32 = core::f32::consts::PI;

/// Smooth Hermite ease over `0..1`, clamped outside.
pub fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A smooth 0 → 1 → 0 hump over `0..1` peaking at `center`. Zero-sloped at both
/// ends and at the peak, so chaining it across strides stays continuous.
pub fn bell(t: f32, center: f32) -> f32 {
    let c = center.clamp(0.05, 0.95);
    let t = t.clamp(0.0, 1.0);
    let rising = t < c;
    let x = if rising { t / c } else { (1.0 - t) / (1.0 - c) };
    smoothstep(x)
}


/// Where the body is in its current step: the half-cycle split that every
/// gait-driven component is shaped against.
#[derive(Debug, Clone, Copy)]
pub struct Stride {
    /// Which foot is down this half-cycle.
    pub stance: PlantedFoot,
    /// +1 when the RIGHT foot bears weight, -1 when the left does. Matches the
    /// figure's +X = right axis, so it doubles as the lateral-shift direction.
    pub stance_sign: f32,
    /// Progress through the half-cycle, 0..1 (strike → next strike).
    pub step_progress: f32,
    /// Progress through ground contact, 0 = strike, 1 = toe-off.
    pub stance_progress: f32,
    /// Progress through flight/transition after toe-off, 0..1.
    pub flight_progress: f32,
    pub in_flight: bool,
}

/// Split the gait phase into the current step's stance and flight portions.
pub fn stride_of(phase: f32, planted_fraction: f32) -> Stride {
    let phase = phase.rem_euclid(1.0);
    // The left foot bears weight through the first half of the cycle, the right
    // through the second — the two feet are exactly half a cycle apart.
    let stance_is_left = phase < 0.5;
    let step_progress = (phase * 2.0).rem_euclid(1.0);
    // `planted_fraction` is a fraction of the FULL cycle; a half-cycle is 0.5,
    // so ground contact occupies this much of the step (1.0 = no flight phase,
    // which is the correct low-speed / double-support case).
    let contact = (planted_fraction / 0.5).clamp(0.05, 1.0);
    let stance_progress = (step_progress / contact).min(1.0);
    let flight_span = (1.0 - contact).max(1.0e-4);
    let flight_progress = ((step_progress - contact) / flight_span).clamp(0.0, 1.0);
    Stride {
        stance: if stance_is_left {
            PlantedFoot::Left
        } else {
            PlantedFoot::Right
        },
        stance_sign: if stance_is_left { -1.0 } else { 1.0 },
        step_progress,
        stance_progress,
        flight_progress,
        in_flight: step_progress > contact,
    }
}

/// Longitudinal separation of the swing foot ahead of the stance foot, in yards
/// along the facing. Positive at late swing (the swing leg has reached ahead),
/// negative just after its toe-off — the real geometric driver of pelvis yaw.
pub fn foot_separation(gait: &GaitState, stance: PlantedFoot, facing: f32) -> f32 {
    let (_, forward) = foot::dirs(facing);
    let (stance_foot, swing_foot) = match stance {
        PlantedFoot::Left => (gait.left.target, gait.right.target),
        PlantedFoot::Right => (gait.right.target, gait.left.target),
    };
    swing_foot.subtract(stance_foot).dot(forward)
}

