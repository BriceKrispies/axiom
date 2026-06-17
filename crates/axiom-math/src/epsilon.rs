//! The math layer's approximate-equality tolerance type.

use crate::math_error::MathError;
use crate::math_result::MathResult;
use crate::scalar::Scalar;

/// A validated, non-negative, finite tolerance for approximate comparison.
///
/// `Epsilon` exists so callers cannot accidentally pass `NaN`, `Inf`, or a
/// negative slack into [`crate::ApproxEq::approx_eq`]. The default value is
/// [`Scalar::DEFAULT_EPSILON`]; use [`Epsilon::new`] for any other tolerance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Epsilon(f32);

impl Epsilon {
    /// The engine-wide default tolerance.
    pub const DEFAULT: Epsilon = Epsilon(Scalar::DEFAULT_EPSILON);

    /// Construct a tolerance, rejecting `NaN`, `±Inf`, and negative values.
    pub fn new(value: f32) -> MathResult<Self> {
        (!value.is_finite())
            .then_some(Err(MathError::non_finite_scalar(
                "epsilon must be finite (no NaN, no Inf)",
            )))
            .or_else(|| {
                (value.is_finite() & (value < 0.0)).then_some(Err(MathError::non_finite_scalar(
                    "epsilon must not be negative",
                )))
            })
            .unwrap_or(Ok(Epsilon(value)))
    }

    /// The underlying tolerance.
    pub const fn value(self) -> f32 {
        self.0
    }
}

impl Default for Epsilon {
    fn default() -> Self {
        Epsilon::DEFAULT
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math_error_code::MathErrorCode;

    #[test]
    fn default_matches_scalar_policy() {
        assert_eq!(Epsilon::default().value(), Scalar::DEFAULT_EPSILON);
        assert_eq!(Epsilon::DEFAULT.value(), Scalar::DEFAULT_EPSILON);
    }

    #[test]
    fn new_accepts_zero_and_positive_finites() {
        assert_eq!(Epsilon::new(0.0).unwrap().value(), 0.0);
        assert_eq!(Epsilon::new(1e-3).unwrap().value(), 1e-3);
    }

    #[test]
    fn new_rejects_negative() {
        let err = Epsilon::new(-1e-6).unwrap_err();
        assert_eq!(err.code(), MathErrorCode::NonFiniteScalar);
    }

    #[test]
    fn new_rejects_nan() {
        let err = Epsilon::new(f32::NAN).unwrap_err();
        assert_eq!(err.code(), MathErrorCode::NonFiniteScalar);
    }

    #[test]
    fn new_rejects_infinity() {
        let err = Epsilon::new(f32::INFINITY).unwrap_err();
        assert_eq!(err.code(), MathErrorCode::NonFiniteScalar);
    }

    #[test]
    fn value_is_round_trip_with_new() {
        let e = Epsilon::new(1e-4).unwrap();
        assert_eq!(e.value(), 1e-4);
    }
}
