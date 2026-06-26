//! The typed component vocabulary: read, write, and enumerate a node's value
//! components by `Entity` — the engine-standard ECS access pattern.
//!
//! Each engine value component (`Transform`, `Bounds`, …) implements
//! [`Component`], mapping the public type onto the scene's storage. That lets
//! [`crate::prelude::RunningApp`] offer generic `get::<T>` / `set::<T>` /
//! `query::<T>` addressed by `Entity` — the Bevy/Godot-shaped surface — without an
//! app ever naming an engine-internal column or node id. The vocabulary is closed:
//! the engine implements [`Component`] for its value types; an app composes the
//! impls through `RunningApp`, it does not add its own.

use axiom_math::Transform;
use axiom_scene::{SceneApi, SceneNodeId as Entity};

use crate::bounds::Bounds;

/// A value-type component addressable by [`Entity`]. An app reads one with
/// [`Component::get`], writes one with [`Component::set`], and enumerates every
/// node that carries one with [`Component::query`]. Implemented by the engine for
/// its component value types.
pub trait Component: Sized {
    /// This component's value on `entity`, or `None` when `entity` is not a live
    /// node or doesn't carry the component.
    fn get(scene: &SceneApi, entity: Entity) -> Option<Self>;

    /// Write this component's value on `entity`, returning whether it took
    /// (`false` when `entity` is not a live node). World transforms refresh on the
    /// next tick, matching the engine's spawn/propagation model.
    fn set(scene: &mut SceneApi, entity: Entity, value: Self) -> bool;

    /// Every `(entity, value)` carrying this component, in ascending `Entity`
    /// order — the single-component iteration primitive.
    fn query(scene: &SceneApi) -> Vec<(Entity, Self)>;
}

impl Component for Transform {
    fn get(scene: &SceneApi, entity: Entity) -> Option<Self> {
        scene.local_transform(entity).ok()
    }

    fn set(scene: &mut SceneApi, entity: Entity, value: Self) -> bool {
        scene.set_local_transform(entity, value).is_ok()
    }

    fn query(scene: &SceneApi) -> Vec<(Entity, Self)> {
        scene.node_transforms()
    }
}

impl Component for Bounds {
    fn get(scene: &SceneApi, entity: Entity) -> Option<Self> {
        scene.bounds_half_extents(entity).map(Bounds::new)
    }

    fn set(scene: &mut SceneApi, entity: Entity, value: Self) -> bool {
        scene.add_bounds(entity, value.half_extents).is_ok()
    }

    fn query(scene: &SceneApi) -> Vec<(Entity, Self)> {
        scene
            .bounded_nodes()
            .into_iter()
            .map(|(entity, half_extents)| (entity, Bounds::new(half_extents)))
            .collect()
    }
}
