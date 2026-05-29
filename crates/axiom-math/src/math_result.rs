//! The math layer's fallible-result alias.

use crate::math_error::MathError;

/// The result type returned by every fallible Layer-02 math operation.
///
/// Fixing the error type to [`MathError`] means callers always pattern-match
/// against the same deterministic `(MathErrorCode, optional KernelError)`
/// identity — there is one error shape in the math layer, not many.
pub type MathResult<T> = Result<T, MathError>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math_error_code::MathErrorCode;

    fn ok_path() -> MathResult<f32> {
        Ok(2.5)
    }

    fn err_path() -> MathResult<f32> {
        Err(MathError::divide_by_zero("denominator was zero"))
    }

    #[test]
    fn ok_carries_value() {
        assert_eq!(ok_path().unwrap(), 2.5);
    }

    #[test]
    fn err_carries_machine_identity() {
        let err = err_path().unwrap_err();
        assert_eq!(err.code(), MathErrorCode::DivideByZero);
        assert!(err.kernel().is_none());
    }
}
