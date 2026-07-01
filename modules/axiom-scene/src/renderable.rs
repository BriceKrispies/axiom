//! Renderable scene reference: opaque mesh + material id pair, stored per node.

use axiom_kernel::{BinaryReader, BinaryWriter, FieldSchema, KernelResult, Reflect, TypeSchema};

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
    casts_contact_shadow: bool,
}

impl Renderable {
    pub const SCHEMA: TypeSchema = TypeSchema::new(
        "Renderable",
        &[
            FieldSchema::new("mesh", "u64"),
            FieldSchema::new("material", "u64"),
            FieldSchema::new("visible", "bool"),
            FieldSchema::new("casts_contact_shadow", "bool"),
        ],
    );

    /// Build a renderable, rejecting an invalid mesh or material ref. It is
    /// visible and (by default) *not* a contact-shadow caster — level geometry
    /// casts no grounding shadow; a discrete dynamic object opts in via
    /// [`Self::set_casts_contact_shadow`].
    pub fn new(mesh: MeshRef, material: MaterialRef) -> SceneResult<Self> {
        mesh.is_valid()
            .then_some(())
            .ok_or_else(|| {
                SceneError::invalid_renderable_reference(
                    "renderable mesh ref was the invalid sentinel",
                )
            })
            .and_then(|()| {
                material.is_valid().then_some(()).ok_or_else(|| {
                    SceneError::invalid_renderable_reference(
                        "renderable material ref was the invalid sentinel",
                    )
                })
            })
            .map(|()| Renderable {
                mesh,
                material,
                visible: true,
                casts_contact_shadow: false,
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

    /// Whether this renderable is a discrete, dynamic object that grounds itself
    /// with a contact shadow (level geometry stays `false`).
    pub const fn casts_contact_shadow(&self) -> bool {
        self.casts_contact_shadow
    }

    pub(crate) fn set_casts_contact_shadow(&mut self, casts: bool) {
        self.casts_contact_shadow = casts;
    }
}

impl Reflect for Renderable {
    const SCHEMA: TypeSchema = Renderable::SCHEMA;

    fn reflect_write(&self, writer: &mut BinaryWriter) {
        self.mesh.reflect_write(writer);
        self.material.reflect_write(writer);
        self.visible.reflect_write(writer);
        self.casts_contact_shadow.reflect_write(writer);
    }

    /// Reconstruct directly (bypassing `new`'s ref-validation): a snapshot is the
    /// engine's own bytes, restored as-stored.
    fn reflect_read(reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
        MeshRef::reflect_read(reader).and_then(|mesh| {
            MaterialRef::reflect_read(reader).and_then(|material| {
                bool::reflect_read(reader).and_then(|visible| {
                    bool::reflect_read(reader).map(|casts_contact_shadow| Renderable {
                        mesh,
                        material,
                        visible,
                        casts_contact_shadow,
                    })
                })
            })
        })
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
        assert!(!r.casts_contact_shadow());
    }

    #[test]
    fn set_casts_contact_shadow_round_trips() {
        let mut r = Renderable::new(MeshRef::from_raw(2), MaterialRef::from_raw(3)).unwrap();
        assert!(!r.casts_contact_shadow());
        r.set_casts_contact_shadow(true);
        assert!(r.casts_contact_shadow());
        r.set_casts_contact_shadow(false);
        assert!(!r.casts_contact_shadow());
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
        assert_eq!(Renderable::SCHEMA.fields().len(), 4);
        assert_eq!(Renderable::SCHEMA.fields()[2].name(), "visible");
        assert_eq!(
            Renderable::SCHEMA.fields()[3].name(),
            "casts_contact_shadow"
        );
    }

    #[test]
    fn reflect_round_trips_visibility_caster_and_refs_and_rejects_truncation() {
        let mut r = Renderable::new(MeshRef::from_raw(7), MaterialRef::from_raw(9)).unwrap();
        r.set_visible(false);
        r.set_casts_contact_shadow(true);
        let mut w = BinaryWriter::new();
        r.reflect_write(&mut w);
        let got = Renderable::reflect_read(&mut BinaryReader::new(&w.into_bytes())).unwrap();
        assert_eq!(got, r);
        assert!(!got.visible());
        assert!(got.casts_contact_shadow());
        assert!(Renderable::reflect_read(&mut BinaryReader::new(&[])).is_err());
    }
}
