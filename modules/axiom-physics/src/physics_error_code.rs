//! Machine-readable physics-error code.

/// The reason a physics operation failed.
/// Codes are physics-module identities: two errors with the same code compare
/// equal regardless of the human-readable message, so error checks stay
/// machine-stable across builds and deterministic replays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u16)]
pub enum PhysicsErrorCode {
    /// A world was constructed with an invalid configuration value
    /// (non-finite gravity, or a zero capacity / iteration / substep count).
    InvalidConfig = 1,
    /// A dynamic body was requested with a non-finite or non-positive mass.
    InvalidMass = 2,
    /// A collider was given an invalid material (friction/restitution/density
    /// out of their allowed ranges).
    InvalidMaterial = 3,
    /// A collider was given an invalid shape dimension (non-finite or
    /// out-of-range radius / half-extent / half-height / plane normal).
    InvalidColliderShape = 4,
    /// An operation referenced a body handle that does not exist.
    BodyNotFound = 5,
    /// An operation referenced a collider handle that does not exist.
    ColliderNotFound = 6,
    /// Creating a body would exceed the configured `max_bodies` capacity.
    BodyCapacityExceeded = 7,
    /// Attaching a collider would exceed the configured `max_colliders` capacity.
    ColliderCapacityExceeded = 8,
    /// A force was applied to a non-dynamic (static or kinematic) body.
    ForceOnNonDynamicBody = 9,
    /// An impulse was applied to a non-dynamic (static or kinematic) body.
    ImpulseOnNonDynamicBody = 10,
    /// A vector or transform input contained a non-finite component
    /// (NaN / ±infinity).
    NonFiniteInput = 11,
    /// A step was requested with a non-positive fixed delta (zero nanoseconds).
    InvalidStep = 12,
    /// A force or impulse was applied to a currently-disabled body.
    OperationOnDisabledBody = 13,
    /// A step would have produced non-finite (`NaN`/`±∞`) body state from
    /// finite-but-extreme inputs; the step was rejected and the world rolled
    /// back to its pre-step state.
    NonFiniteStepResult = 14,
}

impl PhysicsErrorCode {
    /// The stable numeric discriminant.
    pub const fn raw(self) -> u16 {
        self as u16
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discriminants_are_stable() {
        assert_eq!(PhysicsErrorCode::InvalidConfig.raw(), 1);
        assert_eq!(PhysicsErrorCode::ForceOnNonDynamicBody.raw(), 9);
        assert_eq!(PhysicsErrorCode::InvalidStep.raw(), 12);
        assert_eq!(PhysicsErrorCode::OperationOnDisabledBody.raw(), 13);
        assert_eq!(PhysicsErrorCode::NonFiniteStepResult.raw(), 14);
    }

    #[test]
    fn codes_are_distinct_and_ordered() {
        assert_ne!(
            PhysicsErrorCode::InvalidMass,
            PhysicsErrorCode::InvalidMaterial
        );
        assert!(PhysicsErrorCode::InvalidConfig < PhysicsErrorCode::InvalidStep);
    }
}
