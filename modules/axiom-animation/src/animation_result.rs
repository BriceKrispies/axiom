//! The animation module's result alias.

use crate::animation_error::AnimationError;

/// A fallible animation operation: `Ok(value)` or a deterministic
/// [`AnimationError`]. Every public facade method that can fail returns one of
/// these — the module never panics for a validation failure.
pub type AnimationResult<T> = Result<T, AnimationError>;
