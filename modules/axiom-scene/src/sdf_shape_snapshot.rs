//! One SDF-shape entry inside a [`crate::SceneSnapshot`].

use axiom_math::Vec3;

use crate::scene_node_id::SceneNodeId;

/// One raymarched SDF shape in a deterministic scene snapshot, keyed by its node.
///
/// Carries the shape's kind, local dimensions, and colour; the shape's world
/// placement is the node's world transform, looked up by `node` (the same way a
/// [`crate::renderable_snapshot::RenderableSnapshot`] is placed). A consumer
/// translates this — together with the node transform — into the backend-neutral
/// SDF primitive the render backends march.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SdfShapeSnapshot {
    node: SceneNodeId,
    kind: u32,
    dims: Vec3,
    color: Vec3,
}

impl SdfShapeSnapshot {
    /// An entry from its `node`, `kind` discriminant, local `dims`, and `color`.
    pub const fn new(node: SceneNodeId, kind: u32, dims: Vec3, color: Vec3) -> Self {
        SdfShapeSnapshot {
            node,
            kind,
            dims,
            color,
        }
    }

    /// The node this shape is attached to (its world transform places the shape).
    pub const fn node(&self) -> SceneNodeId {
        self.node
    }

    /// The kind discriminant (sphere / box / plane).
    pub const fn kind(&self) -> u32 {
        self.kind
    }

    /// The local dimensions (sphere radius in `x`; box half-extents; plane unused).
    pub const fn dims(&self) -> Vec3 {
        self.dims
    }

    /// The linear-RGB surface colour.
    pub const fn color(&self) -> Vec3 {
        self.color
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node() -> SceneNodeId {
        SceneNodeId::from_raw(7)
    }

    #[test]
    fn accessors_round_trip_constructed_values() {
        // kind `1` is the box discriminant.
        let s = SdfShapeSnapshot::new(node(), 1, Vec3::new(1.0, 2.0, 3.0), Vec3::ONE);
        assert_eq!(s.node(), node());
        assert_eq!(s.kind(), 1);
        assert_eq!(s.dims(), Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(s.color(), Vec3::ONE);
    }

    #[test]
    fn equality_requires_all_fields() {
        let base = SdfShapeSnapshot::new(node(), 0, Vec3::ONE, Vec3::ONE);
        assert_eq!(base, SdfShapeSnapshot::new(node(), 0, Vec3::ONE, Vec3::ONE));
        assert_ne!(
            base,
            SdfShapeSnapshot::new(SceneNodeId::from_raw(8), 0, Vec3::ONE, Vec3::ONE)
        );
        assert_ne!(base, SdfShapeSnapshot::new(node(), 2, Vec3::ONE, Vec3::ONE));
        assert_ne!(base, SdfShapeSnapshot::new(node(), 0, Vec3::ZERO, Vec3::ONE));
        assert_ne!(base, SdfShapeSnapshot::new(node(), 0, Vec3::ONE, Vec3::ZERO));
        assert!(format!("{base:?}").contains("SdfShapeSnapshot"));
    }
}
