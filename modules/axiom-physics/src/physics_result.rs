//! The physics module's fallible-result alias.

use crate::physics_error::PhysicsError;

/// The result type returned by every fallible physics operation.
pub type PhysicsResult<T> = Result<T, PhysicsError>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physics_error_code::PhysicsErrorCode;

    fn ok_path() -> PhysicsResult<u32> {
        Ok(7)
    }

    fn err_path() -> PhysicsResult<u32> {
        Err(PhysicsError::body_not_found("not here"))
    }

    #[test]
    fn ok_carries_value() {
        assert_eq!(ok_path().unwrap(), 7);
    }

    #[test]
    fn err_carries_machine_identity() {
        let err = err_path().unwrap_err();
        assert_eq!(err.code(), PhysicsErrorCode::BodyNotFound);
        assert!(err.math().is_none());
    }
}
