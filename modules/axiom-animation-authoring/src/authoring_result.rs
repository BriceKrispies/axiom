//! The authoring module's result alias.

use crate::authoring_error::AuthoringError;

/// A fallible authoring operation: `Ok(value)` or a deterministic
/// [`AuthoringError`]. Every public facade method that can fail returns one of
/// these — the module never panics for a validation failure.
pub type AuthoringResult<T> = Result<T, AuthoringError>;
