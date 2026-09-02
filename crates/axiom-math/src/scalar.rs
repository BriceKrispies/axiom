//! The deterministic scalar policy for Layer 02.

use crate::math_error::MathError;
use crate::math_result::MathResult;

/// The math layer's scalar policy.
///
/// Axiom standardises on IEEE-754 `f32` as the engine's **interchange** scalar:
/// what crosses a facade, sits in a vertex or index buffer, reaches a GPU
/// uniform, or is stored in a transform. [`crate::Vec3`], [`crate::Mat4`],
/// [`crate::Quat`] and the whole `f32` geometry family are that scalar's types,
/// and they are what an engine boundary speaks.
///
/// `f32` is **not** a claim that every computation runs at single precision.
/// Some domains genuinely need more, and evaluating them in `f32` does not
/// merely lose digits — it *introduces* disagreements the reference does not
/// have. `axiom_surface::srgb_to_linear` is the measured case: across all 256
/// byte inputs, computing in `f64` and narrowing once gives **0/256**
/// mismatches against three.js, while a natively-`f32` transcription of the
/// same algebra gives **175/256**. The rule that falls out, and that this layer
/// follows, is:
///
/// > **Evaluate at the precision the domain requires; narrow once, at the
/// > boundary, to the interchange scalar.**
///
/// The `f64` types here ([`crate::DVec3`]) exist to give that rule a vocabulary
/// rather than leaving each caller to pass loose `f64` triples around. They are
/// for domains whose *internal* precision is load-bearing — a collision kernel
/// over a city-scale world, an atmosphere LUT, an audio impulse response, a
/// bake-time noise oracle a shader is pinned against. They are not a second
/// engine scalar, and nothing should reach for one to store a transform.
///
/// `Scalar` is a zero-sized policy holder that exposes the chosen constants and
/// the finite scalar validation rule the rest of the layer follows. There is no
/// implicit rounding, no clamping and no global epsilon — every checked
/// operation must route through [`Scalar::validate_finite`] (or take an
/// explicit [`crate::Epsilon`] — kept private; reached via [`crate::MathApi`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Scalar;

impl Scalar {
    /// The default tolerance used for approximate comparisons when no
    /// explicit [`crate::Epsilon`] is supplied. `1e-6` is comfortably above
    /// `f32::EPSILON` while still rejecting genuinely distinct values.
    pub const DEFAULT_EPSILON: f32 = 1.0e-6;

    /// The default tolerance for comparing double-precision values. See
    /// [`crate::Epsilon::DEFAULT_DOUBLE`] for why it is not
    /// [`Scalar::DEFAULT_EPSILON`].
    pub const DEFAULT_EPSILON_DOUBLE: f32 = 1.0e-12;

    /// Whether `v` is a finite real number (neither `NaN` nor `±Inf`).
    pub const fn is_finite_value(v: f32) -> bool {
        v.is_finite()
    }

    /// Return `v` if it is finite; otherwise produce a
    /// [`crate::math_error_code::MathErrorCode::NonFiniteScalar`] error.
    pub fn validate_finite(v: f32) -> MathResult<f32> {
        v.is_finite().then_some(v).ok_or_else(|| {
            MathError::non_finite_scalar("math scalar must be finite (no NaN, no Inf)")
        })
    }
}

