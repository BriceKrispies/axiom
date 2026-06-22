//! An opaque, deterministic handle to a future live presentation surface.

use axiom_kernel::HandleId;

use crate::host_error::HostError;
use crate::host_result::HostResult;

/// An opaque, deterministic handle to a future live surface.
///
/// A surface handle is pure identity: a kernel [`HandleId`] and nothing
/// else. It does **not** store a window handle, a canvas, a swapchain, or
/// any GPU/OS object — a future browser/native adapter binds the real
/// surface to this handle out-of-band. The handle is fully inspectable (its
/// raw id) so tests and future backends can correlate it.
///
/// Construct one only through [`crate::HostApi`]; the constructor is
/// crate-private so handles are always minted by the host boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HostSurfaceHandle {
    id: HandleId,
}

impl HostSurfaceHandle {
    /// Construct a validated surface handle. Crate-private: callers go
    /// through [`crate::HostApi::surface_handle`].
    ///
    /// Failure path: a null (zero) handle id → `InvalidSurfaceHandle`.
    pub(crate) fn new(id: HandleId) -> HostResult<Self> {
        id.is_valid()
            .then_some(HostSurfaceHandle { id })
            .ok_or_else(|| HostError::invalid_surface_handle("surface handle id must be non-null"))
    }

    /// The stable kernel identity of this surface handle.
    pub const fn id(&self) -> HandleId {
        self.id
    }

    /// Whether the handle's identity is valid (non-null). Always `true` for a
    /// handle obtained from [`crate::HostApi`]; exposed for downstream code
    /// that re-checks defensively.
    pub const fn is_valid(&self) -> bool {
        self.id.is_valid()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host_error_code::HostErrorCode;

    #[test]
    fn valid_handle_is_constructed() {
        let h = HostSurfaceHandle::new(HandleId::from_raw(7)).unwrap();
        assert_eq!(h.id().raw(), 7);
        assert!(h.is_valid());
    }

    #[test]
    fn null_handle_is_rejected() {
        let err = HostSurfaceHandle::new(HandleId::NULL).unwrap_err();
        assert_eq!(err.code(), HostErrorCode::InvalidSurfaceHandle);
    }

    #[test]
    fn same_id_produces_equal_handles() {
        let a = HostSurfaceHandle::new(HandleId::from_raw(3)).unwrap();
        let b = HostSurfaceHandle::new(HandleId::from_raw(3)).unwrap();
        assert_eq!(a, b);
    }
}
