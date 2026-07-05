//! One bounds entry inside a [`crate::SceneSnapshot`].

use axiom_math::Vec3;

use crate::scene_node_id::SceneNodeId;

/// One axis-aligned bounding-volume entry in a deterministic scene snapshot,
/// keyed by its node. Rolling `Bounds` into the snapshot carries each object's
/// collision/query proxy (its local `half_extents`) alongside its render binding,
/// so a consumer sees the object's spatial extent without a second query pass.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoundsSnapshot {
    node: SceneNodeId,
    half_extents: Vec3,
}

impl BoundsSnapshot {
    pub const fn new(node: SceneNodeId, half_extents: Vec3) -> Self {
        BoundsSnapshot { node, half_extents }
    }

    pub const fn node(&self) -> SceneNodeId {
        self.node
    }

    /// The bounding box half-extents in the node's local unit frame.
    pub const fn half_extents(&self) -> Vec3 {
        self.half_extents
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accessors_round_trip() {
        let b = BoundsSnapshot::new(SceneNodeId::from_raw(4), Vec3::new(0.5, 1.0, 2.0));
        assert_eq!(b.node().raw(), 4);
        assert_eq!(b.half_extents(), Vec3::new(0.5, 1.0, 2.0));
    }

    #[test]
    fn equality_requires_all_fields() {
        let a = BoundsSnapshot::new(SceneNodeId::from_raw(1), Vec3::ONE);
        let b = BoundsSnapshot::new(SceneNodeId::from_raw(1), Vec3::ONE);
        let c = BoundsSnapshot::new(SceneNodeId::from_raw(1), Vec3::ZERO);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
