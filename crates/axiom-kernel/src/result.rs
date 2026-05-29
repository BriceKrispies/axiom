//! The kernel's fallible-result alias.

use crate::error::KernelError;

/// The result type returned by every fallible kernel operation.
///
/// Fixing the error type to [`KernelError`] means callers always pattern-match
/// against the same deterministic `(scope, code)` identity.
pub type KernelResult<T> = Result<T, KernelError>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error_code::KernelErrorCode;
    use crate::error_scope::KernelErrorScope;

    fn ok_path() -> KernelResult<u32> {
        Ok(7)
    }

    fn err_path() -> KernelResult<u32> {
        Err(KernelError::new(
            KernelErrorScope::Memory,
            KernelErrorCode::OutOfBounds,
            "boom",
        ))
    }

    #[test]
    fn ok_carries_value() {
        assert_eq!(ok_path().unwrap(), 7);
    }

    #[test]
    fn err_carries_identity() {
        let e = err_path().unwrap_err();
        assert_eq!(e.scope(), KernelErrorScope::Memory);
        assert_eq!(e.code(), KernelErrorCode::OutOfBounds);
    }
}
