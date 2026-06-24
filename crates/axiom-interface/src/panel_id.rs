//! [`PanelId`] — stable interface-panel identity, a newtype over the kernel's
//! `HandleId`.
//!
//! Panels (and interface-tree nodes) need stable, ordered, hashable identity.
//! That is exactly what `axiom_kernel::HandleId` provides, so `PanelId` adapts it
//! into the interface domain rather than inventing a parallel id scheme — the
//! genuine kernel dependency this layer is built on.

use axiom_kernel::HandleId;

/// Stable identity for an interface panel. A thin newtype over the kernel's
/// opaque [`HandleId`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PanelId(HandleId);

impl PanelId {
    /// Wrap a kernel handle as a panel identity. Minted by
    /// [`crate::InterfaceApi::add_panel`].
    pub(crate) fn from_handle(handle: HandleId) -> Self {
        PanelId(handle)
    }

    /// The underlying kernel handle.
    pub fn handle(self) -> HandleId {
        self.0
    }

    /// The raw `u64` backing this identity.
    pub fn raw(self) -> u64 {
        self.0.raw()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_and_unwraps_a_handle() {
        let id = PanelId::from_handle(HandleId::from_raw(7));
        assert_eq!(id.handle(), HandleId::from_raw(7));
        assert_eq!(id.raw(), 7);
    }

    #[test]
    fn identity_is_stable_and_ordered() {
        assert_eq!(
            PanelId::from_handle(HandleId::from_raw(3)),
            PanelId::from_handle(HandleId::from_raw(3))
        );
        assert!(
            PanelId::from_handle(HandleId::from_raw(1))
                < PanelId::from_handle(HandleId::from_raw(2))
        );
        assert_ne!(
            PanelId::from_handle(HandleId::from_raw(1)),
            PanelId::from_handle(HandleId::from_raw(2))
        );
    }
}
