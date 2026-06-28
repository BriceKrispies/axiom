//! The seven ease curves and their branchless fn-pointer dispatch.
//!
//! Each curve maps normalized time `t ∈ [0, 1]` to eased progress with **exact
//! endpoints** (`f(0) == 0`, `f(1) == 1`); `back_out` overshoots above one
//! between them by design. Presentation math, so `powf` is fine (no §17.6
//! cross-machine bit-exactness is required of display values).

use crate::ids::Ease;

fn linear(t: f32) -> f32 {
    t
}

fn quad_in(t: f32) -> f32 {
    t * t
}

fn quad_out(t: f32) -> f32 {
    t * (2.0 - t)
}

fn quad_in_out(t: f32) -> f32 {
    // Accelerate over the first half, decelerate over the second — selected by a
    // branchless table index, not an `if`.
    let first = 2.0 * t * t;
    let second = 1.0 - 2.0 * (1.0 - t) * (1.0 - t);
    [first, second][(t >= 0.5) as usize]
}

fn cubic_out(t: f32) -> f32 {
    let inv = 1.0 - t;
    1.0 - inv * inv * inv
}

fn expo_out(t: f32) -> f32 {
    // Normalized exponential: divide by the value at `t = 1` so the endpoints are
    // exactly 0 and 1 without an `if t == 1` special case.
    let denom = 1.0 - 2f32.powf(-10.0);
    (1.0 - 2f32.powf(-10.0 * t)) / denom
}

fn back_out(t: f32) -> f32 {
    const C1: f32 = 1.70158;
    const C3: f32 = C1 + 1.0;
    let p = t - 1.0;
    1.0 + C3 * p * p * p + C1 * p * p
}

/// The dispatch table — its order mirrors the [`Ease`] discriminant order so
/// `curve as usize` indexes the matching curve.
const CURVES: [fn(f32) -> f32; 7] = [
    linear,
    quad_in,
    quad_out,
    quad_in_out,
    cubic_out,
    expo_out,
    back_out,
];

/// Evaluate `curve` at normalized time `t`, branchlessly: a table index, no
/// `match` on the discriminant.
pub(crate) fn ease_unit(curve: Ease, t: f32) -> f32 {
    CURVES[curve as usize](t)
}
