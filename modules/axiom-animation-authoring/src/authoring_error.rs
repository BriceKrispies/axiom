//! The authoring module's deterministic error value.

use crate::authoring_error_code::AuthoringErrorCode;

/// A deterministic authoring-module error. Identity is the [`AuthoringErrorCode`]
/// alone: two errors with the same code compare equal regardless of the static
/// human message, so assertions stay machine-stable across builds and replays.
/// No authoring operation panics for a validation failure — it returns one of
/// these instead.
#[derive(Debug, Clone, Copy)]
pub struct AuthoringError {
    code: AuthoringErrorCode,
    message: &'static str,
}

impl AuthoringError {
    /// An authoring error with the given code and static message.
    pub const fn new(code: AuthoringErrorCode, message: &'static str) -> Self {
        AuthoringError { code, message }
    }

    /// A referenced rig id does not exist.
    pub const fn rig_not_found(message: &'static str) -> Self {
        AuthoringError::new(AuthoringErrorCode::RigNotFound, message)
    }

    /// A referenced motion id does not exist.
    pub const fn motion_not_found(message: &'static str) -> Self {
        AuthoringError::new(AuthoringErrorCode::MotionNotFound, message)
    }

    /// A referenced phase id does not exist for its motion.
    pub const fn phase_not_found(message: &'static str) -> Self {
        AuthoringError::new(AuthoringErrorCode::PhaseNotFound, message)
    }

    /// A referenced plan id does not exist.
    pub const fn plan_not_found(message: &'static str) -> Self {
        AuthoringError::new(AuthoringErrorCode::PlanNotFound, message)
    }

    /// A referenced joint name is absent from the rig.
    pub const fn unknown_joint(message: &'static str) -> Self {
        AuthoringError::new(AuthoringErrorCode::UnknownJoint, message)
    }

    /// A referenced effector name is absent from the rig.
    pub const fn unknown_effector(message: &'static str) -> Self {
        AuthoringError::new(AuthoringErrorCode::UnknownEffector, message)
    }

    /// A referenced target name was never declared on the motion.
    pub const fn unknown_target(message: &'static str) -> Self {
        AuthoringError::new(AuthoringErrorCode::UnknownTarget, message)
    }

    /// A phase or event carried an empty/inverted/out-of-range tick span.
    pub const fn invalid_tick_range(message: &'static str) -> Self {
        AuthoringError::new(AuthoringErrorCode::InvalidTickRange, message)
    }

    /// Two phases cover overlapping tick ranges.
    pub const fn overlapping_phases(message: &'static str) -> Self {
        AuthoringError::new(AuthoringErrorCode::OverlappingPhases, message)
    }

    /// An authored position/transform carried a non-finite component.
    pub const fn non_finite_value(message: &'static str) -> Self {
        AuthoringError::new(AuthoringErrorCode::NonFiniteValue, message)
    }

    /// The stable error classification.
    pub const fn code(&self) -> AuthoringErrorCode {
        self.code
    }

    /// The static human-readable message (never part of identity).
    pub const fn message(&self) -> &'static str {
        self.message
    }

    /// The stable numeric error code — inspect *which* error occurred without
    /// naming the internal code enum.
    pub const fn raw_code(&self) -> u16 {
        self.code.raw()
    }
}

/// Equality on machine identity only (the code), never the message.
impl PartialEq for AuthoringError {
    fn eq(&self, other: &Self) -> bool {
        self.code == other.code
    }
}

impl Eq for AuthoringError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_ignores_message_but_keeps_code() {
        let a = AuthoringError::unknown_joint("x");
        let b = AuthoringError::unknown_joint("totally different");
        assert_eq!(a, b);
        assert_eq!(a.code(), AuthoringErrorCode::UnknownJoint);
        assert_eq!(a.message(), "x");
        assert_eq!(a.raw_code(), 5);
    }

    #[test]
    fn different_codes_are_not_equal() {
        assert_ne!(
            AuthoringError::rig_not_found(""),
            AuthoringError::motion_not_found("")
        );
    }

    #[test]
    fn shorthand_constructors_use_their_codes() {
        assert_eq!(AuthoringError::rig_not_found("").code(), AuthoringErrorCode::RigNotFound);
        assert_eq!(AuthoringError::motion_not_found("").code(), AuthoringErrorCode::MotionNotFound);
        assert_eq!(AuthoringError::phase_not_found("").code(), AuthoringErrorCode::PhaseNotFound);
        assert_eq!(AuthoringError::plan_not_found("").code(), AuthoringErrorCode::PlanNotFound);
        assert_eq!(AuthoringError::unknown_joint("").code(), AuthoringErrorCode::UnknownJoint);
        assert_eq!(AuthoringError::unknown_effector("").code(), AuthoringErrorCode::UnknownEffector);
        assert_eq!(AuthoringError::unknown_target("").code(), AuthoringErrorCode::UnknownTarget);
        assert_eq!(AuthoringError::invalid_tick_range("").code(), AuthoringErrorCode::InvalidTickRange);
        assert_eq!(AuthoringError::overlapping_phases("").code(), AuthoringErrorCode::OverlappingPhases);
        assert_eq!(AuthoringError::non_finite_value("").code(), AuthoringErrorCode::NonFiniteValue);
    }
}
