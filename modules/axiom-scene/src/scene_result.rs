//! The scene module's fallible-result alias.

use crate::scene_error::SceneError;

/// The result type returned by every fallible scene operation.
pub type SceneResult<T> = Result<T, SceneError>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene_error_code::SceneErrorCode;

    fn ok_path() -> SceneResult<u32> {
        Ok(7)
    }

    fn err_path() -> SceneResult<u32> {
        Err(SceneError::missing_node("not here"))
    }

    #[test]
    fn ok_carries_value() {
        assert_eq!(ok_path().unwrap(), 7);
    }

    #[test]
    fn err_carries_machine_identity() {
        let err = err_path().unwrap_err();
        assert_eq!(err.code(), SceneErrorCode::MissingNode);
        assert!(err.math().is_none());
    }
}
