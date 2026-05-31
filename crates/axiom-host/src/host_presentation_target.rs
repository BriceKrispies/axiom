//! The abstract thing the engine wants to present into.

use axiom_kernel::HandleId;

use crate::host_error::HostError;
use crate::host_result::HostResult;

/// An abstract, host-owned presentation target.
///
/// A target identifies *the thing the engine wants to present into* — a
/// future window, canvas, or off-screen surface — without naming any
/// browser/OS/WebGPU object. Its identity is a kernel [`HandleId`]; a future
/// browser/native adapter is responsible for mapping that stable id onto a
/// real platform object at bind time.
///
/// Construct one only through [`crate::HostApi`]; the constructor is
/// crate-private so a target's identity is always minted by the host
/// boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HostPresentationTarget {
    id: HandleId,
    label: &'static str,
}

impl HostPresentationTarget {
    /// Construct a validated target. Crate-private: callers go through
    /// [`crate::HostApi::presentation_target`].
    ///
    /// Failure paths:
    /// - a null (zero) handle id → `InvalidPresentationTarget`,
    /// - an empty label → `InvalidPresentationTarget`.
    pub(crate) fn new(id: HandleId, label: &'static str) -> HostResult<Self> {
        if !id.is_valid() {
            return Err(HostError::invalid_presentation_target(
                "presentation target handle id must be non-null",
            ));
        }
        if label.is_empty() {
            return Err(HostError::invalid_presentation_target(
                "presentation target label must be non-empty",
            ));
        }
        Ok(HostPresentationTarget { id, label })
    }

    /// The stable kernel identity of this target.
    pub const fn id(&self) -> HandleId {
        self.id
    }

    /// The deterministic human-readable label of this target.
    pub const fn label(&self) -> &'static str {
        self.label
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host_error_code::HostErrorCode;

    #[test]
    fn valid_target_is_constructed() {
        let t = HostPresentationTarget::new(HandleId::from_raw(1), "primary").unwrap();
        assert_eq!(t.id().raw(), 1);
        assert_eq!(t.label(), "primary");
    }

    #[test]
    fn null_handle_is_rejected() {
        let err = HostPresentationTarget::new(HandleId::NULL, "primary").unwrap_err();
        assert_eq!(err.code(), HostErrorCode::InvalidPresentationTarget);
    }

    #[test]
    fn empty_label_is_rejected() {
        let err = HostPresentationTarget::new(HandleId::from_raw(1), "").unwrap_err();
        assert_eq!(err.code(), HostErrorCode::InvalidPresentationTarget);
    }

    #[test]
    fn same_inputs_produce_equal_targets() {
        let a = HostPresentationTarget::new(HandleId::from_raw(2), "main").unwrap();
        let b = HostPresentationTarget::new(HandleId::from_raw(2), "main").unwrap();
        assert_eq!(a, b);
    }
}
