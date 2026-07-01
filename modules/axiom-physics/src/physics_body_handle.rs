//! Stable identity for a physics rigid body.

/// A stable, opaque handle for a rigid body in a [`crate::PhysicsApi`] world.
/// Handles are deterministic `u64` ids assigned monotonically at body-creation
/// time and **never** reused. They do not depend on pointer addresses and carry
/// no randomness, so the same sequence of `create_*_body` calls always produces
/// the same handles in the same order — safe to store in snapshots, replay
/// logs, and external caches.
/// Handle `0` is the **invalid sentinel** ([`PhysicsBodyHandle::NULL`]); the
/// world's allocator starts at `1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PhysicsBodyHandle(u64);

impl PhysicsBodyHandle {
    /// The reserved invalid value (`0`).
    pub const NULL: PhysicsBodyHandle = PhysicsBodyHandle(0);

    /// Construct from a raw `u64`.
    pub const fn from_raw(raw: u64) -> Self {
        PhysicsBodyHandle(raw)
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

impl Default for PhysicsBodyHandle {
    fn default() -> Self {
        PhysicsBodyHandle::NULL
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_is_zero_and_default() {
        assert_eq!(PhysicsBodyHandle::NULL.raw(), 0);
        assert!(!PhysicsBodyHandle::NULL.is_valid());
        assert_eq!(PhysicsBodyHandle::default(), PhysicsBodyHandle::NULL);
    }

    #[test]
    fn non_zero_is_valid_and_round_trips() {
        let h = PhysicsBodyHandle::from_raw(7);
        assert!(h.is_valid());
        assert_eq!(h.raw(), 7);
        assert!(PhysicsBodyHandle::from_raw(u64::MAX).is_valid());
    }

    #[test]
    fn equality_and_ordering_are_numeric() {
        assert_eq!(PhysicsBodyHandle::from_raw(3), PhysicsBodyHandle::from_raw(3));
        assert_ne!(PhysicsBodyHandle::from_raw(3), PhysicsBodyHandle::from_raw(4));
        assert!(PhysicsBodyHandle::from_raw(1) < PhysicsBodyHandle::from_raw(2));
    }
}