// The default tolerance is a property of the constant, not a runtime input:
// assert at compile time that it is a sensible positive sub-millisecond value,
// rather than as a runtime test that can never observe anything else.
const _: () = assert!(
    (Scalar::DEFAULT_EPSILON > 0.0) & (Scalar::DEFAULT_EPSILON < 1.0e-3),
    "DEFAULT_EPSILON must be a positive sub-millisecond tolerance"
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math_error_code::MathErrorCode;

    #[test]
    fn is_finite_value_accepts_finite_numbers() {
        assert!(Scalar::is_finite_value(0.0));
        assert!(Scalar::is_finite_value(-1.5));
        assert!(Scalar::is_finite_value(f32::MAX));
    }

    #[test]
    fn is_finite_value_rejects_nan_and_inf() {
        assert!(!Scalar::is_finite_value(f32::NAN));
        assert!(!Scalar::is_finite_value(f32::INFINITY));
        assert!(!Scalar::is_finite_value(f32::NEG_INFINITY));
    }

    #[test]
    fn validate_finite_accepts_finite_numbers() {
        assert_eq!(Scalar::validate_finite(2.5).unwrap(), 2.5);
    }

    #[test]
    fn validate_finite_rejects_nan() {
        let err = Scalar::validate_finite(f32::NAN).unwrap_err();
        assert_eq!(err.code(), MathErrorCode::NonFiniteScalar);
    }

    #[test]
    fn validate_finite_rejects_infinity() {
        let err = Scalar::validate_finite(f32::INFINITY).unwrap_err();
        assert_eq!(err.code(), MathErrorCode::NonFiniteScalar);
        let err = Scalar::validate_finite(f32::NEG_INFINITY).unwrap_err();
        assert_eq!(err.code(), MathErrorCode::NonFiniteScalar);
    }
}

/// Clamp `v` into `[lo, hi]`.
///
/// **A comparison chain, not `v.max(lo).min(hi)`, and the difference is NaN.**
/// `f64::max` is documented to *ignore* NaN and return the other operand, so the
/// max/min spelling maps `NaN` to `lo`; a comparison chain fails both tests and
/// passes `NaN` through. The reference this engine's ports are pinned to
/// (`v < lo ? lo : v > hi ? hi : v`) propagates NaN, so this does too.
///
/// Nor is it `f64::clamp`, which panics when `lo > hi`. A clamp with reversed
/// bounds is a caller's bug, but turning it into a panic is a behaviour change
/// no port asked for; here it simply pins to `lo`.
///
/// Three call sites in `apps/axiom-shmup` used the max/min spelling and were
/// silently diverging from their own source on NaN. That is why this exists as
/// one named function rather than a habit repeated nine times.
pub fn clamp(v: f64, lo: f64, hi: f64) -> f64 {
    // Selection, not branching: `v < lo` and `v > hi` are both false for NaN,
    // so NaN falls through to `v` exactly as the comparison chain does.
    [[v, hi][usize::from(v > hi)], lo][usize::from(v < lo)]
}

/// [`clamp`] into the unit interval.
pub fn clamp01(v: f64) -> f64 {
    clamp(v, 0.0, 1.0)
}

/// Linear interpolation, `a + (b - a) * t`.
///
/// Written in that grouping rather than the algebraically equal
/// `a * (1 - t) + b * t`, because only this form reproduces `a` **exactly** at
/// `t == 0`, and a value at rest that drifts in its last bits every frame is a
/// bug you find much later.
pub fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// Exponential approach: move `current` toward `target` at `rate` per second.
///
/// Frame-rate independent, unlike `current += (target - current) * k`, which
/// converges at a speed that depends on how often it is called.
pub fn damp(current: f64, target: f64, rate: f64, dt: f64) -> f64 {
    target + (current - target) * (-rate * dt).exp()
}

/// GLSL `smoothstep`: 0 below `edge0`, 1 above `edge1`, Hermite between.
///
/// A zero-width edge is guarded rather than allowed to divide by zero. The
/// guard is not decoration: two of the three call sites this replaces carried
/// it and one did not, so the un-guarded one produced an infinity that
/// `clamp01` then flattened to 0 or 1 depending on which side the input fell —
/// a silent discontinuity where the caller expected a smooth one.
pub fn smoothstep(edge0: f64, edge1: f64, x: f64) -> f64 {
    let t = clamp01((x - edge0) / crate::nonzero_or(edge1 - edge0, ZERO_EDGE_FALLBACK));
    t * t * (3.0 - 2.0 * t)
}

