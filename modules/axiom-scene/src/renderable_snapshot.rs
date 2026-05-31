//! One renderable entry inside a [`crate::SceneSnapshot`].

use crate::material_ref::MaterialRef;
use crate::mesh_ref::MeshRef;
use crate::renderable_id::RenderableId;
use crate::scene_node_id::SceneNodeId;

/// One renderable entry in a deterministic scene snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RenderableSnapshot {
    id: RenderableId,
    node: SceneNodeId,
    mesh: MeshRef,
    material: MaterialRef,
    visible: bool,
}

impl RenderableSnapshot {
    pub const fn new(
        id: RenderableId,
        node: SceneNodeId,
        mesh: MeshRef,
        material: MaterialRef,
        visible: bool,
    ) -> Self {
        RenderableSnapshot {
            id,
            node,
            mesh,
            material,
            visible,
        }
    }

    pub const fn id(&self) -> RenderableId {
        self.id
    }

    pub const fn node(&self) -> SceneNodeId {
        self.node
    }

    pub const fn mesh(&self) -> MeshRef {
        self.mesh
    }

    pub const fn material(&self) -> MaterialRef {
        self.material
    }

    pub const fn visible(&self) -> bool {
        self.visible
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accessors_round_trip_constructed_values() {
        let s = RenderableSnapshot::new(
            RenderableId::from_raw(7),
            SceneNodeId::from_raw(3),
            MeshRef::from_raw(11),
            MaterialRef::from_raw(99),
            false,
        );
        assert_eq!(s.id().raw(), 7);
        assert_eq!(s.node().raw(), 3);
        assert_eq!(s.mesh().raw(), 11);
        assert_eq!(s.material().raw(), 99);
        assert!(!s.visible());
    }

    #[test]
    fn visible_true_is_reported_true() {
        // Kills `visible -> false`: a renderable constructed visible must
        // report true.
        let s = RenderableSnapshot::new(
            RenderableId::from_raw(1),
            SceneNodeId::from_raw(1),
            MeshRef::from_raw(1),
            MaterialRef::from_raw(1),
            true,
        );
        assert!(s.visible());
    }

    #[test]
    fn equality_requires_all_fields() {
        let a = RenderableSnapshot::new(
            RenderableId::from_raw(1),
            SceneNodeId::from_raw(1),
            MeshRef::from_raw(1),
            MaterialRef::from_raw(1),
            true,
        );
        let b = RenderableSnapshot::new(
            RenderableId::from_raw(1),
            SceneNodeId::from_raw(1),
            MeshRef::from_raw(1),
            MaterialRef::from_raw(1),
            true,
        );
        let c = RenderableSnapshot::new(
            RenderableId::from_raw(1),
            SceneNodeId::from_raw(1),
            MeshRef::from_raw(1),
            MaterialRef::from_raw(1),
            false,
        );
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
