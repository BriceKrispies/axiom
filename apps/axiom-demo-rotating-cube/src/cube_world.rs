//! The demo's world model, expressed on the generic `axiom-ecs` substrate.
//!
//! This is what replaced `axiom-scene` for this app: the cube scene now lives
//! as entities + a component row in an `axiom_ecs::World`, and the
//! parent→child world transform is computed by a `TransformPropagation`
//! `WorldSystem` — the ECS philosophy that "a transform hierarchy is just a
//! system over the world store". A query then reads the world into the same
//! `SceneSnapshotArtifact` the render pipeline already consumes, so nothing
//! downstream changes.

use std::collections::BTreeMap;

use axiom_ecs::{EntityStore, World, WorldSystem};
use axiom_kernel::EntityId;
use axiom_math::{Transform, Vec3};

use crate::scene_to_render_input::{
    SceneCameraArtifact, SceneLightArtifact, SceneNodeArtifact, SceneRenderableArtifact,
    SceneSnapshotArtifact,
};

/// Perspective camera parameters carried on an entity.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CameraData {
    pub fovy_radians: f32,
    pub aspect: f32,
    pub near: f32,
    pub far: f32,
}

/// Directional light parameters carried on an entity.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LightData {
    pub color: Vec3,
    pub intensity: f32,
}

/// A renderable (mesh + material) carried on an entity.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RenderableData {
    pub mesh_id: u64,
    pub material_id: u64,
    pub visible: bool,
}

/// The demo's component row: the set of optional components any one entity may
/// carry. This is the `R` the generic `axiom_ecs::World<R>` stores.
#[derive(Debug, Clone, Default)]
pub(crate) struct CubeComponents {
    pub local: Option<Transform>,
    pub world: Option<Transform>,
    pub parent: Option<EntityId>,
    pub camera: Option<CameraData>,
    pub light: Option<LightData>,
    pub renderable: Option<RenderableData>,
}

/// The transform-hierarchy system: computes each entity's world transform from
/// its local transform and its parent's world transform
/// (`world = parent_world ∘ local`).
///
/// Entities are processed in ascending entity-id order; the demo always spawns
/// a parent before its children, so a parent's world is computed before any
/// child reads it. Deterministic (ordered `iter` + `BTreeMap`).
pub(crate) struct TransformPropagation;

impl WorldSystem<CubeComponents> for TransformPropagation {
    fn run(&self, store: &mut EntityStore<CubeComponents>) {
        let items: Vec<(EntityId, Transform, Option<EntityId>)> = store
            .iter()
            .filter_map(|(id, row)| row.local.map(|local| (id, local, row.parent)))
            .collect();

        let mut worlds: BTreeMap<EntityId, Transform> = BTreeMap::new();
        for (id, local, parent) in items {
            let world = match parent.and_then(|p| worlds.get(&p).copied()) {
                Some(parent_world) => Transform::combine(parent_world, local),
                None => local,
            };
            worlds.insert(id, world);
        }

        for (id, world) in worlds {
            if let Some(row) = store.get_mut(id) {
                row.world = Some(world);
            }
        }
    }
}

/// Read the world into the plain-data `SceneSnapshotArtifact` the render
/// pipeline consumes. Every entity with a local transform is a node; cameras,
/// lights, and renderables come from their respective components. The entity id
/// is the node id.
pub(crate) fn world_to_scene_snapshot(world: &World<CubeComponents>) -> SceneSnapshotArtifact {
    let mut nodes = Vec::new();
    let mut cameras = Vec::new();
    let mut lights = Vec::new();
    let mut renderables = Vec::new();

    for (id, row) in world.iter() {
        if let Some(local) = row.local {
            nodes.push(SceneNodeArtifact {
                id: id.raw(),
                parent: row.parent.map(|p| p.raw()),
                local,
                world: row.world.unwrap_or(local),
            });
        }
        if let Some(camera) = row.camera {
            cameras.push(SceneCameraArtifact {
                node: id.raw(),
                fovy_radians: camera.fovy_radians,
                aspect: camera.aspect,
                near: camera.near,
                far: camera.far,
            });
        }
        if let Some(light) = row.light {
            lights.push(SceneLightArtifact {
                node: id.raw(),
                color: light.color,
                intensity: light.intensity,
            });
        }
        if let Some(renderable) = row.renderable {
            renderables.push(SceneRenderableArtifact {
                id: id.raw(),
                node: id.raw(),
                mesh_id: renderable.mesh_id,
                material_id: renderable.material_id,
                visible: renderable.visible,
            });
        }
    }

    SceneSnapshotArtifact {
        nodes,
        cameras,
        lights,
        renderables,
    }
}