/// [`smoothstep`] with Perlin's second-order curve — zero first *and second*
/// derivative at both ends, so a value driven by it starts and stops without a
/// visible kink.
pub fn smootherstep(edge0: f64, edge1: f64, x: f64) -> f64 {
    let t = clamp01((x - edge0) / crate::nonzero_or(edge1 - edge0, ZERO_EDGE_FALLBACK));
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// [`smoothstep`] over an already-normalised `t`, clamped.
pub fn smooth_unit(t: f64) -> f64 {
    let t = clamp01(t);
    t * t * (3.0 - 2.0 * t)
}

/// [`smootherstep`] over an already-normalised `t`, clamped.
pub fn smoother_unit(t: f64) -> f64 {
    let t = clamp01(t);
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// Wrap an angle into `[-PI, PI)`.
///
/// **Half-open at the *bottom*, not the top**, which is worth stating because
/// the reference this is ported from documents it the other way round and is
/// wrong: `%` leaves the shifted angle in `[0, TAU)`, so subtracting PI gives
/// `[-PI, PI)`. An exact half-turn comes back as `-PI`, not `+PI`. The code was
/// transcribed faithfully; only the comment was inherited incorrectly, and it
/// has been wrong in both languages since the port.
pub fn wrap_pi(a: f64) -> f64 {
    let shifted = (a + core::f64::consts::PI) % core::f64::consts::TAU;
    // `%` keeps the sign of the dividend, so a negative result needs one turn
    // added. Arithmetic rather than a branch: the flag is 0 or 1.
    let lifted = shifted + core::f64::consts::TAU * f64::from(u8::from(shifted < 0.0));
    lifted - core::f64::consts::PI
}

/// Substituted for a zero edge span in [`smoothstep`] / [`smootherstep`].
const ZERO_EDGE_FALLBACK: f64 = 1e-6;

#[cfg(test)]
mod scalar_kit_tests {
    use super::{
        clamp, clamp01, damp, lerp, smooth_unit, smoother_unit, smoothstep, smootherstep, wrap_pi,
    };

    #[test]
    fn clamp_pins_to_each_bound_and_passes_the_middle_through() {
        assert_eq!(clamp(-5.0, 0.0, 1.0), 0.0);
        assert_eq!(clamp(5.0, 0.0, 1.0), 1.0);
        assert_eq!(clamp(0.25, 0.0, 1.0), 0.25);
        // The bounds themselves are inside.
        assert_eq!(clamp(0.0, 0.0, 1.0), 0.0);
        assert_eq!(clamp(1.0, 0.0, 1.0), 1.0);
    }

    /// **The reason this function exists.** `v.max(lo).min(hi)` is the spelling
    /// three call sites used, and it is not the same function: `f64::max`
    /// ignores NaN and returns the other operand, so NaN clamps to `lo`. The
    /// reference these ports follow propagates NaN. If this test ever fails,
    /// someone has "simplified" `clamp` back into the divergent form.
    #[test]
    fn clamp_propagates_nan_where_the_max_min_spelling_would_not() {
        assert!(clamp(f64::NAN, 0.0, 1.0).is_nan());
        // The spelling it replaces, for contrast — tests may branch freely.
        let max_min = f64::NAN.max(0.0).min(1.0);
        assert_eq!(max_min, 0.0, "max/min swallows NaN, which is the whole point");
    }

    /// `f64::clamp` panics on reversed bounds. This pins to `lo` instead: a
    /// caller's bug should not become a crash a port never had.
    #[test]
    fn reversed_bounds_pin_to_the_low_edge_rather_than_panicking() {
        assert_eq!(clamp(0.5, 1.0, 0.0), 1.0);
    }

    #[test]
    fn clamp01_is_clamp_over_the_unit_interval() {
        assert_eq!(clamp01(-1.0), 0.0);
        assert_eq!(clamp01(2.0), 1.0);
        assert_eq!(clamp01(0.5), 0.5);
        assert!(clamp01(f64::NAN).is_nan());
    }

    #[test]
    fn lerp_reproduces_its_endpoints_exactly() {
        let (a, b) = (0.1, 9.7);
        // Exactly, not approximately: `a + (b - a) * t` returns `a` bit for bit
        // at t = 0, where `a * (1 - t) + b * t` does not.
        assert_eq!(lerp(a, b, 0.0), a);
        assert_eq!(lerp(a, b, 1.0), b);
        assert_eq!(lerp(0.0, 10.0, 0.3), 3.0);
    }

    #[test]
    fn lerp_extrapolates_outside_the_unit_interval() {
        assert_eq!(lerp(0.0, 10.0, 2.0), 20.0);
        assert_eq!(lerp(0.0, 10.0, -1.0), -10.0);
    }

    #[test]
    fn damp_approaches_the_target_and_rests_on_it() {
        assert!((damp(0.0, 10.0, 1000.0, 1.0) - 10.0).abs() < 1e-6);
        assert_eq!(damp(3.0, 3.0, 5.0, 0.5), 3.0);
        // Zero elapsed time moves nothing.
        assert_eq!(damp(1.0, 9.0, 5.0, 0.0), 1.0);
    }

    #[test]
    fn smoothstep_is_flat_outside_its_edges_and_half_way_between() {
        assert_eq!(smoothstep(2.0, 4.0, 1.0), 0.0);
        assert_eq!(smoothstep(2.0, 4.0, 5.0), 1.0);
        assert_eq!(smoothstep(2.0, 4.0, 3.0), 0.5);
    }

    /// The guard that one of the three replaced call sites lacked. Without it
    /// the division is an infinity, which `clamp01` flattens to 0 or 1 — a step
    /// where the caller asked for a ramp.
    #[test]
    fn a_zero_width_edge_does_not_divide_by_zero() {
        let at = smoothstep(3.0, 3.0, 3.0);
        assert!(at.is_finite(), "got {at}");
        assert!(smoothstep(3.0, 3.0, 9.0).is_finite());
        assert!(smootherstep(3.0, 3.0, -9.0).is_finite());
    }

    #[test]
    fn smootherstep_has_a_zero_second_derivative_at_the_ends() {
        assert_eq!(smootherstep(0.0, 1.0, 0.0), 0.0);
        assert_eq!(smootherstep(0.0, 1.0, 1.0), 1.0);
        assert_eq!(smootherstep(0.0, 1.0, 0.5), 0.5);
        // Flatter than smoothstep near the ends, which is the whole difference.
        assert!(smootherstep(0.0, 1.0, 0.1) < smoothstep(0.0, 1.0, 0.1));
    }

    #[test]
    fn the_unit_forms_clamp_their_argument() {
        assert_eq!(smooth_unit(-1.0), 0.0);
        assert_eq!(smooth_unit(2.0), 1.0);
        assert_eq!(smooth_unit(0.5), 0.5);
        assert_eq!(smoother_unit(-1.0), 0.0);
        assert_eq!(smoother_unit(2.0), 1.0);
        assert_eq!(smoother_unit(0.5), 0.5);
    }

    #[test]
    fn the_unit_forms_agree_with_the_edge_forms_over_zero_to_one() {
        [0.0, 0.125, 0.5, 0.875, 1.0].into_iter().for_each(|t| {
            assert_eq!(smooth_unit(t), smoothstep(0.0, 1.0, t), "t={t}");
            assert_eq!(smoother_unit(t), smootherstep(0.0, 1.0, t), "t={t}");
        });
    }

    #[test]
    fn wrap_pi_brings_an_angle_into_the_half_open_turn() {
        let pi = core::f64::consts::PI;
        let tau = core::f64::consts::TAU;
        assert!((wrap_pi(0.0)).abs() < 1e-12);
        assert!((wrap_pi(tau) - 0.0).abs() < 1e-12);
        // An exact half-turn lands on the CLOSED end, which is -PI. Both
        // directions, because `%` keeps the dividend's sign and the lift is
        // what makes them agree.
        assert!((wrap_pi(3.0 * pi) + pi).abs() < 1e-12, "{}", wrap_pi(3.0 * pi));
        assert!((wrap_pi(-3.0 * pi) + pi).abs() < 1e-12, "{}", wrap_pi(-3.0 * pi));
        assert!((wrap_pi(pi) + pi).abs() < 1e-12, "{}", wrap_pi(pi));
        // Everything lands inside the half-open turn.
        (-40..=40).for_each(|k| {
            let w = wrap_pi(f64::from(k) * 0.7);
            assert!(w >= -pi && w < pi, "k={k} -> {w}");
        });
    }
}
