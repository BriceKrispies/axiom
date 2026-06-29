//! The scene's **open** component arm of the [`SceneApi`] facade: app/agent-
//! defined components the engine was never told about at compile time, stored
//! type-erased by their [`Reflect`] schema name. A child module so neither this
//! file nor the main `impl SceneApi` block exceeds the engine's size budgets.
//!
//! This is the home a *retained world over the wasm boundary* (`@axiom/game`'s
//! `world.set(e, {kind,…})`) uses: the engine's typed columns stay the zero-cost
//! borrowed hot path, while this serves the app-blind path where only the game
//! names the schema. Despawn clears an entity's dynamic components (see
//! `Scene::despawn_entity`), so a recycled id never inherits stale ones.

use axiom_kernel::Reflect;

use super::SceneApi;
use crate::scene_node_id::SceneNodeId;

impl SceneApi {
    /// Set (or replace) `node`'s dynamic component of type `T`. Returns whether
    /// `node` named a live node; a stale handle is a clean `false`, so a dynamic
    /// component never attaches to a dead/absent entity.
    pub fn set_dynamic<T: Reflect>(&mut self, node: SceneNodeId, value: T) -> bool {
        self.scene.set_dynamic(node, value)
    }

    /// Read `node`'s dynamic component of type `T` (an owned value) — `None` if
    /// absent, or if the stored bytes do not decode as `T` (a graceful miss).
    pub fn get_dynamic<T: Reflect>(&self, node: SceneNodeId) -> Option<T> {
        self.scene.get_dynamic(node)
    }

    /// Whether `node` carries a dynamic component of type `T`.
    pub fn has_dynamic<T: Reflect>(&self, node: SceneNodeId) -> bool {
        self.scene.has_dynamic::<T>(node)
    }

    /// Remove `node`'s dynamic component of type `T`, returning whether it existed.
    pub fn remove_dynamic<T: Reflect>(&mut self, node: SceneNodeId) -> bool {
        self.scene.remove_dynamic::<T>(node)
    }

    /// Every node carrying *all* the dynamic component kinds named in `kinds` (by
    /// [`Reflect`] schema name), in ascending node-id order — the dynamic mirror of
    /// a typed query, behind a retained world's `query(...kinds)`.
    pub fn query_dynamic(&self, kinds: &[&'static str]) -> Vec<SceneNodeId> {
        self.scene.query_dynamic(kinds)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_kernel::{BinaryReader, BinaryWriter, FieldSchema, KernelResult, TypeSchema};

    // A tiny app-defined component for the dynamic-arm tests: two `f32` fields so
    // the typed round-trip (de)serializes real data through `Reflect`.
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
    // A second, zero-field marker kind, so the intersection query spans two kinds.
    #[derive(Debug)]
    struct Marked;
    impl Reflect for Marked {
        const SCHEMA: TypeSchema = TypeSchema::new("Marked", &[]);
        fn reflect_write(&self, _w: &mut BinaryWriter) {}
        fn reflect_read(_r: &mut BinaryReader<'_>) -> KernelResult<Self> {
            Ok(Marked)
        }
    }

    #[test]
    fn dynamic_components_round_trip_query_and_clear_on_despawn() {
        let mut api = SceneApi::new();
        let a = api.create_node();
        let b = api.create_node();
        // Set on live nodes returns true; a stale handle is a clean false (no
        // insert), so a dynamic component never attaches to a non-node.
        assert!(api.set_dynamic(a, Velocity2D { x: 1.0, y: 2.0 }));
        assert!(api.set_dynamic(b, Velocity2D { x: 3.0, y: 4.0 }));
        assert!(api.set_dynamic(a, Marked));
        assert!(!api.set_dynamic(SceneNodeId::from_raw(9999), Marked));
        // Read owned values back; presence checks; absent reads are None/false.
        assert_eq!(
            api.get_dynamic::<Velocity2D>(a),
            Some(Velocity2D { x: 1.0, y: 2.0 })
        );
        assert!(api.has_dynamic::<Marked>(a));
        assert!(!api.has_dynamic::<Marked>(b));
        // The marker round-trips back through its `reflect_read` while present.
        assert!(api.get_dynamic::<Marked>(a).is_some());
        assert!(api
            .get_dynamic::<Velocity2D>(SceneNodeId::from_raw(9999))
            .is_none());
        // Query: a single kind enumerates ascending; the intersection is only the
        // node carrying every named kind.
        assert_eq!(api.query_dynamic(&["Velocity2D"]), vec![a, b]);
        assert_eq!(api.query_dynamic(&["Velocity2D", "Marked"]), vec![a]);
        // Remove drops just that kind; removing again is a clean false.
        assert!(api.remove_dynamic::<Marked>(a));
        assert!(!api.remove_dynamic::<Marked>(a));
        assert!(api.has_dynamic::<Velocity2D>(a));
        // Despawn clears every dynamic component on the node; b is untouched.
        assert!(api.despawn_node(a));
        assert!(api.get_dynamic::<Velocity2D>(a).is_none());
        assert_eq!(
            api.get_dynamic::<Velocity2D>(b),
            Some(Velocity2D { x: 3.0, y: 4.0 })
        );
    }
}
