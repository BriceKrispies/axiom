//! Stable identity for a renderable component.

/// A stable, opaque identifier for a [`crate::Renderable`] component
/// owned by a scene.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RenderableId(u64);

impl RenderableId {
    pub const INVALID: RenderableId = RenderableId(0);

    pub const fn from_raw(raw: u64) -> Self {
        RenderableId(raw)
    }

    pub const fn raw(self) -> u64 {
        self.0
    }

    pub const fn is_valid(self) -> bool {
        self.0 != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_is_zero() {
        assert!(!RenderableId::INVALID.is_valid());
    }

    #[test]
    fn non_zero_is_valid() {
        assert!(RenderableId::from_raw(7).is_valid());
    }

    #[test]
    fn equality_and_ordering_are_numeric() {
        assert_eq!(RenderableId::from_raw(1), RenderableId::from_raw(1));
        assert!(RenderableId::from_raw(1) < RenderableId::from_raw(2));
    }
}
