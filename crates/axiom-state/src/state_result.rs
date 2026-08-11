//! The result alias every fallible state operation returns.

use crate::state_error::StateError;

/// The outcome of a state operation: a value, or a [`StateError`] identity.
pub type StateResult<T> = Result<T, StateError>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state_error_code::StateErrorCode;

    #[test]
    fn the_alias_carries_a_value_or_a_state_error() {
        let ok: StateResult<u32> = Ok(7);
        assert_eq!(ok, Ok(7));

        let failed: StateResult<u32> =
            Err(StateError::new(StateErrorCode::InvalidSchema, "bad schema"));
        assert_eq!(
            failed.unwrap_err().code(),
            StateErrorCode::InvalidSchema
        );
    }
}
