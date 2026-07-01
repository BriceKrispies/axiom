//! One renderable entry inside a [`crate::SceneSnapshot`].

use crate::material_ref::MaterialRef;
use crate::mesh_ref::MeshRef;
use crate::scene_node_id::SceneNodeId;

/// One renderable entry in a deterministic scene snapshot, keyed by its node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RenderableSnapshot {
    node: SceneNodeId,
    mesh: MeshRef,
    material: MaterialRef,
    visible: bool,
    casts_contact_shadow: bool,
}

impl RenderableSnapshot {
    pub const fn new(
        node: SceneNodeId,
        mesh: MeshRef,
        material: MaterialRef,
        visible: bool,
        casts_contact_shadow: bool,
    ) -> Self {
        RenderableSnapshot {
            node,
            mesh,
            material,
            visible,
            casts_contact_shadow,
        }
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

    /// Whether this renderable is a discrete dynamic object that grounds itself
    /// with a contact shadow (level geometry is `false`).
    pub const fn casts_contact_shadow(&self) -> bool {
        self.casts_contact_shadow
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accessors_round_trip_constructed_values() {
        let s = RenderableSnapshot::new(
            SceneNodeId::from_raw(3),
            MeshRef::from_raw(11),
            MaterialRef::from_raw(99),
            false,
            true,
        );
        assert_eq!(s.node().raw(), 3);
        assert_eq!(s.mesh().raw(), 11);
        assert_eq!(s.material().raw(), 99);
        assert!(!s.visible());
        assert!(s.casts_contact_shadow());
    }

    #[test]
    fn visible_true_caster_false_is_reported() {
        let s = RenderableSnapshot::new(
            SceneNodeId::from_raw(1),
            MeshRef::from_raw(1),
            MaterialRef::from_raw(1),
            true,
            false,
        );
        assert!(s.visible());
        assert!(!s.casts_contact_shadow());
    }

    #[test]
    fn equality_requires_all_fields() {
        let a = RenderableSnapshot::new(
            SceneNodeId::from_raw(1),
            MeshRef::from_raw(1),
            MaterialRef::from_raw(1),
            true,
            false,
        );
        let b = RenderableSnapshot::new(
            SceneNodeId::from_raw(1),
            MeshRef::from_raw(1),
            MaterialRef::from_raw(1),
            true,
            false,
        );
        let c = RenderableSnapshot::new(
            SceneNodeId::from_raw(1),
            MeshRef::from_raw(1),
            MaterialRef::from_raw(1),
            false,
            false,
        );
        let d = RenderableSnapshot::new(
            SceneNodeId::from_raw(1),
            MeshRef::from_raw(1),
            MaterialRef::from_raw(1),
            true,
            true,
        );
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, d);
    }
}
