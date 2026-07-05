//! One renderable entry inside a [`crate::SceneSnapshot`].

use crate::animation_ref::AnimationRef;
use crate::material_ref::MaterialRef;
use crate::mesh_ref::MeshRef;
use crate::scene_node_id::SceneNodeId;
use crate::texture_ref::TextureRef;

/// One renderable entry in a deterministic scene snapshot, keyed by its node.
///
/// Carries the full object binding — mesh, material, the optional albedo
/// `texture` and `animation` refs, and the visibility / contact-shadow flags —
/// so a consumer sees one coherent object rather than a mesh+material pair with
/// its texture and animation living in side tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RenderableSnapshot {
    node: SceneNodeId,
    mesh: MeshRef,
    material: MaterialRef,
    texture: TextureRef,
    animation: AnimationRef,
    visible: bool,
    casts_contact_shadow: bool,
}

impl RenderableSnapshot {
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        node: SceneNodeId,
        mesh: MeshRef,
        material: MaterialRef,
        texture: TextureRef,
        animation: AnimationRef,
        visible: bool,
        casts_contact_shadow: bool,
    ) -> Self {
        RenderableSnapshot {
            node,
            mesh,
            material,
            texture,
            animation,
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

    /// Whether this renderable is a discrete dynamic object that grounds itself
    /// with a contact shadow (level geometry is `false`).
    pub const fn casts_contact_shadow(&self) -> bool {
        self.casts_contact_shadow
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> RenderableSnapshot {
        RenderableSnapshot::new(
            SceneNodeId::from_raw(3),
            MeshRef::from_raw(11),
            MaterialRef::from_raw(99),
            TextureRef::from_raw(7),
            AnimationRef::from_raw(5),
            false,
            true,
        )
    }

    #[test]
    fn accessors_round_trip_constructed_values() {
        let s = sample();
        assert_eq!(s.node().raw(), 3);
        assert_eq!(s.mesh().raw(), 11);
        assert_eq!(s.material().raw(), 99);
        assert_eq!(s.texture(), TextureRef::from_raw(7));
        assert_eq!(s.animation(), AnimationRef::from_raw(5));
        assert!(!s.visible());
        assert!(s.casts_contact_shadow());
    }

    #[test]
    fn visible_true_caster_false_is_reported() {
        let s = RenderableSnapshot::new(
            SceneNodeId::from_raw(1),
            MeshRef::from_raw(1),
            MaterialRef::from_raw(1),
            TextureRef::INVALID,
            AnimationRef::INVALID,
            true,
            false,
        );
        assert!(s.visible());
        assert!(!s.casts_contact_shadow());
        assert!(!s.texture().is_valid());
        assert!(!s.animation().is_valid());
    }

    #[test]
    fn equality_requires_all_fields() {
        let a = sample();
        let b = sample();
        let mut differs_texture = a;
        differs_texture.texture = TextureRef::from_raw(8);
        let mut differs_animation = a;
        differs_animation.animation = AnimationRef::from_raw(6);
        assert_eq!(a, b);
        assert_ne!(a, differs_texture);
        assert_ne!(a, differs_animation);
    }
}
