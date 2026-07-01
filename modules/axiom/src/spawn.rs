//! `Spawn`: a runtime spawn request — the components to create a node mid-game.
//!
//! The runtime counterpart to authoring a node in `setup`: the engine owns object
//! lifetime, so a game adds objects mid-play (a frozen ghost, a re-spawned enemy)
//! through [`crate::prelude::RunningApp::spawn`] instead of pre-allocating them.

use axiom_math::{Transform, Vec3};

use crate::handle::Handle;
use crate::material::Material;
use crate::mesh::Mesh;

/// A request to create a node at runtime: a renderable (by mesh + material
/// [`Handle`]) at a transform, optionally marked with a player index (for the
/// per-tick move + addressing) and given bounds (so spatial queries can hit it).
/// The handles are the ones the `setup` closure produced via
/// [`crate::prelude::Assets::add`] — an app holds them and spawns against them, so
/// a runtime spawn reuses an already-registered mesh/material.
#[derive(Debug, Clone, Copy)]
pub struct Spawn {
    /// World transform of the new node.
    pub transform: Transform,
    /// The renderable's mesh.
    pub mesh: Handle<Mesh>,
    /// The renderable's material.
    pub material: Handle<Material>,
    /// Mark the node with this player index, if any.
    pub player: Option<u32>,
    /// Attach axis-aligned bounds of these local half-extents, if any.
    pub bounds: Option<Vec3>,
    /// Whether the renderable grounds itself with a contact shadow.
    pub casts_contact_shadow: bool,
}

impl Spawn {
    /// A renderable spawn of `mesh` + `material` at `transform`: no player mark,
    /// no bounds, no contact shadow. Chain the `with_*` setters to add them.
    pub fn new(transform: Transform, mesh: Handle<Mesh>, material: Handle<Material>) -> Self {
        Spawn {
            transform,
            mesh,
            material,
            player: None,
            bounds: None,
            casts_contact_shadow: false,
        }
    }

    /// Mark the spawned node with `index` (so the per-tick move and despawn can
    /// address it; also how a game-side classifier recognizes it).
    pub fn with_player(mut self, index: u32) -> Self {
        self.player = Some(index);
        self
    }

    /// Give the spawned node an axis-aligned bounds of `half_extents`.
    pub fn with_bounds(mut self, half_extents: Vec3) -> Self {
        self.bounds = Some(half_extents);
        self
    }

    /// Mark the spawned renderable as a contact-shadow caster.
    pub fn casts_contact_shadow(mut self) -> Self {
        self.casts_contact_shadow = true;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::Assets;

    #[test]
    fn builder_sets_each_field() {
        let mut meshes: Assets<Mesh> = Assets::new();
        let mut materials: Assets<Material> = Assets::new();
        let mesh = meshes.add(Mesh::cube());
        let material = materials.add(Material::lit(crate::color::Color::WHITE));

        let bare = Spawn::new(Transform::IDENTITY, mesh, material);
        assert_eq!(
            (bare.mesh.id(), bare.material.id()),
            (mesh.id(), material.id())
        );
        assert_eq!(bare.player, None);
        assert_eq!(bare.bounds, None);
        assert!(!bare.casts_contact_shadow);

        let full = Spawn::new(Transform::IDENTITY, mesh, material)
            .with_player(7)
            .with_bounds(Vec3::new(0.5, 0.5, 0.5))
            .casts_contact_shadow();
        assert_eq!(full.player, Some(7));
        assert_eq!(full.bounds, Some(Vec3::new(0.5, 0.5, 0.5)));
        assert!(full.casts_contact_shadow);
    }
}
