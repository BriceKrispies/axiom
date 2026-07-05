//! Renderable scene reference: the object-binding component stored per node.

use axiom_kernel::{BinaryReader, BinaryWriter, FieldSchema, KernelResult, Reflect, TypeSchema};

use crate::animation_ref::AnimationRef;
use crate::material_ref::MaterialRef;
use crate::mesh_ref::MeshRef;
use crate::scene_error::SceneError;
use crate::scene_result::SceneResult;
use crate::texture_ref::TextureRef;

/// A renderable component, stored on the node entity it belongs to.
///
/// This is the engine's **object-binding contract**: paired with the node it is
/// keyed by (which supplies the object's identity + transform), it binds the
/// full visual identity of a game thing in one place — a `mesh`, a `material`, an
/// optional albedo `texture`, an optional `animation` binding (the posed
/// articulated figure / clip that drives it), plus the visibility and
/// contact-shadow flags a culling layer / debug overlay toggles. The scene
/// module owns none of the referenced resources; each ref is an opaque handle a
/// resource/render module (or the app composition layer) resolves. `texture` and
/// `animation` default to their `INVALID` sentinel (untextured, static) so a
/// plain renderable is unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Renderable {
    mesh: MeshRef,
    material: MaterialRef,
    texture: TextureRef,
    animation: AnimationRef,
    visible: bool,
    casts_contact_shadow: bool,
}

impl Renderable {
    pub const SCHEMA: TypeSchema = TypeSchema::new(
        "Renderable",
        &[
            FieldSchema::new("mesh", "u64"),
            FieldSchema::new("material", "u64"),
            FieldSchema::new("texture", "u64"),
            FieldSchema::new("animation", "u64"),
            FieldSchema::new("visible", "bool"),
            FieldSchema::new("casts_contact_shadow", "bool"),
        ],
    );

    /// Build a renderable, rejecting an invalid mesh or material ref. It is
    /// visible, untextured, un-animated, and (by default) *not* a contact-shadow
    /// caster — level geometry casts no grounding shadow; a discrete dynamic
    /// object opts in via [`Self::set_casts_contact_shadow`], binds a texture via
    /// [`Self::set_texture`], and binds an animation via [`Self::set_animation`].
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
                texture: TextureRef::INVALID,
                animation: AnimationRef::INVALID,
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

    /// The albedo texture bound to this object (`INVALID` = untextured).
    pub const fn texture(&self) -> TextureRef {
        self.texture
    }

    /// The animation binding driving this object (`INVALID` = static).
    pub const fn animation(&self) -> AnimationRef {
        self.animation
    }

    pub const fn visible(&self) -> bool {
        self.visible
    }

    pub(crate) fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    pub(crate) fn set_texture(&mut self, texture: TextureRef) {
        self.texture = texture;
    }

    pub(crate) fn set_animation(&mut self, animation: AnimationRef) {
        self.animation = animation;
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
        self.texture.reflect_write(writer);
        self.animation.reflect_write(writer);
        self.visible.reflect_write(writer);
        self.casts_contact_shadow.reflect_write(writer);
    }

    /// Reconstruct directly (bypassing `new`'s ref-validation): a snapshot is the
    /// engine's own bytes, restored as-stored.
    fn reflect_read(reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
        MeshRef::reflect_read(reader).and_then(|mesh| {
            MaterialRef::reflect_read(reader).and_then(|material| {
                TextureRef::reflect_read(reader).and_then(|texture| {
                    AnimationRef::reflect_read(reader).and_then(|animation| {
                        bool::reflect_read(reader).and_then(|visible| {
                            bool::reflect_read(reader).map(|casts_contact_shadow| Renderable {
                                mesh,
                                material,
                                texture,
                                animation,
                                visible,
                                casts_contact_shadow,
                            })
                        })
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
        assert!(!r.texture().is_valid());
        assert!(!r.animation().is_valid());
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
    fn set_texture_and_animation_round_trip() {
        let mut r = Renderable::new(MeshRef::from_raw(2), MaterialRef::from_raw(3)).unwrap();
        r.set_texture(TextureRef::from_raw(9));
        r.set_animation(AnimationRef::from_raw(11));
        assert_eq!(r.texture(), TextureRef::from_raw(9));
        assert_eq!(r.animation(), AnimationRef::from_raw(11));
        assert!(r.texture().is_valid());
        assert!(r.animation().is_valid());
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
        assert_eq!(Renderable::SCHEMA.fields().len(), 6);
        assert_eq!(Renderable::SCHEMA.fields()[2].name(), "texture");
        assert_eq!(Renderable::SCHEMA.fields()[3].name(), "animation");
        assert_eq!(Renderable::SCHEMA.fields()[4].name(), "visible");
        assert_eq!(
            Renderable::SCHEMA.fields()[5].name(),
            "casts_contact_shadow"
        );
    }

    #[test]
    fn reflect_round_trips_all_fields_and_rejects_truncation() {
        let mut r = Renderable::new(MeshRef::from_raw(7), MaterialRef::from_raw(9)).unwrap();
        r.set_visible(false);
        r.set_casts_contact_shadow(true);
        r.set_texture(TextureRef::from_raw(13));
        r.set_animation(AnimationRef::from_raw(21));
        let mut w = BinaryWriter::new();
        r.reflect_write(&mut w);
        let got = Renderable::reflect_read(&mut BinaryReader::new(&w.into_bytes())).unwrap();
        assert_eq!(got, r);
        assert!(!got.visible());
        assert!(got.casts_contact_shadow());
        assert_eq!(got.texture(), TextureRef::from_raw(13));
        assert_eq!(got.animation(), AnimationRef::from_raw(21));
        assert!(Renderable::reflect_read(&mut BinaryReader::new(&[])).is_err());
    }
}
