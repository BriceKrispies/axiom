//! Machine-readable scene-error code.

/// The reason a Layer-04-or-above scene operation failed.
///
/// Codes are scene-module identities; two errors with the same code
/// compare equal regardless of human message, so error checks stay
/// machine-stable across builds and replays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u16)]
pub enum SceneErrorCode {
    /// A scene operation referenced a node id that does not exist (or
    /// has been removed).
    MissingNode = 1,
    /// A camera operation referenced a camera id that does not exist.
    MissingCamera = 2,
    /// A light operation referenced a light id that does not exist.
    MissingLight = 3,
    /// A renderable operation referenced a renderable id that does not
    /// exist.
    MissingRenderable = 4,
    /// A `set_parent` call would make a node its own parent.
    SelfParenting = 5,
    /// A `set_parent` call would create a cycle in the hierarchy.
    HierarchyCycle = 6,
    /// A camera was constructed with invalid intrinsic parameters
    /// (`fovy`, `aspect`, `near`, `far`). The wrapped math layer error
    /// code is preserved on the [`crate::SceneError`].
    InvalidCameraParameters = 7,
    /// A light was constructed with a non-finite or non-positive
    /// intensity (or non-finite colour components).
    InvalidLightParameters = 8,
    /// A renderable was constructed with an invalid mesh or material
    /// reference (the invalid sentinel `0`).
    InvalidRenderableReference = 9,
}

impl SceneErrorCode {
    pub const fn raw(self) -> u16 {
        self as u16
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discriminants_are_stable() {
        assert_eq!(SceneErrorCode::MissingNode.raw(), 1);
        assert_eq!(SceneErrorCode::InvalidRenderableReference.raw(), 9);
    }

    #[test]
    fn codes_are_distinct_and_ordered() {
        assert_ne!(
            SceneErrorCode::SelfParenting,
            SceneErrorCode::HierarchyCycle
        );
        assert!(SceneErrorCode::MissingNode < SceneErrorCode::InvalidRenderableReference);
    }
}
