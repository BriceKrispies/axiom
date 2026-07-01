//! `Bounds`: an axis-aligned bounding volume an app attaches to a node so it
//! answers the spatial queries ([`crate::prelude::RunningApp::raycast`] /
//! [`crate::prelude::RunningApp::overlap_box`]).
//!
//! An authoring value type, like [`crate::prelude::Spin`]: spawned in a bundle
//! and realized into the scene's bounds component. It is a *spatial-query*
//! volume (picking / overlap / line-of-sight), not a physics collider.

use axiom_math::Vec3;

/// An axis-aligned bounding box, given as half-extents in the node's local
/// frame. The scene sizes it by the node's world scale at query time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    /// Half the box's size along each local axis.
    pub half_extents: Vec3,
}

impl Bounds {
    /// A bounding box with the given local half-extents.
    pub const fn new(half_extents: Vec3) -> Self {
        Bounds { half_extents }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_stores_half_extents_and_is_copy_debug() {
        let b = Bounds::new(Vec3::new(0.5, 1.0, 2.0));
        let c = b;
        assert_eq!(b, c);
        assert_eq!(c.half_extents, Vec3::new(0.5, 1.0, 2.0));
        assert!(format!("{b:?}").contains("Bounds"));
    }
}
