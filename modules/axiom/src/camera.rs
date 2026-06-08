//! A camera component an app spawns onto a node.

use axiom_kernel::Meters;

use crate::angle::Angle;

/// Perspective camera intrinsics: vertical field of view plus near/far clip
/// planes. The aspect ratio is supplied by the engine from the window viewport,
/// so it is not part of the authored projection.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PerspectiveProjection {
    pub fov_y: Angle,
    pub near: Meters,
    pub far: Meters,
}

/// A camera component. Today the engine offers a perspective camera; the
/// authored projection is resolved against the viewport aspect when the app
/// runs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    projection: PerspectiveProjection,
}

impl Camera {
    /// A perspective camera with the given intrinsics.
    pub const fn perspective(projection: PerspectiveProjection) -> Self {
        Camera { projection }
    }

    /// The authored perspective intrinsics.
    pub const fn projection(self) -> PerspectiveProjection {
        self.projection
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perspective_carries_its_intrinsics() {
        let proj = PerspectiveProjection {
            fov_y: Angle::degrees(60.0),
            near: Meters::new(0.1).unwrap(),
            far: Meters::new(100.0).unwrap(),
        };
        let camera = Camera::perspective(proj);
        assert_eq!(camera.projection(), proj);
    }
}
