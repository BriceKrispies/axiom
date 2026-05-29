//! The runtime's fallible-result alias.

use crate::runtime_error::RuntimeError;

/// The `Result` type returned by every fallible runtime operation.
pub type RuntimeResult<T> = Result<T, RuntimeError>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime_error_code::RuntimeErrorCode;

    fn ok_path() -> RuntimeResult<u32> {
        Ok(11)
    }
    fn err_path() -> RuntimeResult<u32> {
        Err(RuntimeError::new(RuntimeErrorCode::SystemFailed, "boom"))
    }

    #[test]
    fn ok_carries_value() {
        assert_eq!(ok_path().unwrap(), 11);
    }
    #[test]
    fn err_carries_identity() {
        assert_eq!(
            err_path().unwrap_err().code(),
            RuntimeErrorCode::SystemFailed
        );
    }
}
