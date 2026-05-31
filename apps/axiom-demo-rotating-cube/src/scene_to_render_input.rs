//! App-owned glue: `SceneSnapshot + ResolvedResources → RenderInput`.
//!
//! `axiom-scene`, `axiom-resources`, and `axiom-render` never import one
//! another. Their contract values (`SceneSnapshot`, `ResolvedResources`,
//! `RenderInput`) are intentionally **not nameable** outside their owning
//! modules — each module re-exports exactly one facade. So the demo app
//! reads each producer through its facade into the plain-data artifacts
//! defined here, then this module performs the *semantic* translation on
//! those nameable artifacts.
//!
//! The orchestrator in [`crate::vertical_slice`] does the un-nameable
//! plumbing (calling facade accessors); the actual translation policy —
//! which lights to emit, how resolved meshes/materials map to render
//! indices, how renderables become draw objects — lives in
//! [`scene_to_render_input`] and is unit-testable on plain data.

use axiom_math::{Mat4, MathApi, Transform, Vec3};

/// Logical viewport width the demo renders at (logical pixels).
pub(crate) const VIEWPORT_WIDTH: u32 = 800;
/// Logical viewport height the demo renders at (logical pixels).
pub(crate) const VIEWPORT_HEIGHT: u32 = 600;

/// Background clear colour (linear RGBA).
pub(crate) const DEMO_CLEAR_COLOR: [f32; 4] = [0.05, 0.06, 0.08, 1.0];
/// Base colour of the demo cube's basic-lit material (linear RGBA).
pub(crate) const DEMO_CUBE_BASE_COLOR: [f32; 4] = [0.8, 0.4, 0.2, 1.0];
/// World-space direction the demo's single directional light points along.
pub(crate) const DEMO_LIGHT_DIRECTION_WORLD: Vec3 = Vec3::new(0.3, -1.0, 0.4);
/// Colour of the demo's directional light (linear RGB).
pub(crate) const DEMO_LIGHT_COLOR: Vec3 = Vec3::new(1.0, 1.0, 1.0);
/// Intensity of the demo's directional light.
pub(crate) const DEMO_LIGHT_INTENSITY: f32 = 1.0;

/// Render-side light kind code: a directional light.
pub(crate) const LIGHT_KIND_DIRECTIONAL: u32 = 0;

// ----------------------------------------------------------------------
// Source artifact: a plain-data mirror of `axiom_scene::SceneSnapshot`.
// ----------------------------------------------------------------------

/// One node entry mirrored from a scene snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneNodeArtifact {
    pub id: u64,
    pub parent: Option<u64>,
    pub local: Transform,
    pub world: Transform,
}

/// One camera entry mirrored from a scene snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneCameraArtifact {
    pub node: u64,
    pub fovy_radians: f32,
    pub aspect: f32,
    pub near: f32,
    pub far: f32,
}

/// One light entry mirrored from a scene snapshot.
///
/// `axiom-scene`'s `LightKind` is not nameable outside the module, and the
/// demo only ever creates directional lights, so the kind is not mirrored
/// here; the translation treats every scene light as directional.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneLightArtifact {
    pub node: u64,
    pub color: Vec3,
    pub intensity: f32,
}

/// One renderable entry mirrored from a scene snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneRenderableArtifact {
    pub id: u64,
    pub node: u64,
    pub mesh_id: u64,
    pub material_id: u64,
    pub visible: bool,
}

/// Plain-data mirror of `axiom_scene::SceneSnapshot`.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneSnapshotArtifact {
    pub nodes: Vec<SceneNodeArtifact>,
    pub cameras: Vec<SceneCameraArtifact>,
    pub lights: Vec<SceneLightArtifact>,
    pub renderables: Vec<SceneRenderableArtifact>,
}

impl SceneSnapshotArtifact {
    /// World transform of a node by id, if present.
    pub(crate) fn world_of(&self, node_id: u64) -> Option<Transform> {
        self.nodes
            .iter()
            .find(|n| n.id == node_id)
            .map(|n| n.world)
    }
}

// ----------------------------------------------------------------------
// Source artifact: a plain-data mirror of `axiom_resources::ResolvedResources`.
// ----------------------------------------------------------------------

/// One resolved mesh, with full CPU-side vertex data so the artifact is
/// fully inspectable and deterministic.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedMeshArtifact {
    pub id: u64,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
}

/// One resolved material.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedMaterialArtifact {
    pub id: u64,
    pub base_color: [f32; 4],
}

