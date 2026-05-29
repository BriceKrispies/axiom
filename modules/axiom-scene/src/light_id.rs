//! Stable identity for a light component.

/// A stable, opaque identifier for a [`crate::Light`] component owned by
/// a scene.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LightId(u64);

impl LightId {
    pub const INVALID: LightId = LightId(0);

    pub const fn from_raw(raw: u64) -> Self {
        LightId(raw)
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
        assert!(!LightId::INVALID.is_valid());
    }

    #[test]
    fn non_zero_is_valid() {
        assert!(LightId::from_raw(42).is_valid());
    }

    #[test]
    fn equality_and_ordering_are_numeric() {
        assert_eq!(LightId::from_raw(2), LightId::from_raw(2));
        assert!(LightId::from_raw(2) < LightId::from_raw(5));
    }
}
