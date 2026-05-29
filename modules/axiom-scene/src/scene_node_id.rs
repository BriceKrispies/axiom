//! Stable identity for a scene node.

/// A stable, opaque identifier for a [`crate::Scene`] node.
///
/// IDs are assigned monotonically by the scene at node-creation time and
/// are **never** reused: removing a node leaves its ID retired forever.
/// That makes IDs deterministically reproducible (same scene operations
/// always produce the same IDs in the same order) and safe to store in
/// snapshot tests, replay logs, and external caches.
///
/// ID `0` is the **invalid sentinel**. The scene's `next_id` counter
/// starts at `1`, so a freshly-constructed `SceneNodeId(0)` is recognised
/// as not-a-real-node by [`SceneNodeId::is_valid`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SceneNodeId(u64);

impl SceneNodeId {
    /// The reserved invalid value (`0`).
    pub const INVALID: SceneNodeId = SceneNodeId(0);

    /// Construct from a raw `u64`.
    pub const fn from_raw(raw: u64) -> Self {
        SceneNodeId(raw)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_is_zero() {
        assert_eq!(SceneNodeId::INVALID.raw(), 0);
        assert!(!SceneNodeId::INVALID.is_valid());
    }

    #[test]
    fn non_zero_is_valid() {
        assert!(SceneNodeId::from_raw(1).is_valid());
        assert!(SceneNodeId::from_raw(u64::MAX).is_valid());
    }

    #[test]
    fn equality_is_by_raw_value() {
        let a = SceneNodeId::from_raw(7);
        let b = SceneNodeId::from_raw(7);
        let c = SceneNodeId::from_raw(8);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn ordering_is_numeric() {
        let a = SceneNodeId::from_raw(1);
        let b = SceneNodeId::from_raw(2);
        assert!(a < b);
        let mut ids = vec![
            SceneNodeId::from_raw(3),
            SceneNodeId::from_raw(1),
            SceneNodeId::from_raw(2),
        ];
        ids.sort();
        assert_eq!(
            ids,
            vec![
                SceneNodeId::from_raw(1),
                SceneNodeId::from_raw(2),
                SceneNodeId::from_raw(3),
            ]
        );
    }

    #[test]
    fn copy_is_by_value() {
        let a = SceneNodeId::from_raw(5);
        let b = a;
        assert_eq!(a, b);
    }
}
