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

    /// The default tolerance for comparing **double-precision** values
    /// ([`crate::DVec3`], `f64`).
    ///
    /// [`Epsilon::DEFAULT`] is sized for `f32`, whose ~7 significant digits make
    /// `1e-6` a reasonable "equal enough". Applied to an `f64` it is far too
    /// loose: it would call two values equal that disagree in the sixth digit,
    /// which is precisely the precision a double-precision type is carried for.
    /// `1e-12` sits comfortably above `f64::EPSILON` (~2.2e-16) while still
    /// rejecting genuinely distinct values.
    ///
    /// The tolerance is stored as `f32` like every other `Epsilon` — `1e-12` is
    /// exactly representable in range there, and widening it at the comparison
    /// costs nothing. The type is shared on purpose: a tolerance is a tolerance,
    /// and forking `Epsilon` into two types would make every caller choose
    /// between two spellings of one idea.
    pub const DEFAULT_DOUBLE: Epsilon = Epsilon(Scalar::DEFAULT_EPSILON_DOUBLE);

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
        assert_eq!(
            Epsilon::DEFAULT_DOUBLE.value(),
            Scalar::DEFAULT_EPSILON_DOUBLE
        );
        // The double default is strictly tighter, and both are positive and
        // finite — the two properties `Epsilon::new` would have enforced had
        // they not been consts.
        assert!(Epsilon::DEFAULT_DOUBLE.value() < Epsilon::DEFAULT.value());
        assert!(Epsilon::DEFAULT_DOUBLE.value() > 0.0);
        assert!(Epsilon::DEFAULT_DOUBLE.value().is_finite());
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
