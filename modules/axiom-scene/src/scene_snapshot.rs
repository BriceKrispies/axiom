//! The deterministic per-scene snapshot future apps/modules consume.

use crate::camera_snapshot::CameraSnapshot;
use crate::light_snapshot::LightSnapshot;
use crate::node_snapshot::NodeSnapshot;
use crate::renderable_snapshot::RenderableSnapshot;
use crate::scene::Scene;

/// A deterministic, value-typed snapshot of a [`Scene`].
///
/// Plain data. Lists are ordered by their respective ids — ascending
/// `SceneNodeId`, `CameraId`, `LightId`, `RenderableId`. Two snapshots
/// taken from scenes constructed with equal operations are
/// byte-identical.
///
/// The snapshot intentionally contains **no** GPU objects, browser
/// objects, asset payloads, file paths, editor state, or gameplay
/// state. Future resource/render modules (or app composition layers)
/// can extend this contract; the module itself owns only this shape.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneSnapshot {
    nodes: Vec<NodeSnapshot>,
    cameras: Vec<CameraSnapshot>,
    lights: Vec<LightSnapshot>,
    renderables: Vec<RenderableSnapshot>,
}

impl SceneSnapshot {
    /// Build a snapshot from a scene. Reads every node, camera, light,
    /// and renderable in ascending-id order.
    pub fn from_scene(scene: &Scene) -> Self {
        let nodes = scene
            .nodes_in_order()
            .map(|(id, n)| NodeSnapshot::new(id, n.parent(), n.local(), n.world()))
            .collect();
        let cameras = scene
            .cameras_in_order()
            .map(|(id, c)| {
                CameraSnapshot::new(
                    id,
                    c.node(),
                    c.fovy_radians(),
                    c.aspect(),
                    c.near(),
                    c.far(),
                )
            })
            .collect();
        let lights = scene
            .lights_in_order()
            .map(|(id, l)| {
                LightSnapshot::new(id, l.node(), l.kind(), l.color(), l.intensity())
            })
            .collect();
        let renderables = scene
            .renderables_in_order()
            .map(|(id, r)| {
                RenderableSnapshot::new(
                    id,
                    r.node(),
                    r.mesh(),
                    r.material(),
                    r.visible(),
                )
            })
            .collect();
        SceneSnapshot {
            nodes,
            cameras,
            lights,
            renderables,
        }
    }

    pub fn nodes(&self) -> &[NodeSnapshot] {
        &self.nodes
    }

    pub fn cameras(&self) -> &[CameraSnapshot] {
        &self.cameras
    }

    pub fn lights(&self) -> &[LightSnapshot] {
        &self.lights
    }

    pub fn renderables(&self) -> &[RenderableSnapshot] {
        &self.renderables
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
            && self.cameras.is_empty()
            && self.lights.is_empty()
            && self.renderables.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::Camera;
    use crate::light::Light;
    use crate::material_ref::MaterialRef;
    use crate::mesh_ref::MeshRef;
    use crate::renderable::Renderable;
    use axiom_math::{MathApi, Transform, Vec3};

    fn math() -> MathApi {
        MathApi::new()
    }

    fn populated_scene() -> Scene {
        let mut s = Scene::new();
        let a = s.create_node(Transform::from_translation(Vec3::new(1.0, 0.0, 0.0)));
        let b = s.create_node(Transform::from_translation(Vec3::new(0.0, 2.0, 0.0)));
        s.set_parent(b, a).unwrap();
        s.update_world_transforms().unwrap();
        let cam = Camera::perspective(
            &math(),
            a,
            std::f32::consts::FRAC_PI_2,
            1.0,
            0.1,
            100.0,
        )
        .unwrap();
        s.add_camera(cam).unwrap();
        let l = Light::directional(&math(), a, Vec3::ONE, 1.0).unwrap();
        s.add_light(l).unwrap();
        let r = Renderable::new(b, MeshRef::from_raw(7), MaterialRef::from_raw(8)).unwrap();
        s.add_renderable(r).unwrap();
        s
    }

    #[test]
    fn snapshot_of_empty_scene_is_empty() {
        let s = SceneSnapshot::from_scene(&Scene::new());
        assert!(s.is_empty());
        assert!(s.nodes().is_empty());
        assert!(s.cameras().is_empty());
        assert!(s.lights().is_empty());
        assert!(s.renderables().is_empty());
    }

    #[test]
    fn snapshot_preserves_ascending_node_id_order() {
        let s = SceneSnapshot::from_scene(&populated_scene());
        let ids: Vec<u64> = s.nodes().iter().map(|n| n.id().raw()).collect();
        let mut sorted = ids.clone();
        sorted.sort();
        assert_eq!(ids, sorted);
    }

    #[test]
    fn snapshot_contains_camera_light_renderable() {
        let s = SceneSnapshot::from_scene(&populated_scene());
        assert_eq!(s.cameras().len(), 1);
        assert_eq!(s.lights().len(), 1);
        assert_eq!(s.renderables().len(), 1);
    }

    #[test]
    fn snapshot_world_transform_reflects_propagation() {
        let s = SceneSnapshot::from_scene(&populated_scene());
        // Child node is at world (1, 2, 0) because parent translates (1, 0, 0)
        // and child translates (0, 2, 0) in its local frame.
        let child = s
            .nodes()
            .iter()
            .find(|n| n.parent().is_some())
            .expect("expected a child node");
        assert_eq!(child.world().translation.x, 1.0);
        assert_eq!(child.world().translation.y, 2.0);
    }

    #[test]
    fn identical_scenes_produce_identical_snapshots() {
        let a = SceneSnapshot::from_scene(&populated_scene());
        let b = SceneSnapshot::from_scene(&populated_scene());
        assert_eq!(a, b);
    }
}
