//! Typed component access on [`RunningApp`] by `Entity` — `get` / `set` / `query`
//! over the engine's component vocabulary ([`Component`]). The Bevy/Godot-shaped
//! read/write surface: address any node by its first-class handle and access its
//! components by type, never by an engine-internal column or player index.

use axiom_scene::SceneNodeId as Entity;

use super::RunningApp;
use crate::component::Component;

impl RunningApp {
    /// This entity's `T` component, or `None` if it doesn't carry one — the typed
    /// read at the heart of the ECS surface, e.g. `app.get::<Transform>(entity)`.
    pub fn get<T: Component>(&self, entity: Entity) -> Option<T> {
        T::get(&self.scene, entity)
    }

    /// Write `entity`'s `T` component, returning whether it took (`false` if
    /// `entity` is not a live node). The runtime counterpart to authoring in a
    /// bundle, e.g. `app.set::<Transform>(entity, moved)`. World transforms (and
    /// thus spatial queries) refresh on the next tick.
    pub fn set<T: Component>(&mut self, entity: Entity, value: T) -> bool {
        T::set(&mut self.scene, entity, value)
    }

    /// Every `(entity, T)` carrying a `T` component, in ascending `Entity` order —
    /// the single-component iteration primitive, e.g. `app.query::<Bounds>()`.
    pub fn query<T: Component>(&self) -> Vec<(Entity, T)> {
        T::query(&self.scene)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use crate::bounds::Bounds;
    use crate::color::Color;
    use crate::default_plugins::DefaultPlugins;
    use crate::material::Material;
    use crate::mesh::Mesh;
    use crate::player::Player;
    use crate::renderable::Renderable;
    use crate::window::Window;
    use axiom_math::{Transform, Vec3};

    /// An app with one authored node: a player-marked enemy that carries a
    /// renderable and bounds, so its handle is recoverable via `player_entity`.
    fn app_with_enemy() -> RunningApp {
        App::new()
            .window(Window::new(64, 64))
            .add_plugins(DefaultPlugins)
            .setup(|world, meshes, materials| {
                let cube = meshes.add(Mesh::cube());
                let material = materials.add(Material::lit(Color::WHITE));
                world.spawn((
                    Transform::from_translation(Vec3::new(0.0, 0.0, -3.0)),
                    Renderable {
                        mesh: cube,
                        material,
                    },
                    Player::new(0),
                    Bounds::new(Vec3::new(0.5, 0.5, 0.5)),
                ));
            })
            .build()
    }

    #[test]
    fn get_reads_transform_and_bounds_by_entity() {
        let mut app = app_with_enemy();
        let enemy = app.player_entity(0).expect("enemy is player 0");
        assert_eq!(
            app.get::<Transform>(enemy).map(|t| t.translation),
            Some(Vec3::new(0.0, 0.0, -3.0))
        );
        assert_eq!(
            app.get::<Bounds>(enemy),
            Some(Bounds::new(Vec3::new(0.5, 0.5, 0.5)))
        );
        assert!(app.despawn(enemy));
        assert_eq!(app.get::<Transform>(enemy), None);
        assert_eq!(app.get::<Bounds>(enemy), None);
    }

    #[test]
    fn set_writes_transform_and_bounds_and_reports_liveness() {
        let mut app = app_with_enemy();
        let enemy = app.player_entity(0).expect("enemy is player 0");
        let moved = Transform::from_translation(Vec3::new(1.0, 2.0, 3.0));
        assert!(app.set::<Transform>(enemy, moved));
        assert_eq!(
            app.get::<Transform>(enemy).map(|t| t.translation),
            Some(Vec3::new(1.0, 2.0, 3.0))
        );
        assert!(app.set::<Bounds>(enemy, Bounds::new(Vec3::new(2.0, 2.0, 2.0))));
        assert_eq!(
            app.get::<Bounds>(enemy),
            Some(Bounds::new(Vec3::new(2.0, 2.0, 2.0)))
        );
        assert!(app.despawn(enemy));
        assert!(!app.set::<Transform>(enemy, moved));
        assert!(!app.set::<Bounds>(enemy, Bounds::new(Vec3::ONE)));
    }

    #[test]
    fn query_enumerates_each_component() {
        let app = app_with_enemy();
        let enemy = app.player_entity(0).expect("enemy is player 0");
        // DefaultPlugins may add a camera/light node, so assert membership rather
        // than an exact set.
        let transforms = app.query::<Transform>();
        assert!(transforms
            .iter()
            .any(|&(e, t)| e == enemy && t.translation == Vec3::new(0.0, 0.0, -3.0)));
        let bounded = app.query::<Bounds>();
        assert_eq!(
            bounded,
            vec![(enemy, Bounds::new(Vec3::new(0.5, 0.5, 0.5)))]
        );
    }
}
