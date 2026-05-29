//! The frame layer's fallible-result alias.

use crate::frame_error::FrameError;

/// The result type returned by every fallible Layer-04 frame operation.
///
/// Fixing the error type to [`FrameError`] means callers always pattern-match
/// against the same deterministic `(FrameErrorCode, Option<HostError>)`
/// identity — there is one error shape at the engine frame boundary.
pub type FrameResult<T> = Result<T, FrameError>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame_error_code::FrameErrorCode;

    fn ok_path() -> FrameResult<u32> {
        Ok(7)
    }

    fn err_path() -> FrameResult<u32> {
        Err(FrameError::invalid_frame_timing("steps mismatch"))
    }

    #[test]
    fn ok_carries_value() {
        assert_eq!(ok_path().unwrap(), 7);
    }

    #[test]
    fn err_carries_machine_identity() {
        let err = err_path().unwrap_err();
        assert_eq!(err.code(), FrameErrorCode::InvalidFrameTiming);
        assert!(err.host().is_none());
    }
}
