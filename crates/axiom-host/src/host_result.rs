//! The host layer's fallible-result alias.

use crate::host_error::HostError;

/// The result type returned by every fallible Layer-03 host operation.
///
/// Fixing the error type to [`HostError`] means callers always pattern-match
/// against the same deterministic `(HostErrorCode, Option<RuntimeError>)`
/// identity — there is one error shape at the host boundary, not many.
pub type HostResult<T> = Result<T, HostError>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host_error_code::HostErrorCode;

    fn ok_path() -> HostResult<u32> {
        Ok(7)
    }

    fn err_path() -> HostResult<u32> {
        Err(HostError::invalid_scale_factor("nan"))
    }

    #[test]
    fn ok_carries_value() {
        assert_eq!(ok_path().unwrap(), 7);
    }

    #[test]
    fn err_carries_machine_identity() {
        let err = err_path().unwrap_err();
        assert_eq!(err.code(), HostErrorCode::InvalidScaleFactor);
        assert!(err.runtime().is_none());
    }
}
