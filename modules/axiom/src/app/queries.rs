//! The engine's Entity-addressed world surface on [`RunningApp`] — spatial
//! queries (Category 2) and runtime lifecycle (Category 3) of the game
//! vocabulary (`docs/game-vocabulary.md`). The engine answers "what is where" and
//! owns object lifetime, returning first-class [`Entity`] handles an app holds.
//! A child module of `app` so it reaches `RunningApp`'s private scene while
//! keeping `app.rs` within the per-file size budget.

use axiom_kernel::Meters;
use axiom_math::Vec3;
use axiom_scene::SceneNodeId as Entity;

use super::RunningApp;
use crate::spawn::Spawn;

impl RunningApp {
    /// Cast a ray from `origin` along `direction` and return the [`Entity`] of the
    /// nearest bounded node it enters within `max_distance` (or `None`). The single
    /// primitive behind hitscan, line-of-sight, and picking — the nearest hit *is*
    /// the blocker, so a wall in front of an actor correctly shadows it. The caller
    /// classifies the returned entity (e.g. "is it one of my enemies?").
    pub fn raycast(&self, origin: Vec3, direction: Vec3, max_distance: Meters) -> Option<Entity> {
        self.scene.raycast(origin, direction, max_distance)
    }

    /// Every bounded node whose world box overlaps the query box (centered at
    /// `center`, of `half_extents`), as [`Entity`] handles in ascending order. The
    /// single primitive behind collision tests and proximity/contact checks.
    pub fn overlap_box(&self, center: Vec3, half_extents: Vec3) -> Vec<Entity> {
        self.scene.overlap_box(center, half_extents)
    }

    /// Cast a ray and return the nearest bounded node **with the world-space entry
    /// point** on its box (or `None`) — [`Self::raycast`] plus the exact hit point,
    /// whose distance from `origin` a perceiving agent reads as "how far is the
    /// thing in front of me".
    pub fn raycast_hit(
        &self,
        origin: Vec3,
        direction: Vec3,
        max_distance: Meters,
    ) -> Option<(Entity, Vec3)> {
        self.scene.raycast_hit(origin, direction, max_distance)
    }

    /// Attach (or replace) a coarse semantic kind on `entity` — the engine-native
    /// "what is this thing" a perceiving agent reads off a hit, whose code
    /// vocabulary the game owns. Returns whether `entity` named a live node.
    pub fn tag(&mut self, entity: Entity, kind_code: u32) -> bool {
        self.scene.add_tag(entity, kind_code).is_ok()
    }

    /// The coarse kind code tagged on `entity`, if any — classifies a raycast /
    /// overlap hit (an untagged hit is plain geometry).
    pub fn tag_of(&self, entity: Entity) -> Option<u32> {
        self.scene.tag_of(entity)
    }

    /// Despawn `entity` — Category 3 (lifecycle). The engine owns object lifetime,
    /// so a game removes a killed actor for real instead of faking it (parking a
    /// corpse off-screen). Returns whether `entity` named a live node; despawning
    /// an absent/already-removed handle is a clean `false`.
    pub fn despawn(&mut self, entity: Entity) -> bool {
        self.scene.despawn_node(entity)
    }

    /// The [`Entity`] of the node marked with `player` index, if any — how an app
    /// that authored actors by index in `setup` recovers their handles to address
    /// them (despawn, query) afterward.
    pub fn player_entity(&self, player: u32) -> Option<Entity> {
        self.scene.player_entity(player)
    }

    /// The authoritative world-space translation of the node marked with `player`
    /// index, if any. A read-only projection of the simulation state — an
    /// authoritative headless host reads it to broadcast a renderable view to
    /// clients without keeping a parallel position mirror that could diverge from
    /// the engine.
    pub fn player_translation(&self, player: u32) -> Option<Vec3> {
        self.scene.player_translation(player)
    }

