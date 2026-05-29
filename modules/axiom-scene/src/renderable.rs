//! Renderable scene reference: opaque mesh + material id pair on a node.

use crate::material_ref::MaterialRef;
use crate::mesh_ref::MeshRef;
use crate::scene_error::SceneError;
use crate::scene_node_id::SceneNodeId;
use crate::scene_result::SceneResult;

/// A renderable reference attached to a scene node.
///
/// The scene module does **not** own meshes or materials; a renderable
/// is a triple of opaque refs (`mesh`, `material`) and an attachment
/// node, plus a visibility flag a debug overlay / culling layer / app
/// can toggle. A future resource/render module (or app composition
/// layer) is responsible for resolving the refs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Renderable {
    node: SceneNodeId,
    mesh: MeshRef,
    material: MaterialRef,
    visible: bool,
}

impl Renderable {
    /// Build a renderable, rejecting an invalid mesh or material ref.
    pub fn new(
        node: SceneNodeId,
        mesh: MeshRef,
        material: MaterialRef,
    ) -> SceneResult<Self> {
        if !mesh.is_valid() {
            return Err(SceneError::invalid_renderable_reference(
                "renderable mesh ref was the invalid sentinel",
            ));
        }
        if !material.is_valid() {
            return Err(SceneError::invalid_renderable_reference(
                "renderable material ref was the invalid sentinel",
            ));
        }
        Ok(Renderable {
            node,
            mesh,
            material,
            visible: true,
        })
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

    pub(crate) fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene_error_code::SceneErrorCode;

    #[test]
    fn renderable_is_built_with_valid_refs() {
        let r = Renderable::new(
            SceneNodeId::from_raw(1),
            MeshRef::from_raw(2),
            MaterialRef::from_raw(3),
        )
        .unwrap();
        assert_eq!(r.node().raw(), 1);
        assert_eq!(r.mesh().raw(), 2);
        assert_eq!(r.material().raw(), 3);
        assert!(r.visible());
    }

    #[test]
    fn zero_mesh_ref_is_rejected() {
        let err = Renderable::new(
            SceneNodeId::from_raw(1),
            MeshRef::INVALID,
            MaterialRef::from_raw(3),
        )
        .unwrap_err();
        assert_eq!(err.code(), SceneErrorCode::InvalidRenderableReference);
    }

    #[test]
    fn zero_material_ref_is_rejected() {
        let err = Renderable::new(
            SceneNodeId::from_raw(1),
            MeshRef::from_raw(2),
            MaterialRef::INVALID,
        )
        .unwrap_err();
        assert_eq!(err.code(), SceneErrorCode::InvalidRenderableReference);
    }

    #[test]
    fn set_visibility_round_trips() {
        let mut r = Renderable::new(
            SceneNodeId::from_raw(1),
            MeshRef::from_raw(2),
            MaterialRef::from_raw(3),
        )
        .unwrap();
        assert!(r.visible());
        r.set_visible(false);
        assert!(!r.visible());
        r.set_visible(true);
        assert!(r.visible());
    }

    #[test]
    fn equal_renderables_compare_equal() {
        let a = Renderable::new(
            SceneNodeId::from_raw(1),
            MeshRef::from_raw(2),
            MaterialRef::from_raw(3),
        )
        .unwrap();
        let b = Renderable::new(
            SceneNodeId::from_raw(1),
            MeshRef::from_raw(2),
            MaterialRef::from_raw(3),
        )
        .unwrap();
        assert_eq!(a, b);
    }
}
