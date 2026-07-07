//! The physical-animation bridge's deterministic error value.

use crate::physical_error_code::PhysicalErrorCode;

/// A deterministic bridge error. Identity is the [`PhysicalErrorCode`] alone
/// (the message is not part of identity), so assertions stay machine-stable.
/// Failures from the composed facades are folded into `PhysicsFailed` /
/// `AuthoringFailed` — the bridge never panics for a composition failure.
#[derive(Debug, Clone, Copy)]
pub struct PhysicalError {
    code: PhysicalErrorCode,
    message: &'static str,
}

impl PhysicalError {
    /// Construct an error with a code and static message.
    pub const fn new(code: PhysicalErrorCode, message: &'static str) -> Self {
        PhysicalError { code, message }
    }

    /// A composed `axiom-physics` call failed.
    pub const fn physics_failed(message: &'static str) -> Self {
        PhysicalError::new(PhysicalErrorCode::PhysicsFailed, message)
    }

    /// A composed `axiom-animation-authoring` call failed.
    pub const fn authoring_failed(message: &'static str) -> Self {
        PhysicalError::new(PhysicalErrorCode::AuthoringFailed, message)
    }

    /// An operation needed a binding that has not been built.
    pub const fn not_bound(message: &'static str) -> Self {
        PhysicalError::new(PhysicalErrorCode::NotBound, message)
    }

    /// An operation needed a ball that has not been attached.
    pub const fn no_ball(message: &'static str) -> Self {
        PhysicalError::new(PhysicalErrorCode::NoBall, message)
    }

    /// The stable error classification.
    pub const fn code(&self) -> PhysicalErrorCode {
        self.code
    }

    /// The static human-readable message (never part of identity).
    pub const fn message(&self) -> &'static str {
        self.message
    }

    /// The stable numeric error code.
    pub const fn raw_code(&self) -> u16 {
        self.code.raw()
    }
}

impl PartialEq for PhysicalError {
    fn eq(&self, other: &Self) -> bool {
        self.code == other.code
    }
}

impl Eq for PhysicalError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_is_the_code_and_constructors_use_their_codes() {
        let a = PhysicalError::physics_failed("x");
        let b = PhysicalError::physics_failed("different");
        assert_eq!(a, b);
        assert_eq!(a.message(), "x");
        assert_eq!(a.raw_code(), 1);
        assert_eq!(PhysicalError::physics_failed("").code(), PhysicalErrorCode::PhysicsFailed);
        assert_eq!(PhysicalError::authoring_failed("").code(), PhysicalErrorCode::AuthoringFailed);
        assert_eq!(PhysicalError::not_bound("").code(), PhysicalErrorCode::NotBound);
        assert_eq!(PhysicalError::no_ball("").code(), PhysicalErrorCode::NoBall);
        assert_ne!(PhysicalError::not_bound(""), PhysicalError::no_ball(""));
    }
}
