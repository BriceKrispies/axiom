//! The engine's **dynamic, kind-keyed** retained-world surface on [`RunningApp`]
//! — the app-blind component arm that complements the typed `get`/`set`/`query`
//! in [`super::RunningApp`]'s `app/components.rs`.
//!
//! Where the typed surface addresses the engine's *known* component vocabulary
//! (`Transform`, `Bounds`, …) by Rust type, this surface stores *app-defined*
//! components the engine was never told about at compile time, keyed by their
//! [`Reflect`] schema name in the scene's dynamic arm. It is the native home a
//! retained game world over the wasm boundary (`@axiom/game`'s
//! `world.spawn`/`world.set(e, {kind,…})`/`world.query(...kinds)`) is built on:
//! the bridge app declares a *closed* game-component vocabulary as `Reflect`
//! structs and routes each JS `kind` string to the matching type.
//!
//! Bare-entity lifecycle (`spawn_empty`/`despawn_subtree`/`children_of`) reuses
//! the scene's landed node lifecycle; despawn clears an entity's dynamic
//! components (see `axiom_scene` `Scene::despawn_entity`), so a recycled id never
//! inherits stale components.

use axiom_kernel::Reflect;
use axiom_scene::SceneNodeId as Entity;

use super::RunningApp;

impl RunningApp {
    /// Spawn a bare entity carrying no engine components, returning its [`Entity`]
    /// — the root of a retained game object an app then dresses with dynamic
    /// components via [`Self::set_dynamic`]. The dynamic-world counterpart to
    /// [`Self::spawn`] (which authors a *rendered* node from a mesh/material).
    pub fn spawn_empty(&mut self) -> Entity {
        self.scene.create_node()
    }

    /// Set (or replace) `entity`'s app-defined component of type `T`, stored
    /// type-erased by its [`Reflect`] schema name. Returns whether `entity` named
    /// a live node; a stale handle is a clean `false`, so a dynamic component is
    /// never attached to a dead/absent entity.
    pub fn set_dynamic<T: Reflect>(&mut self, entity: Entity, value: T) -> bool {
        self.scene.set_dynamic(entity, value)
    }

    /// Read `entity`'s dynamic component of type `T` (an owned value) — `None` if
    /// absent, or if the stored bytes do not decode as `T` (a graceful miss,
    /// never UB). The dynamic mirror of [`Self::get`].
    pub fn get_dynamic<T: Reflect>(&self, entity: Entity) -> Option<T> {
        self.scene.get_dynamic(entity)
    }

    /// Whether `entity` carries a dynamic component of type `T`.
    pub fn has_dynamic<T: Reflect>(&self, entity: Entity) -> bool {
        self.scene.has_dynamic::<T>(entity)
    }

    /// Remove `entity`'s dynamic component of type `T`, returning whether it
    /// existed.
    pub fn remove_dynamic<T: Reflect>(&mut self, entity: Entity) -> bool {
        self.scene.remove_dynamic::<T>(entity)
    }

    /// Every entity carrying *all* the dynamic component kinds named in `kinds`
    /// (by [`Reflect`] schema name), in ascending [`Entity`] order — the dynamic
    /// mirror of [`Self::query`], behind a retained world's `query(...kinds)`.
    pub fn query_dynamic(&self, kinds: &[&'static str]) -> Vec<Entity> {
        self.scene.query_dynamic(kinds)
    }

    /// Despawn `entity` and its whole subtree, returning whether `entity` named a
    /// live node — removing a parent takes its attached parts (and all their
    /// dynamic components) with it, so nothing outlives its owner.
    pub fn despawn_subtree(&mut self, entity: Entity) -> bool {
        self.scene.despawn_subtree(entity)
    }

    /// The direct children of `entity`, in ascending [`Entity`] order (empty for a
    /// leaf or an absent node) — for walking an attached-part / formation
    /// hierarchy.
    pub fn children_of(&self, entity: Entity) -> Vec<Entity> {
        self.scene.children_of(entity)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use crate::default_plugins::DefaultPlugins;
    use crate::window::Window;
    use axiom_kernel::{BinaryReader, BinaryWriter, FieldSchema, KernelResult, TypeSchema};

    #[derive(Debug, Clone, Copy, PartialEq)]
    struct Velocity2D {
        x: f32,
        y: f32,
    }
    impl Reflect for Velocity2D {
        const SCHEMA: TypeSchema = TypeSchema::new(
            "Velocity2D",
            &[FieldSchema::new("x", "f32"), FieldSchema::new("y", "f32")],
        );
        fn reflect_write(&self, w: &mut BinaryWriter) {
            self.x.reflect_write(w);
            self.y.reflect_write(w);
        }
        fn reflect_read(r: &mut BinaryReader<'_>) -> KernelResult<Self> {
            f32::reflect_read(r).and_then(|x| f32::reflect_read(r).map(|y| Velocity2D { x, y }))
        }
    }
    #[derive(Debug)]
    struct Marked;
    impl Reflect for Marked {
        const SCHEMA: TypeSchema = TypeSchema::new("Marked", &[]);
        fn reflect_write(&self, _w: &mut BinaryWriter) {}
        fn reflect_read(_r: &mut BinaryReader<'_>) -> KernelResult<Self> {
            Ok(Marked)
        }
    }

    fn app() -> RunningApp {
        App::new()
            .window(Window::new(64, 64))
            .add_plugins(DefaultPlugins)
            .build()
    }

    #[test]
    fn dynamic_world_surface_spawns_sets_reads_queries_and_cascades() {
        let mut app = app();
        let a = app.spawn_empty();
        let b = app.spawn_empty();
        assert_ne!(a, b);
        assert!(app.set_dynamic(a, Velocity2D { x: 1.0, y: 2.0 }));
        assert!(app.set_dynamic(b, Velocity2D { x: 3.0, y: 4.0 }));
        assert!(app.set_dynamic(a, Marked));
        assert!(!app.set_dynamic(Entity::from_raw(9999), Marked));
        assert_eq!(
            app.get_dynamic::<Velocity2D>(a),
            Some(Velocity2D { x: 1.0, y: 2.0 })
        );
        assert!(app.has_dynamic::<Marked>(a));
        assert!(!app.has_dynamic::<Marked>(b));
        assert!(app.get_dynamic::<Marked>(a).is_some());
        assert!(app.get_dynamic::<Velocity2D>(Entity::from_raw(9999)).is_none());
        assert_eq!(app.query_dynamic(&["Velocity2D"]), vec![a, b]);
        assert_eq!(app.query_dynamic(&["Velocity2D", "Marked"]), vec![a]);
        assert!(app.remove_dynamic::<Marked>(a));
        assert!(!app.remove_dynamic::<Marked>(a));
        assert!(app.children_of(a).is_empty());
        assert!(app.despawn_subtree(a));
        assert!(app.get_dynamic::<Velocity2D>(a).is_none());
        assert_eq!(
            app.get_dynamic::<Velocity2D>(b),
            Some(Velocity2D { x: 3.0, y: 4.0 })
        );
    }
}
