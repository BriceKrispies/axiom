//! The host layer's deterministic error value.

use axiom_runtime::RuntimeError;

use crate::host_error_code::HostErrorCode;

/// A deterministic Layer-03 host boundary error.
///
/// Identity is `(code, runtime-cause-identity)`. Two errors with the same
/// [`HostErrorCode`] and the same wrapped [`RuntimeError`] compare equal
/// regardless of the static human message — error checks stay machine-stable
/// across builds and replays.
#[derive(Debug, Clone, Copy)]
pub struct HostError {
    code: HostErrorCode,
    message: &'static str,
    runtime: Option<RuntimeError>,
}

impl HostError {
    /// A host-only error without a wrapped runtime cause.
    pub const fn new(code: HostErrorCode, message: &'static str) -> Self {
        HostError {
            code,
            message,
            runtime: None,
        }
    }

    /// A host error that wraps a runtime failure.
    pub const fn with_runtime(
        code: HostErrorCode,
        message: &'static str,
        cause: RuntimeError,
    ) -> Self {
        HostError {
            code,
            message,
            runtime: Some(cause),
        }
    }

    /// Shorthand for [`HostErrorCode::InvalidViewportDimensions`].
    pub const fn invalid_viewport_dimensions(message: &'static str) -> Self {
        HostError::new(HostErrorCode::InvalidViewportDimensions, message)
    }

    /// Shorthand for [`HostErrorCode::InvalidScaleFactor`].
    pub const fn invalid_scale_factor(message: &'static str) -> Self {
        HostError::new(HostErrorCode::InvalidScaleFactor, message)
    }

    /// Shorthand for [`HostErrorCode::InvalidFrameSequence`].
    pub const fn invalid_frame_sequence(message: &'static str) -> Self {
        HostError::new(HostErrorCode::InvalidFrameSequence, message)
    }

    /// Shorthand for [`HostErrorCode::InvalidBoundaryConfig`].
    pub const fn invalid_boundary_config(message: &'static str) -> Self {
        HostError::new(HostErrorCode::InvalidBoundaryConfig, message)
    }

    /// Shorthand for [`HostErrorCode::InvalidLifecycleTransition`].
    pub const fn invalid_lifecycle_transition(message: &'static str) -> Self {
        HostError::new(HostErrorCode::InvalidLifecycleTransition, message)
    }

    /// Shorthand for [`HostErrorCode::RuntimeStepFailed`] that preserves the
    /// runtime cause.
    pub const fn runtime_step_failed(message: &'static str, cause: RuntimeError) -> Self {
        HostError::with_runtime(HostErrorCode::RuntimeStepFailed, message, cause)
    }

    /// Shorthand for [`HostErrorCode::InvalidPresentationTarget`].
    pub const fn invalid_presentation_target(message: &'static str) -> Self {
        HostError::new(HostErrorCode::InvalidPresentationTarget, message)
    }

    /// Shorthand for [`HostErrorCode::InvalidSurfaceHandle`].
    pub const fn invalid_surface_handle(message: &'static str) -> Self {
        HostError::new(HostErrorCode::InvalidSurfaceHandle, message)
    }

    /// Shorthand for [`HostErrorCode::InvalidPresentationRequest`].
    pub const fn invalid_presentation_request(message: &'static str) -> Self {
        HostError::new(HostErrorCode::InvalidPresentationRequest, message)
    }

    /// The machine-readable host error code.
    pub const fn code(&self) -> HostErrorCode {
        self.code
    }

    /// The static human message. Never used for comparison.
    pub const fn message(&self) -> &'static str {
        self.message
    }

    /// The wrapped runtime cause, if this failure originated there.
    pub const fn runtime(&self) -> Option<RuntimeError> {
        self.runtime
    }
}

/// Equality on machine identity only.
impl PartialEq for HostError {
    fn eq(&self, other: &Self) -> bool {
        self.code == other.code && self.runtime == other.runtime
    }
}

impl Eq for HostError {}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_runtime::{RuntimeError, RuntimeErrorCode};

    #[test]
    fn identity_ignores_message() {
        let a = HostError::new(HostErrorCode::InvalidScaleFactor, "x");
        let b = HostError::new(HostErrorCode::InvalidScaleFactor, "totally different");
        assert_eq!(a, b);
    }

    #[test]
    fn different_code_is_not_equal() {
        let a = HostError::new(HostErrorCode::InvalidScaleFactor, "");
        let b = HostError::new(HostErrorCode::InvalidFrameSequence, "");
        assert_ne!(a, b);
    }

    #[test]
    fn shorthand_constructors_use_their_codes() {
        assert_eq!(
            HostError::invalid_viewport_dimensions("").code(),
            HostErrorCode::InvalidViewportDimensions
        );
        assert_eq!(
            HostError::invalid_scale_factor("").code(),
            HostErrorCode::InvalidScaleFactor
        );
        assert_eq!(
            HostError::invalid_frame_sequence("").code(),
            HostErrorCode::InvalidFrameSequence
        );
        assert_eq!(
            HostError::invalid_boundary_config("").code(),
            HostErrorCode::InvalidBoundaryConfig
        );
        assert_eq!(
            HostError::invalid_lifecycle_transition("").code(),
            HostErrorCode::InvalidLifecycleTransition
        );
    }

    #[test]
    fn wraps_a_runtime_error_and_preserves_identity() {
        let cause = RuntimeError::new(RuntimeErrorCode::StepWhileNotRunning, "x");
        let wrapped = HostError::runtime_step_failed("runtime step failed", cause);
        assert_eq!(wrapped.code(), HostErrorCode::RuntimeStepFailed);
        assert_eq!(wrapped.runtime(), Some(cause));
    }

    #[test]
    fn wrapped_and_unwrapped_are_not_equal() {
        let cause = RuntimeError::new(RuntimeErrorCode::StepWhileNotRunning, "x");
        let bare = HostError::new(HostErrorCode::RuntimeStepFailed, "x");
        let wrapped = HostError::with_runtime(HostErrorCode::RuntimeStepFailed, "x", cause);
        assert_ne!(bare, wrapped);
    }

    #[test]
    fn message_is_preserved_but_not_part_of_identity() {
        let e = HostError::new(HostErrorCode::InvalidScaleFactor, "scale was NaN");
        assert_eq!(e.message(), "scale was NaN");
    }
}