/// Plain-data mirror of `axiom_resources::ResolvedResources`.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedResourcesArtifact {
    pub meshes: Vec<ResolvedMeshArtifact>,
    pub materials: Vec<ResolvedMaterialArtifact>,
}

// ----------------------------------------------------------------------
// Target artifact: a plain-data plan for `axiom_render::RenderInput`.
// ----------------------------------------------------------------------

/// The camera the render input should use.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderCameraArtifact {
    pub view: Mat4,
    pub projection: Mat4,
}

/// One light in the render input.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderLightArtifact {
    pub kind_code: u32,
    pub vector_world: Vec3,
    pub color: Vec3,
    pub intensity: f32,
}

/// One mesh upload in the render input.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderMeshArtifact {
    pub id: u64,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
}

/// One material upload in the render input.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderMaterialArtifact {
    pub id: u64,
    pub base_color: [f32; 4],
}

/// One draw object in the render input. `mesh_idx`/`material_idx` are
/// indices into the render input's mesh/material arrays (render uses
/// indices, not resource ids).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderObjectArtifact {
    pub world: Mat4,
    pub mesh_idx: u32,
    pub material_idx: u32,
    pub visible: bool,
}

/// Plain-data plan for an `axiom_render::RenderInput`. The orchestrator
/// replays this plan into the real `RenderApi` builder to obtain the
/// (un-nameable) `RenderInput`.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderInputArtifact {
    pub viewport_width: u32,
    pub viewport_height: u32,
    pub clear_color: [f32; 4],
    pub camera: Option<RenderCameraArtifact>,
    pub lights: Vec<RenderLightArtifact>,
    pub meshes: Vec<RenderMeshArtifact>,
    pub materials: Vec<RenderMaterialArtifact>,
    pub objects: Vec<RenderObjectArtifact>,
}

/// Compute a view matrix from a camera node's world transform.
///
/// `view = inverse(world)`. The demo's camera node always has identity
/// scale, the only case where `Transform::inverse` succeeds.
pub(crate) fn view_matrix_from_world(camera_world: Transform) -> Mat4 {
    camera_world
        .inverse()
        .expect("demo camera node always has identity scale, so inverse succeeds")
        .to_matrix()
}

