//! Stable identity for a camera component.

/// A stable, opaque identifier for a [`crate::Camera`] component owned by
/// a scene.
///
/// Like [`crate::SceneNodeId`], ids are monotonically assigned at
/// creation time and never reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CameraId(u64);

impl CameraId {
    pub const INVALID: CameraId = CameraId(0);

    pub const fn from_raw(raw: u64) -> Self {
        CameraId(raw)
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
        assert_eq!(CameraId::INVALID.raw(), 0);
        assert!(!CameraId::INVALID.is_valid());
    }

    #[test]
    fn non_zero_is_valid() {
        assert!(CameraId::from_raw(1).is_valid());
    }

    #[test]
    fn equality_is_by_raw_value() {
        assert_eq!(CameraId::from_raw(3), CameraId::from_raw(3));
        assert_ne!(CameraId::from_raw(3), CameraId::from_raw(4));
    }

    #[test]
    fn ordering_is_numeric() {
        assert!(CameraId::from_raw(1) < CameraId::from_raw(2));
    }
}