    /// Create a node at runtime from a [`Spawn`] request, returning its [`Entity`]
    /// — the runtime counterpart to authoring in `setup`, so a game adds objects
    /// mid-play (a frozen ghost, a re-spawned enemy). Attaches the renderable
    /// (by the spec's mesh/material handles), then the optional player mark,
    /// bounds, and contact-shadow flag, and propagates world transforms so the new
    /// node is immediately queryable.
    pub fn spawn(&mut self, spec: Spawn) -> Entity {
        let node = self.scene.create_node_with_transform(spec.transform);
        let mesh = self.scene.mesh_ref(spec.mesh.id());
        let material = self.scene.material_ref(spec.material.id());
        let _ = self.scene.add_renderable(node, mesh, material);
        spec.player.into_iter().for_each(|index| {
            let _ = self.scene.add_player(node, index);
        });
        spec.bounds.into_iter().for_each(|half_extents| {
            let _ = self.scene.add_bounds(node, half_extents);
        });
        spec.casts_contact_shadow.then(|| {
            self.scene
                .set_renderable_casts_contact_shadow(node, true)
                .ok()
        });
        self.scene.update_world_transforms();
        node
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use crate::bounds::Bounds;
    use crate::color::Color;
    use crate::contact_shadow_caster::ContactShadowCaster;
    use crate::default_plugins::DefaultPlugins;
    use crate::handle::Handle;
    use crate::material::Material;
    use crate::mesh::Mesh;
    use crate::player::{Player, PlayerInput};
    use crate::renderable::Renderable;
    use crate::spawn::Spawn;
    use crate::window::Window;
    use axiom_math::Transform;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[test]
    fn raycast_and_overlap_return_entities_the_caller_classifies() {
        let app = App::new()
            .window(Window::new(64, 64))
            .add_plugins(DefaultPlugins)
            .setup(|world, meshes, materials| {
                let cube = meshes.add(Mesh::cube());
                let material = materials.add(Material::lit(Color::WHITE));
                // A wall: bounded, no player marker.
                world.spawn((
                    Transform::from_translation(Vec3::new(3.0, 0.0, 0.0)),
                    Renderable {
                        mesh: cube,
                        material,
                    },
                    Bounds::new(Vec3::new(0.5, 0.5, 0.5)),
                ));
                // An enemy: player-marked (a 5-component bundle).
                world.spawn((
                    Transform::from_translation(Vec3::new(0.0, 0.0, -3.0)),
                    Renderable {
                        mesh: cube,
                        material,
                    },
                    Player::new(0),
                    ContactShadowCaster,
                    Bounds::new(Vec3::new(0.5, 0.5, 0.5)),
                ));
            })
            .build();
        let reach = Meters::new(100.0).unwrap();
        let enemy = app.player_entity(0).expect("enemy is marked player 0");
        // North hits the enemy entity; up hits nothing.
        assert_eq!(
            app.raycast(Vec3::ZERO, Vec3::new(0.0, 0.0, -1.0), reach),
            Some(enemy)
        );
        assert_eq!(
            app.raycast(Vec3::ZERO, Vec3::new(0.0, 1.0, 0.0), reach),
            None
        );
        // East hits the wall — some entity, but not the enemy (the caller's check).
        let wall = app.raycast(Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0), reach);
        assert!(wall.is_some());
        assert_ne!(wall, Some(enemy));
        // Overlap returns the same entities; the origin overlaps neither.
        assert_eq!(
            app.overlap_box(Vec3::new(0.0, 0.0, -3.0), Vec3::new(0.2, 0.2, 0.2)),
            vec![enemy]
        );
        let at_wall = app.overlap_box(Vec3::new(3.0, 0.0, 0.0), Vec3::new(0.2, 0.2, 0.2));
        assert_eq!(at_wall, vec![wall.unwrap()]);
        assert!(app
            .overlap_box(Vec3::ZERO, Vec3::new(0.2, 0.2, 0.2))
            .is_empty());
    }

    #[test]
    fn raycast_hit_and_tags_classify_what_the_agent_sees() {
        let mut app = App::new()
            .window(Window::new(64, 64))
            .add_plugins(DefaultPlugins)
            .setup(|world, meshes, materials| {
                let cube = meshes.add(Mesh::cube());
                let material = materials.add(Material::lit(Color::WHITE));
                // A wall (untagged geometry) east, an enemy (player 0) north.
                world.spawn((
                    Transform::from_translation(Vec3::new(3.0, 0.0, 0.0)),
                    Renderable { mesh: cube, material },
                    Bounds::new(Vec3::new(0.5, 0.5, 0.5)),
                ));
                world.spawn((
                    Transform::from_translation(Vec3::new(0.0, 0.0, -3.0)),
                    Renderable { mesh: cube, material },
                    Player::new(0),
                    Bounds::new(Vec3::new(0.5, 0.5, 0.5)),
                ));
            })
            .build();
        let reach = Meters::new(100.0).unwrap();
        let enemy = app.player_entity(0).expect("enemy is player 0");
        // Tag the enemy (entity-native classification); the wall stays untagged.
        assert!(app.tag(enemy, 2)); // 2 = "enemy" in this game's vocabulary
        assert!(!app.tag(Entity::from_raw(9999), 2)); // missing node -> false

        // North hits the enemy: the agent reads the exact point and its kind.
        let (north_node, north_point) = app
            .raycast_hit(Vec3::ZERO, Vec3::new(0.0, 0.0, -1.0), reach)
            .expect("ray hits the enemy");
        assert_eq!(north_node, enemy);
        assert!((north_point.z + 2.5).abs() < 1.0e-5, "entry on the near face");
        assert_eq!(app.tag_of(north_node), Some(2));

        // East hits the wall: a real hit, but untagged -> plain geometry.
        let (east_node, _point) = app
            .raycast_hit(Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0), reach)
            .expect("ray hits the wall");
        assert_ne!(east_node, enemy);
        assert_eq!(app.tag_of(east_node), None);