/// Translate a scene snapshot + resolved resources into a neutral render
/// input plan. This is the heart of the scene→render app glue and is
/// pure: same inputs always produce the same plan.
///
/// `math` provides the deterministic perspective projection; the camera
/// view is derived from the camera node's world transform.
pub(crate) fn scene_to_render_input(
    math: &MathApi,
    scene: &SceneSnapshotArtifact,
    resources: &ResolvedResourcesArtifact,
) -> RenderInputArtifact {
    // Camera: first camera in the snapshot, if any.
    let camera = scene.cameras.first().map(|cam| {
        let world = scene
            .world_of(cam.node)
            .expect("camera node id is present in the node list");
        let view = view_matrix_from_world(world);
        let projection = math
            .mat4_perspective(cam.fovy_radians, cam.aspect, cam.near, cam.far)
            .expect("camera intrinsics were validated at scene insertion time");
        RenderCameraArtifact { view, projection }
    });

    // Lights: every demo light is directional with the demo's world
    // direction; colour and intensity carry through from the snapshot.
    let lights = scene
        .lights
        .iter()
        .map(|light| RenderLightArtifact {
            kind_code: LIGHT_KIND_DIRECTIONAL,
            vector_world: DEMO_LIGHT_DIRECTION_WORLD,
            color: light.color,
            intensity: light.intensity,
        })
        .collect();

    // Meshes / materials: resolved order defines the render-side index.
    let meshes: Vec<RenderMeshArtifact> = resources
        .meshes
        .iter()
        .map(|m| RenderMeshArtifact {
            id: m.id,
            positions: m.positions.clone(),
            normals: m.normals.clone(),
            uvs: m.uvs.clone(),
            indices: m.indices.clone(),
        })
        .collect();
    let materials: Vec<RenderMaterialArtifact> = resources
        .materials
        .iter()
        .map(|m| RenderMaterialArtifact {
            id: m.id,
            base_color: m.base_color,
        })
        .collect();

    // Objects: one per renderable, resolving its mesh/material ids to the
    // render-side indices computed above. A renderable whose refs do not
    // resolve is skipped (render would skip it anyway).
    let objects = scene
        .renderables
        .iter()
        .filter_map(|r| {
            let world = scene.world_of(r.node)?.to_matrix();
            let mesh_idx = meshes.iter().position(|m| m.id == r.mesh_id)? as u32;
            let material_idx =
                materials.iter().position(|m| m.id == r.material_id)? as u32;
            Some(RenderObjectArtifact {
                world,
                mesh_idx,
                material_idx,
                visible: r.visible,
            })
        })
        .collect();

    RenderInputArtifact {
        viewport_width: VIEWPORT_WIDTH,
        viewport_height: VIEWPORT_HEIGHT,
        clear_color: DEMO_CLEAR_COLOR,
        camera,
        lights,
        meshes,
        materials,
        objects,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scene_with_cube() -> SceneSnapshotArtifact {
        SceneSnapshotArtifact {
            nodes: vec![
                SceneNodeArtifact {
                    id: 1,
                    parent: None,
                    local: Transform::IDENTITY,
                    world: Transform::IDENTITY,
                },
                SceneNodeArtifact {
                    id: 2,
                    parent: Some(1),
                    local: Transform::IDENTITY,
                    world: Transform::IDENTITY,
                },
                SceneNodeArtifact {
                    id: 3,
                    parent: None,
                    local: Transform::from_translation(Vec3::new(0.0, 0.0, 5.0)),
                    world: Transform::from_translation(Vec3::new(0.0, 0.0, 5.0)),
                },
            ],
            cameras: vec![SceneCameraArtifact {
                node: 3,
                fovy_radians: std::f32::consts::FRAC_PI_3,
                aspect: 4.0 / 3.0,
                near: 0.1,
                far: 100.0,
            }],
            lights: vec![SceneLightArtifact {
                node: 1,
                color: Vec3::ONE,
                intensity: 1.0,
            }],
            renderables: vec![SceneRenderableArtifact {
                id: 1,
                node: 2,
                mesh_id: 1,
                material_id: 2,
                visible: true,
            }],
        }
    }

    fn resources_with_cube() -> ResolvedResourcesArtifact {
        ResolvedResourcesArtifact {
            meshes: vec![ResolvedMeshArtifact {
                id: 1,
                positions: vec![[0.5, 0.5, 0.5]; 24],
                normals: vec![[0.0, 1.0, 0.0]; 24],
                uvs: vec![[0.0, 0.0]; 24],
                indices: (0..36).collect(),
            }],
            materials: vec![ResolvedMaterialArtifact {
                id: 2,
                base_color: DEMO_CUBE_BASE_COLOR,
            }],
        }
    }

    #[test]
    fn translation_is_pure_and_deterministic() {
        let math = MathApi::new();
        let scene = scene_with_cube();
        let resources = resources_with_cube();
        let a = scene_to_render_input(&math, &scene, &resources);
        let b = scene_to_render_input(&math, &scene, &resources);
        assert_eq!(a, b);
    }

    #[test]
    fn object_resolves_to_mesh_and_material_indices() {
        let math = MathApi::new();
        let input = scene_to_render_input(&math, &scene_with_cube(), &resources_with_cube());
        assert_eq!(input.objects.len(), 1);
        assert_eq!(input.objects[0].mesh_idx, 0);
        assert_eq!(input.objects[0].material_idx, 0);
        assert!(input.objects[0].visible);
    }

    #[test]
    fn camera_view_sends_camera_position_to_origin() {
        let math = MathApi::new();
        let input = scene_to_render_input(&math, &scene_with_cube(), &resources_with_cube());
        let camera = input.camera.expect("scene has a camera");
        let eye = camera.view.transform_point(Vec3::new(0.0, 0.0, 5.0));
        assert!(eye.x.abs() < 1.0e-5);
        assert!(eye.y.abs() < 1.0e-5);
        assert!(eye.z.abs() < 1.0e-5);
    }

    #[test]
    fn lights_translate_to_directional() {
        let math = MathApi::new();
        let input = scene_to_render_input(&math, &scene_with_cube(), &resources_with_cube());
        assert_eq!(input.lights.len(), 1);
        assert_eq!(input.lights[0].kind_code, LIGHT_KIND_DIRECTIONAL);
        assert_eq!(input.lights[0].vector_world, DEMO_LIGHT_DIRECTION_WORLD);
    }

    #[test]
    fn unresolvable_object_is_skipped() {
        let math = MathApi::new();
        let mut scene = scene_with_cube();
        scene.renderables[0].mesh_id = 999;
        let input = scene_to_render_input(&math, &scene, &resources_with_cube());
        assert!(input.objects.is_empty());
    }
}
