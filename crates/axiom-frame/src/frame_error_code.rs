//! Machine-readable frame-error code.

/// The reason a Layer-04 engine frame boundary operation failed.
///
/// Codes are layer-04 identities; lower layers carry their own closed
/// enums, so frame owns its identity and may wrap a host-layer cause when
/// one is available (see [`crate::FrameError::with_host`]). Two errors with
/// the same code (and the same wrapped host cause, if any) compare equal
/// regardless of message, so error checks stay machine-stable across builds
/// and replays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u16)]
pub enum FrameErrorCode {
    /// Two consecutive frames were built with non-increasing engine frame
    /// indices (the builder enforces strict monotonicity).
    InvalidEngineFrameSequence = 1,
    /// Two consecutive host reports arrived with non-increasing host frame
    /// sequence numbers.
    InvalidHostFrameSequence = 2,
    /// The host report's `steps_executed` did not match the plan's
    /// `steps` — a host-driver bug the frame boundary refuses to paper over.
    InvalidFrameTiming = 3,
    /// A viewport derived from the host's viewport failed Layer-02 math
    /// validation (e.g. a non-finite aspect ratio).
    InvalidViewport = 4,
    /// A host-side adaptation failed; the wrapped [`axiom_host::HostError`]
    /// preserves the host cause.
    HostFrameAdaptationFailed = 5,
}

impl FrameErrorCode {
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
        assert_eq!(FrameErrorCode::InvalidEngineFrameSequence.raw(), 1);
        assert_eq!(FrameErrorCode::HostFrameAdaptationFailed.raw(), 5);
    }

    #[test]
    fn codes_are_distinct_and_ordered() {
        assert_ne!(
            FrameErrorCode::InvalidHostFrameSequence,
            FrameErrorCode::InvalidFrameTiming
        );
        assert!(FrameErrorCode::InvalidEngineFrameSequence < FrameErrorCode::InvalidViewport);
    }
}
