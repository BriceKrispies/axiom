//! The deterministic status of a host presentation request.

/// The deterministic status of a presentation request as evaluated by the
/// host boundary.
///
/// This pass ships only [`HostPresentationStatus::PendingBackend`]: a request
/// can be structurally valid, but the host layer has no live backend bound,
/// so it never claims a real GPU exists. The remaining variants exist so a
/// future live pass can report richer states without changing the boundary
/// shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HostPresentationStatus {
    /// The request is structurally valid but no live backend is bound yet.
    /// This is the only status the current (Recording-only) pass produces.
    PendingBackend,
    /// Presentation is unavailable in this environment (no adapter/device
    /// could ever be provided). Reserved for a future live pass.
    Unavailable,
    /// A live backend is bound and ready to present. **Not reachable** until
    /// the live WebGPU pass binds a real surface/device.
    Ready,
}

impl HostPresentationStatus {
    /// Whether this status indicates a real, ready-to-present backend.
    /// Always `false` in the current pass.
    pub const fn is_ready(self) -> bool {
        matches!(self, HostPresentationStatus::Ready)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variants_are_distinct() {
        assert_ne!(
            HostPresentationStatus::PendingBackend,
            HostPresentationStatus::Ready
        );
        assert_ne!(
            HostPresentationStatus::PendingBackend,
            HostPresentationStatus::Unavailable
        );
        assert_ne!(
            HostPresentationStatus::Unavailable,
            HostPresentationStatus::Ready
        );
    }

    #[test]
    fn pending_backend_is_not_ready() {
        assert!(!HostPresentationStatus::PendingBackend.is_ready());
        assert!(HostPresentationStatus::Ready.is_ready());
    }
}