        // Up hits nothing.
        assert!(app
            .raycast_hit(Vec3::ZERO, Vec3::new(0.0, 1.0, 0.0), reach)
            .is_none());
    }

    /// A shared cell the `setup` closure writes the captured handles into.
    type HandleSink = Rc<RefCell<Option<(Handle<Mesh>, Handle<Material>)>>>;

    /// Build an app whose `setup` registers a cube mesh + white material, handing
    /// the captured handles back so a test can `spawn` against them at runtime.
    fn app_with_handles() -> (RunningApp, Handle<Mesh>, Handle<Material>) {
        let sink: HandleSink = Rc::new(RefCell::new(None));
        let captured = sink.clone();
        let app = App::new()
            .window(Window::new(64, 64))
            .add_plugins(DefaultPlugins)
            .setup(move |_world, meshes, materials| {
                let cube = meshes.add(Mesh::cube());
                let material = materials.add(Material::lit(Color::WHITE));
                *captured.borrow_mut() = Some((cube, material));
            })
            .build();
        let (cube, material) = (*sink.borrow()).expect("setup registered the handles");
        (app, cube, material)
    }

    #[test]
    fn spawn_returns_an_entity_queryable_addressable_and_removable() {
        let (mut app, cube, material) = app_with_handles();
        // A plain renderable spawn — no player, no bounds, no caster.
        let plain = app.spawn(Spawn::new(
            Transform::from_translation(Vec3::new(3.0, 0.0, 0.0)),
            cube,
            material,
        ));
        // A player-marked, bounded, shadow-casting spawn — fully queryable.
        let enemy = app.spawn(
            Spawn::new(
                Transform::from_translation(Vec3::new(0.0, 0.0, -3.0)),
                cube,
                material,
            )
            .with_player(0)
            .with_bounds(Vec3::new(0.5, 0.5, 0.5))
            .casts_contact_shadow(),
        );
        // The spawn return is the same handle queries hand back.
        assert_eq!(app.player_entity(0), Some(enemy));
        assert_ne!(plain, enemy);
        let reach = Meters::new(100.0).unwrap();
        assert_eq!(
            app.raycast(Vec3::ZERO, Vec3::new(0.0, 0.0, -1.0), reach),
            Some(enemy)
        );
        assert_eq!(app.player_translation(0), Some(Vec3::new(0.0, 0.0, -3.0)));
        // The plain spawn has no bounds, so it is not a query hit.
        assert_eq!(
            app.raycast(Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0), reach),
            None
        );
        // And the enemy can be despawned by its handle.
        assert!(app.despawn(enemy));
        assert_eq!(
            app.raycast(Vec3::ZERO, Vec3::new(0.0, 0.0, -1.0), reach),
            None
        );
        assert!(!app.despawn(enemy));
    }

    #[test]
    fn player_translation_tracks_the_authoritative_position() {
        let app = App::new()
            .window(Window::new(64, 64))
            .add_plugins(DefaultPlugins)
            .setup(|world, meshes, materials| {
                let cube = meshes.add(Mesh::cube());
                let material = materials.add(Material::lit(Color::WHITE));
                world.spawn((
                    Transform::IDENTITY,
                    Renderable {
                        mesh: cube,
                        material,
                    },
                    Player::new(0),
                ));
            })
            .build();
        let mut app = app;
        // The player spawns at the origin; no node is marked player 1.
        assert_eq!(app.player_translation(0), Some(Vec3::new(0.0, 0.0, 0.0)));
        assert_eq!(app.player_translation(1), None);
        // A move advances the authoritative translation read back from the engine.
        app.tick_with(0, &[PlayerInput::new(0, Vec3::new(1.5, 0.0, 0.0))]);
        assert_eq!(app.player_translation(0), Some(Vec3::new(1.5, 0.0, 0.0)));
    }
}
