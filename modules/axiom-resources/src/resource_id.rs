//! Stable, opaque identity for a resource.

/// A stable, opaque identifier for any resource in a
/// [`crate::ResourcesApi`]-managed table.
///
/// IDs are monotonically assigned at registration time and never reused.
/// ID `0` is the invalid sentinel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ResourceId(u64);

impl ResourceId {
    pub const INVALID: ResourceId = ResourceId(0);

    pub const fn from_raw(raw: u64) -> Self {
        ResourceId(raw)
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
        assert!(!ResourceId::INVALID.is_valid());
        assert_eq!(ResourceId::INVALID.raw(), 0);
    }

    #[test]
    fn non_zero_is_valid() {
        assert!(ResourceId::from_raw(1).is_valid());
    }

    #[test]
    fn ids_compare_by_raw() {
        assert_eq!(ResourceId::from_raw(3), ResourceId::from_raw(3));
        assert!(ResourceId::from_raw(1) < ResourceId::from_raw(2));
    }
}
