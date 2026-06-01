//! Renderable scene reference: opaque mesh + material id pair, stored per node.

use axiom_kernel::{FieldSchema, TypeSchema};

use crate::material_ref::MaterialRef;
use crate::mesh_ref::MeshRef;
use crate::scene_error::SceneError;
use crate::scene_result::SceneResult;

/// A renderable component, stored on the node entity it belongs to.
///
/// The scene module does **not** own meshes or materials; a renderable is a
/// pair of opaque refs (`mesh`, `material`) plus a visibility flag a debug
/// overlay / culling layer / app can toggle. The node it is attached to is the
/// entity this component is keyed by. A future resource/render module (or app
/// composition layer) is responsible for resolving the refs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Renderable {
    mesh: MeshRef,
    material: MaterialRef,
    visible: bool,
}

impl Renderable {
    /// The reflected shape of a renderable component.
    pub const SCHEMA: TypeSchema = TypeSchema::new(
        "Renderable",
        &[
            FieldSchema::new("mesh", "u64"),
            FieldSchema::new("material", "u64"),
            FieldSchema::new("visible", "bool"),
        ],
    );

    /// Build a renderable, rejecting an invalid mesh or material ref.
    pub fn new(mesh: MeshRef, material: MaterialRef) -> SceneResult<Self> {
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
            mesh,
            material,
            visible: true,
        })
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
        let r = Renderable::new(MeshRef::from_raw(2), MaterialRef::from_raw(3)).unwrap();
        assert_eq!(r.mesh().raw(), 2);
        assert_eq!(r.material().raw(), 3);
        assert!(r.visible());
    }

    #[test]
    fn zero_mesh_ref_is_rejected() {
        let err = Renderable::new(MeshRef::INVALID, MaterialRef::from_raw(3)).unwrap_err();
        assert_eq!(err.code(), SceneErrorCode::InvalidRenderableReference);
    }

    #[test]
    fn zero_material_ref_is_rejected() {
        let err = Renderable::new(MeshRef::from_raw(2), MaterialRef::INVALID).unwrap_err();
        assert_eq!(err.code(), SceneErrorCode::InvalidRenderableReference);
    }

    #[test]
    fn set_visibility_round_trips() {
        let mut r = Renderable::new(MeshRef::from_raw(2), MaterialRef::from_raw(3)).unwrap();
        assert!(r.visible());
        r.set_visible(false);
        assert!(!r.visible());
        r.set_visible(true);
        assert!(r.visible());
    }

    #[test]
    fn equal_renderables_compare_equal() {
        let a = Renderable::new(MeshRef::from_raw(2), MaterialRef::from_raw(3)).unwrap();
        let b = Renderable::new(MeshRef::from_raw(2), MaterialRef::from_raw(3)).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn schema_names_the_renderable_fields() {
        assert_eq!(Renderable::SCHEMA.name(), "Renderable");
        assert_eq!(Renderable::SCHEMA.fields().len(), 3);
        assert_eq!(Renderable::SCHEMA.fields()[2].name(), "visible");
    }
}
