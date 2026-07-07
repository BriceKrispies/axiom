//! The physical-animation bridge's result alias and the two facade-funnel helpers.

use crate::physical_error::PhysicalError;

/// A fallible bridge operation: `Ok(value)` or a deterministic [`PhysicalError`].
pub type PhysicalResult<T> = Result<T, PhysicalError>;

/// Fold an `axiom-physics` result into a bridge result. Generic over the physics
/// error type (which the bridge cannot name) — every physics failure funnels here.
pub(crate) fn phys<T, E>(result: Result<T, E>) -> PhysicalResult<T> {
    result.map_err(|_| PhysicalError::physics_failed("an axiom-physics call failed"))
}

/// Fold an `axiom-animation-authoring` result into a bridge result. Generic over
/// the authoring error type — every authoring failure funnels here.
pub(crate) fn auth<T, E>(result: Result<T, E>) -> PhysicalResult<T> {
    result.map_err(|_| PhysicalError::authoring_failed("an axiom-animation-authoring call failed"))
}
