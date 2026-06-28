//! Stable identity for a physics collider.

/// A stable, opaque handle for a collider in a [`crate::PhysicsApi`] world.
///
/// Like [`crate::PhysicsBodyHandle`], collider handles are deterministic `u64`
/// ids assigned monotonically at attach time and never reused. Handle `0` is the
/// invalid sentinel ([`PhysicsColliderHandle::NULL`]); the world's collider
/// allocator starts at `1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PhysicsColliderHandle(u64);

impl PhysicsColliderHandle {
    /// The reserved invalid value (`0`).
    pub const NULL: PhysicsColliderHandle = PhysicsColliderHandle(0);

    /// Construct from a raw `u64`.
    pub const fn from_raw(raw: u64) -> Self {
        PhysicsColliderHandle(raw)
    }

    /// The underlying raw value.
    pub const fn raw(self) -> u64 {
        self.0
    }

    /// `true` iff this is not the invalid sentinel.
    pub const fn is_valid(self) -> bool {
        self.0 != 0
    }
}

impl Default for PhysicsColliderHandle {
    fn default() -> Self {
        PhysicsColliderHandle::NULL
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_is_zero_and_default() {
        assert_eq!(PhysicsColliderHandle::NULL.raw(), 0);
        assert!(!PhysicsColliderHandle::NULL.is_valid());
        assert_eq!(
            PhysicsColliderHandle::default(),
            PhysicsColliderHandle::NULL
        );
    }

    #[test]
    fn non_zero_is_valid_and_round_trips() {
        let h = PhysicsColliderHandle::from_raw(9);
        assert!(h.is_valid());
        assert_eq!(h.raw(), 9);
        assert!(PhysicsColliderHandle::from_raw(u64::MAX).is_valid());
    }

    #[test]
    fn equality_and_ordering_are_numeric() {
        assert_eq!(
            PhysicsColliderHandle::from_raw(3),
            PhysicsColliderHandle::from_raw(3)
        );
        assert_ne!(
            PhysicsColliderHandle::from_raw(3),
            PhysicsColliderHandle::from_raw(4)
        );
        assert!(PhysicsColliderHandle::from_raw(1) < PhysicsColliderHandle::from_raw(2));
    }
}
