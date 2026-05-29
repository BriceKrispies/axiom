//! The frame layer's deterministic error value.

use axiom_host::HostError;

use crate::frame_error_code::FrameErrorCode;

/// A deterministic Layer-04 engine frame error.
///
/// Identity is `(code, host-cause-identity)`. Two errors with the same
/// [`FrameErrorCode`] and the same wrapped [`HostError`] compare equal
/// regardless of the static human message — error checks stay machine-stable
/// across builds and replays.
#[derive(Debug, Clone, Copy)]
pub struct FrameError {
    code: FrameErrorCode,
    message: &'static str,
    host: Option<HostError>,
}

impl FrameError {
    /// A frame-only error without a wrapped host cause.
    pub const fn new(code: FrameErrorCode, message: &'static str) -> Self {
        FrameError {
            code,
            message,
            host: None,
        }
    }

    /// A frame error that wraps a host failure.
    pub const fn with_host(code: FrameErrorCode, message: &'static str, cause: HostError) -> Self {
        FrameError {
            code,
            message,
            host: Some(cause),
        }
    }

    /// Shorthand for [`FrameErrorCode::InvalidEngineFrameSequence`].
    pub const fn invalid_engine_frame_sequence(message: &'static str) -> Self {
        FrameError::new(FrameErrorCode::InvalidEngineFrameSequence, message)
    }

    /// Shorthand for [`FrameErrorCode::InvalidHostFrameSequence`].
    pub const fn invalid_host_frame_sequence(message: &'static str) -> Self {
        FrameError::new(FrameErrorCode::InvalidHostFrameSequence, message)
    }

    /// Shorthand for [`FrameErrorCode::InvalidFrameTiming`].
    pub const fn invalid_frame_timing(message: &'static str) -> Self {
        FrameError::new(FrameErrorCode::InvalidFrameTiming, message)
    }

    /// Shorthand for [`FrameErrorCode::InvalidViewport`].
    pub const fn invalid_viewport(message: &'static str) -> Self {
        FrameError::new(FrameErrorCode::InvalidViewport, message)
    }

    /// Shorthand for [`FrameErrorCode::HostFrameAdaptationFailed`] that
    /// preserves the host cause.
    pub const fn host_frame_adaptation_failed(message: &'static str, cause: HostError) -> Self {
        FrameError::with_host(FrameErrorCode::HostFrameAdaptationFailed, message, cause)
    }

    /// The machine-readable frame error code.
    pub const fn code(&self) -> FrameErrorCode {
        self.code
    }

    /// The static human message. Never used for comparison.
    pub const fn message(&self) -> &'static str {
        self.message
    }

    /// The wrapped host cause, if this failure originated there.
    pub const fn host(&self) -> Option<HostError> {
        self.host
    }
}

/// Equality on machine identity only.
impl PartialEq for FrameError {
    fn eq(&self, other: &Self) -> bool {
        self.code == other.code && self.host == other.host
    }
}

impl Eq for FrameError {}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_host::HostErrorCode;

    fn host_cause() -> HostError {
        HostError::invalid_scale_factor("scale not finite")
    }

    #[test]
    fn identity_ignores_message() {
        let a = FrameError::new(FrameErrorCode::InvalidViewport, "x");
        let b = FrameError::new(FrameErrorCode::InvalidViewport, "totally different");
        assert_eq!(a, b);
    }

    #[test]
    fn different_code_is_not_equal() {
        let a = FrameError::new(FrameErrorCode::InvalidViewport, "");
        let b = FrameError::new(FrameErrorCode::InvalidFrameTiming, "");
        assert_ne!(a, b);
    }

    #[test]
    fn shorthand_constructors_use_their_codes() {
        assert_eq!(
            FrameError::invalid_engine_frame_sequence("").code(),
            FrameErrorCode::InvalidEngineFrameSequence
        );
        assert_eq!(
            FrameError::invalid_host_frame_sequence("").code(),
            FrameErrorCode::InvalidHostFrameSequence
        );
        assert_eq!(
            FrameError::invalid_frame_timing("").code(),
            FrameErrorCode::InvalidFrameTiming
        );
        assert_eq!(
            FrameError::invalid_viewport("").code(),
            FrameErrorCode::InvalidViewport
        );
    }

    #[test]
    fn wraps_a_host_error_and_preserves_identity() {
        let cause = host_cause();
        let wrapped = FrameError::host_frame_adaptation_failed("host failed", cause);
        assert_eq!(wrapped.code(), FrameErrorCode::HostFrameAdaptationFailed);
        assert_eq!(wrapped.host(), Some(cause));
        assert_eq!(
            wrapped.host().unwrap().code(),
            HostErrorCode::InvalidScaleFactor
        );
    }

    #[test]
    fn wrapped_and_unwrapped_are_not_equal() {
        let cause = host_cause();
        let bare = FrameError::new(FrameErrorCode::HostFrameAdaptationFailed, "x");
        let wrapped =
            FrameError::with_host(FrameErrorCode::HostFrameAdaptationFailed, "x", cause);
        assert_ne!(bare, wrapped);
    }

    #[test]
    fn message_is_preserved_but_not_part_of_identity() {
        let e = FrameError::new(FrameErrorCode::InvalidViewport, "viewport had NaN aspect");
        assert_eq!(e.message(), "viewport had NaN aspect");
    }
}
