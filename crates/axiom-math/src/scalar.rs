//! The deterministic scalar policy for Layer 02.

use crate::math_error::MathError;
use crate::math_result::MathResult;

/// The math layer's scalar policy.
///
/// Axiom standardises on IEEE-754 `f32` as the engine scalar. `Scalar` is a
/// zero-sized policy holder that exposes the chosen constants and the finite
/// scalar validation rule the rest of the layer follows. There is no implicit
/// rounding, no clamping and no global epsilon — every checked operation must
/// route through [`Scalar::validate_finite`] (or take an explicit
/// [`crate::Epsilon`] — kept private; reached via [`crate::MathApi`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Scalar;

impl Scalar {
    /// The default tolerance used for approximate comparisons when no
    /// explicit [`crate::Epsilon`] is supplied. `1e-6` is comfortably above
    /// `f32::EPSILON` while still rejecting genuinely distinct values.
    pub const DEFAULT_EPSILON: f32 = 1.0e-6;

    /// Whether `v` is a finite real number (neither `NaN` nor `±Inf`).
    pub const fn is_finite_value(v: f32) -> bool {
        v.is_finite()
    }

    /// Return `v` if it is finite; otherwise produce a
    /// [`crate::math_error_code::MathErrorCode::NonFiniteScalar`] error.
    pub fn validate_finite(v: f32) -> MathResult<f32> {
        if v.is_finite() {
            Ok(v)
        } else {
            Err(MathError::non_finite_scalar(
                "math scalar must be finite (no NaN, no Inf)",
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math_error_code::MathErrorCode;

    #[test]
    fn default_epsilon_is_a_sensible_positive_tolerance() {
        assert!(Scalar::DEFAULT_EPSILON > 0.0);
        assert!(Scalar::DEFAULT_EPSILON < 1.0e-3);
    }

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

    #[test]
    fn default_is_a_no_state_marker() {
        // The policy is a zero-sized marker; default must equal the unit value.
        assert_eq!(Scalar, Scalar::default());
    }
}
