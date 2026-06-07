//! Machine-readable host-error code.

/// The reason a Layer-03 host boundary operation failed.
///
/// Codes are layer-03 identities; the kernel/runtime models below have closed
/// enums of their own, so host owns its identity and may wrap a lower-layer
/// cause when one is available (see [`crate::HostError::with_runtime`]).
/// Two errors with the same code (and same wrapped cause, if any) compare
/// equal regardless of message, so error checks stay machine-stable across
/// builds and replays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u16)]
pub enum HostErrorCode {
    /// A viewport was constructed with zero logical or physical extent.
    InvalidViewportDimensions = 1,
    /// A viewport scale factor was not finite or not positive.
    InvalidScaleFactor = 2,
    /// A frame input arrived out of order (sequence did not strictly
    /// increase from the previous frame the driver accepted).
    InvalidFrameSequence = 3,
    /// A host-supplied elapsed-nanos value was invalid (today: only
    /// reserved for forward use, since `u64` already excludes negatives).
    InvalidElapsedNanos = 4,
    /// A [`crate::HostBoundaryConfig`] was rejected at validation time
    /// (zero max steps per frame, or a fixed step rejected by the kernel).
    InvalidBoundaryConfig = 5,
    /// A [`axiom_runtime::Runtime::step`] call returned an error; the
    /// runtime cause is preserved on the wrapping [`crate::HostError`].
    RuntimeStepFailed = 6,
    /// A host lifecycle signal was applied in a state that does not
    /// permit it (today: shutting down then re-applying signals).
    InvalidLifecycleTransition = 7,
    /// A [`crate::HostPresentationTarget`] was constructed with a null
    /// handle id or an empty label.
    InvalidPresentationTarget = 8,
    /// A [`crate::HostSurfaceHandle`] was constructed with a null handle id.
    InvalidSurfaceHandle = 9,
    /// A [`crate::HostPresentationRequest`] bound an invalid/missing target
    /// or surface, or an internally inconsistent adapter/device pairing.
    InvalidPresentationRequest = 10,
    /// A [`crate::Pixels`] quantity was constructed from a non-finite value
    /// (NaN or infinity).
    NonFinitePixels = 11,
}

impl HostErrorCode {
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
        assert_eq!(HostErrorCode::InvalidViewportDimensions.raw(), 1);
        assert_eq!(HostErrorCode::InvalidLifecycleTransition.raw(), 7);
        assert_eq!(HostErrorCode::InvalidPresentationTarget.raw(), 8);
        assert_eq!(HostErrorCode::InvalidSurfaceHandle.raw(), 9);
        assert_eq!(HostErrorCode::InvalidPresentationRequest.raw(), 10);
        assert_eq!(HostErrorCode::NonFinitePixels.raw(), 11);
    }

    #[test]
    fn codes_are_distinct_and_ordered() {
        assert_ne!(
            HostErrorCode::InvalidScaleFactor,
            HostErrorCode::InvalidFrameSequence
        );
        assert!(HostErrorCode::InvalidViewportDimensions < HostErrorCode::RuntimeStepFailed);
    }
}
