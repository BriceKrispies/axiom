//! The demo's world model, expressed on the generic `axiom-ecs` substrate.
//!
//! The cube scene lives as entities + **sparse component columns** in an
//! `axiom_ecs::World<CubeStorage>`: each component type is its own column, so an
//! entity only carries what it has. The parent→child world transform is a
//! `TransformPropagation` `WorldSystem` over those columns — the ECS philosophy
//! that "a transform hierarchy is just a system over the world". A query reads
//! the world into the same `SceneSnapshotArtifact` the render pipeline already
//! consumes, so nothing downstream changes.

use std::collections::BTreeMap;

use axiom_ecs::{ComponentColumn, EntityRegistry, World, WorldStep, WorldSystem};
use axiom_kernel::{BinaryReader, BinaryWriter, EntityId, FieldSchema, KernelResult, Reflect, TypeSchema};
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

impl Reflect for CameraData {
    const SCHEMA: TypeSchema = TypeSchema::new(
        "CameraData",
        &[
            FieldSchema::new("fovy_radians", "f32"),
            FieldSchema::new("aspect", "f32"),
            FieldSchema::new("near", "f32"),
            FieldSchema::new("far", "f32"),
        ],
    );

    fn reflect_write(&self, writer: &mut BinaryWriter) {
        self.fovy_radians.reflect_write(writer);
        self.aspect.reflect_write(writer);
        self.near.reflect_write(writer);
        self.far.reflect_write(writer);
    }

    fn reflect_read(reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
        Ok(CameraData {
            fovy_radians: f32::reflect_read(reader)?,
            aspect: f32::reflect_read(reader)?,
            near: f32::reflect_read(reader)?,
            far: f32::reflect_read(reader)?,
        })
    }
}

impl Reflect for LightData {
    const SCHEMA: TypeSchema = TypeSchema::new(
        "LightData",
        &[
            FieldSchema::new("color", "Vec3"),
            FieldSchema::new("intensity", "f32"),
        ],
    );

    fn reflect_write(&self, writer: &mut BinaryWriter) {
        self.color.reflect_write(writer);
        self.intensity.reflect_write(writer);
    }

    fn reflect_read(reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
        Ok(LightData {
            color: Vec3::reflect_read(reader)?,
            intensity: f32::reflect_read(reader)?,
        })
    }
}

impl Reflect for RenderableData {
    const SCHEMA: TypeSchema = TypeSchema::new(
        "RenderableData",
        &[
            FieldSchema::new("mesh_id", "u64"),
            FieldSchema::new("material_id", "u64"),
            FieldSchema::new("visible", "bool"),
        ],
    );

    fn reflect_write(&self, writer: &mut BinaryWriter) {
        self.mesh_id.reflect_write(writer);
        self.material_id.reflect_write(writer);
        self.visible.reflect_write(writer);
    }

    fn reflect_read(reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
        Ok(RenderableData {
            mesh_id: u64::reflect_read(reader)?,
            material_id: u64::reflect_read(reader)?,
            visible: bool::reflect_read(reader)?,
        })
    }
}

/// The demo's component storage: one sparse column per component type. This is
/// the `S` the generic `axiom_ecs::World<S>` holds.
#[derive(Default)]
pub(crate) struct CubeStorage {
    pub locals: ComponentColumn<Transform>,
    pub worlds: ComponentColumn<Transform>,
    pub parents: ComponentColumn<EntityId>,
    pub cameras: ComponentColumn<CameraData>,
    pub lights: ComponentColumn<LightData>,
    pub renderables: ComponentColumn<RenderableData>,
}

/// The transform-hierarchy system: computes each entity's world transform from
/// its local transform and its parent's world transform
/// (`world = parent_world ∘ local`).
///
/// Entities are processed in ascending entity-id order; the demo always spawns
/// a parent before its children, so a parent's world is computed before any
/// child reads it. Deterministic (ordered `iter` + `BTreeMap`).
pub(crate) struct TransformPropagation;

impl WorldSystem<CubeStorage> for TransformPropagation {
    fn run(&self, _step: &WorldStep, entities: &EntityRegistry, storage: &mut CubeStorage) {
        let mut worlds: BTreeMap<EntityId, Transform> = BTreeMap::new();
        for id in entities.iter() {
            if let Some(&local) = storage.locals.get(id) {
                let world = match storage.parents.get(id).and_then(|p| worlds.get(p).copied()) {
                    Some(parent_world) => Transform::combine(parent_world, local),
                    None => local,
                };
                worlds.insert(id, world);
            }
        }
        for (id, world) in worlds {
            storage.worlds.insert(id, world);
        }
    }
}

/// Read the world into the plain-data `SceneSnapshotArtifact` the render
/// pipeline consumes. Every entity with a local transform is a node; cameras,
/// lights, and renderables come from their respective columns. The entity id is
/// the node id.
pub(crate) fn world_to_scene_snapshot(world: &World<CubeStorage>) -> SceneSnapshotArtifact {
    let storage = world.storage();
    let mut nodes = Vec::new();
    let mut cameras = Vec::new();
    let mut lights = Vec::new();
    let mut renderables = Vec::new();

    for id in world.entities().iter() {
        if let Some(&local) = storage.locals.get(id) {
            nodes.push(SceneNodeArtifact {
                id: id.raw(),
                parent: storage.parents.get(id).map(|p| p.raw()),
                local,
                world: storage.worlds.get(id).copied().unwrap_or(local),
            });
        }
        if let Some(camera) = storage.cameras.get(id) {
            cameras.push(SceneCameraArtifact {
                node: id.raw(),
                fovy_radians: camera.fovy_radians,
                aspect: camera.aspect,
                near: camera.near,
                far: camera.far,
            });
        }
        if let Some(light) = storage.lights.get(id) {
            lights.push(SceneLightArtifact {
                node: id.raw(),
                color: light.color,
                intensity: light.intensity,
            });
        }
        if let Some(renderable) = storage.renderables.get(id) {
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
