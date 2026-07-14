//! The deterministic per-scene snapshot future apps/modules consume.

use crate::bounds_snapshot::BoundsSnapshot;
use crate::camera_snapshot::CameraSnapshot;
use crate::light_snapshot::LightSnapshot;
use crate::node_snapshot::NodeSnapshot;
use crate::renderable_snapshot::RenderableSnapshot;
use crate::scene::Scene;
use crate::scene_node_id::SceneNodeId;
use crate::sdf_shape_snapshot::SdfShapeSnapshot;
use crate::tag_snapshot::TagSnapshot;

/// A deterministic, value-typed snapshot of a [`Scene`].
///
/// Plain data. Lists are ordered by ascending node (entity) id. Two snapshots
/// taken from scenes constructed with equal operations are byte-identical.
///
/// The snapshot intentionally contains **no** GPU objects, browser objects,
/// asset payloads, file paths, editor state, or gameplay state. The app reads
/// it through [`crate::SceneApi`] accessors to translate into a renderer's
/// input.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneSnapshot {
    nodes: Vec<NodeSnapshot>,
    cameras: Vec<CameraSnapshot>,
    lights: Vec<LightSnapshot>,
    renderables: Vec<RenderableSnapshot>,
    sdf_shapes: Vec<SdfShapeSnapshot>,
    tags: Vec<TagSnapshot>,
    bounds: Vec<BoundsSnapshot>,
}

impl SceneSnapshot {
    /// Build a snapshot from a scene. Every node (and its camera/light/
    /// renderable, if any) is read in ascending entity-id order.
    pub fn from_scene(scene: &Scene) -> Self {
        let world = scene.world();
        let storage = world.storage();
        let mut nodes = Vec::new();
        let mut cameras = Vec::new();
        let mut lights = Vec::new();
        let mut renderables = Vec::new();
        let mut sdf_shapes = Vec::new();
        let mut tags = Vec::new();
        let mut bounds = Vec::new();

        storage.locals.iter().for_each(|(id, &local)| {
            let node = SceneNodeId::from_raw(id.raw());
            let parent = storage
                .parents
                .get(id)
                .map(|p| SceneNodeId::from_raw(p.raw()));
            let world_t = storage.worlds.get(id).copied().unwrap_or(local);
            nodes.push(NodeSnapshot::new(node, parent, local, world_t));

            storage.cameras.get(id).into_iter().for_each(|c| {
                cameras.push(CameraSnapshot::new(
                    node,
                    c.fovy_radians(),
                    c.aspect(),
                    c.near(),
                    c.far(),
                ));
            });
            storage.lights.get(id).into_iter().for_each(|l| {
                lights.push(LightSnapshot::new(node, l.kind(), l.color(), l.intensity()));
            });
            storage.renderables.get(id).into_iter().for_each(|r| {
                renderables.push(RenderableSnapshot::new(
                    node,
                    r.mesh(),
                    r.material(),
                    r.texture(),
                    r.animation(),
                    r.visible(),
                    r.casts_contact_shadow(),
                ));
            });
            storage.sdf_shapes.get(id).into_iter().for_each(|shape| {
                sdf_shapes.push(SdfShapeSnapshot::new(
                    node,
                    shape.kind(),
                    shape.dims(),
                    shape.color(),
                ));
            });
            storage.tags.get(id).into_iter().for_each(|tag| {
                tags.push(TagSnapshot::new(node, tag.kind_code()));
            });
            storage.bounds.get(id).into_iter().for_each(|b| {
                bounds.push(BoundsSnapshot::new(node, b.half_extents()));
            });
        });

        SceneSnapshot {
            nodes,
            cameras,
            lights,
            renderables,
            sdf_shapes,
            tags,
            bounds,
        }
    }

    pub fn nodes(&self) -> &[NodeSnapshot] {
        &self.nodes
    }

    /// The node with the given id, or `None` if the scene has no such node.
    ///
    /// `O(log N)`: the node list is kept in ascending id order (see
    /// [`Self::from_scene`]), so this binary-searches rather than scans. A
    /// consumer resolving many ids — e.g. a renderer mapping each renderable to
    /// its node's world transform — does so in `O(log N)` per lookup instead of
    /// `O(N)`, turning an `O(renderables x nodes)` pass into `O(renderables log
    /// nodes)`.
    pub fn node(&self, id: SceneNodeId) -> Option<&NodeSnapshot> {
        self.nodes
            .binary_search_by_key(&id.raw(), |n| n.id().raw())
            .ok()
            .map(|i| &self.nodes[i])
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

    /// The raymarched SDF shapes, ordered by ascending node id. A consumer pairs
    /// each entry with its node's world transform (via [`Self::node`]) to build
    /// the backend-neutral SDF primitive the render backends march.
    pub fn sdf_shapes(&self) -> &[SdfShapeSnapshot] {
        &self.sdf_shapes
    }

    /// The coarse semantic kinds attached to nodes, ordered by ascending node id.
    /// Rolled into the snapshot (M8) so perception and render read the same scene.
    pub fn tags(&self) -> &[TagSnapshot] {
        &self.tags
    }

    /// The axis-aligned bounding volumes attached to nodes, ordered by ascending
    /// node id — each object's collision/query proxy carried alongside its render
    /// binding.
    pub fn bounds(&self) -> &[BoundsSnapshot] {
        &self.bounds
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
            & self.cameras.is_empty()
            & self.lights.is_empty()
            & self.renderables.is_empty()
            & self.sdf_shapes.is_empty()
            & self.tags.is_empty()
            & self.bounds.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bounds::Bounds;
    use crate::camera::Camera;
    use crate::light::Light;
    use crate::material_ref::MaterialRef;
    use crate::mesh_ref::MeshRef;
    use crate::renderable::Renderable;
    use crate::sdf_shape::SdfShape;
    use axiom_kernel::{Meters, Radians, Ratio};
    use axiom_math::{MathApi, Transform, Vec3};

    fn math() -> MathApi {
        MathApi::new()
    }

    fn populated_scene() -> Scene {
        let mut s = Scene::new();
        let a = s.create_node(Transform::from_translation(Vec3::new(1.0, 0.0, 0.0)));
        let b = s.create_node(Transform::from_translation(Vec3::new(0.0, 2.0, 0.0)));
        s.set_parent(b, a).unwrap();
        s.add_camera(
            a,
            Camera::perspective(
                &math(),
                Radians::new(std::f32::consts::FRAC_PI_2).unwrap(),
                Ratio::new(1.0).unwrap(),
                Meters::new(0.1).unwrap(),
                Meters::new(100.0).unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        s.add_light(
            a,
            Light::directional(&math(), Vec3::ONE, Ratio::new(1.0).unwrap()).unwrap(),
        )
        .unwrap();
        s.add_renderable(
            b,
            Renderable::new(MeshRef::from_raw(7), MaterialRef::from_raw(8)).unwrap(),
        )
        .unwrap();
        s.add_sdf_shape(
            b,
            SdfShape::sphere(&math(), Meters::new(0.5).unwrap(), Vec3::ONE).unwrap(),
        )
        .unwrap();
        s.add_tag(b, 42).unwrap();
        s.add_bounds(b, Bounds::new(Vec3::new(0.5, 0.5, 0.5)))
            .unwrap();
        s.update_world_transforms();
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
        assert!(s.sdf_shapes().is_empty());
        assert!(s.tags().is_empty());
        assert!(s.bounds().is_empty());
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
    fn snapshot_contains_camera_light_renderable_and_sdf_shape() {
        let s = SceneSnapshot::from_scene(&populated_scene());
        assert_eq!(s.cameras().len(), 1);
        assert_eq!(s.lights().len(), 1);
        assert_eq!(s.renderables().len(), 1);
        assert_eq!(s.sdf_shapes().len(), 1);
        let shape = &s.sdf_shapes()[0];
        assert_eq!(shape.kind(), SdfShape::SPHERE);
        assert_eq!(shape.dims(), Vec3::new(0.5, 0.5, 0.5));
        assert!(s.node(shape.node()).is_some());
        // Tag and Bounds are rolled into the same snapshot, keyed by the same node.
        assert_eq!(s.tags().len(), 1);
        assert_eq!(s.tags()[0].kind_code(), 42);
        assert!(s.node(s.tags()[0].node()).is_some());
        assert_eq!(s.bounds().len(), 1);
        assert_eq!(s.bounds()[0].half_extents(), Vec3::new(0.5, 0.5, 0.5));
        assert!(s.node(s.bounds()[0].node()).is_some());
    }

    #[test]
    fn snapshot_world_transform_reflects_propagation() {
        let s = SceneSnapshot::from_scene(&populated_scene());
        let child = s
            .nodes()
            .iter()
            .find(|n| n.parent().is_some())
            .expect("a child node");
        assert_eq!(child.world().translation.x, 1.0);
        assert_eq!(child.world().translation.y, 2.0);
    }

    #[test]
    fn snapshot_world_falls_back_to_local_before_propagation() {
        let mut s = Scene::new();
        let a = s.create_node(Transform::from_translation(Vec3::new(3.0, 0.0, 0.0)));
        let b = s.create_node(Transform::from_translation(Vec3::new(0.0, 5.0, 0.0)));
        s.set_parent(b, a).unwrap();
        let snap = SceneSnapshot::from_scene(&s);
        let child = snap.nodes().iter().find(|n| n.parent().is_some()).unwrap();
        assert_eq!(child.world().translation.x, 0.0);
        assert_eq!(child.world().translation.y, 5.0);
    }

    #[test]
    fn node_lookup_resolves_present_ids_and_rejects_absent_ones() {
        let s = SceneSnapshot::from_scene(&populated_scene());
        for n in s.nodes() {
            assert_eq!(s.node(n.id()), Some(n));
        }
        assert!(s.node(SceneNodeId::from_raw(9_999)).is_none());
    }

    #[test]
    fn identical_scenes_produce_identical_snapshots() {
        assert_eq!(
            SceneSnapshot::from_scene(&populated_scene()),
            SceneSnapshot::from_scene(&populated_scene())
        );
    }
}

#[cfg(test)]
mod cov {
    use super::*;
    use crate::animation_ref::AnimationRef;
    use crate::light_kind::LightKind;
    use crate::material_ref::MaterialRef;
    use crate::mesh_ref::MeshRef;
    use crate::texture_ref::TextureRef;
    use axiom_kernel::{Meters, Radians, Ratio};
    use axiom_math::{Transform, Vec3};

    fn node_only() -> NodeSnapshot {
        NodeSnapshot::new(
            SceneNodeId::from_raw(1),
            None,
            Transform::IDENTITY,
            Transform::IDENTITY,
        )
    }
    fn camera_only() -> CameraSnapshot {
        CameraSnapshot::new(
            SceneNodeId::from_raw(1),
            Radians::new(1.0).unwrap(),
            Ratio::new(1.0).unwrap(),
            Meters::new(0.1).unwrap(),
            Meters::new(100.0).unwrap(),
        )
    }
    fn light_only() -> LightSnapshot {
        LightSnapshot::new(
            SceneNodeId::from_raw(1),
            LightKind::Point,
            Vec3::ONE,
            Ratio::new(1.0).unwrap(),
        )
    }
    fn renderable_only() -> RenderableSnapshot {
        RenderableSnapshot::new(
            SceneNodeId::from_raw(1),
            MeshRef::from_raw(1),
            MaterialRef::from_raw(1),
            TextureRef::INVALID,
            AnimationRef::INVALID,
            true,
            false,
        )
    }
    fn sdf_shape_only() -> SdfShapeSnapshot {
        SdfShapeSnapshot::new(SceneNodeId::from_raw(1), 0, Vec3::ONE, Vec3::ONE)
    }
    fn tag_only() -> TagSnapshot {
        TagSnapshot::new(SceneNodeId::from_raw(1), 3)
    }
    fn bounds_only() -> BoundsSnapshot {
        BoundsSnapshot::new(SceneNodeId::from_raw(1), Vec3::ONE)
    }

    fn empty() -> SceneSnapshot {
        SceneSnapshot {
            nodes: vec![],
            cameras: vec![],
            lights: vec![],
            renderables: vec![],
            sdf_shapes: vec![],
            tags: vec![],
            bounds: vec![],
        }
    }

    #[test]
    fn not_empty_when_only_nodes_present() {
        let s = SceneSnapshot {
            nodes: vec![node_only()],
            ..empty()
        };
        assert!(!s.is_empty());
    }

    #[test]
    fn not_empty_when_only_cameras_present() {
        let s = SceneSnapshot {
            cameras: vec![camera_only()],
            ..empty()
        };
        assert!(!s.is_empty());
    }

    #[test]
    fn not_empty_when_only_lights_present() {
        let s = SceneSnapshot {
            lights: vec![light_only()],
            ..empty()
        };
        assert!(!s.is_empty());
    }

    #[test]
    fn not_empty_when_only_renderables_present() {
        let s = SceneSnapshot {
            renderables: vec![renderable_only()],
            ..empty()
        };
        assert!(!s.is_empty());
    }

    #[test]
    fn not_empty_when_only_sdf_shapes_present() {
        let s = SceneSnapshot {
            sdf_shapes: vec![sdf_shape_only()],
            ..empty()
        };
        assert!(!s.is_empty());
    }

    #[test]
    fn not_empty_when_only_tags_present() {
        let s = SceneSnapshot {
            tags: vec![tag_only()],
            ..empty()
        };
        assert!(!s.is_empty());
    }

    #[test]
    fn not_empty_when_only_bounds_present() {
        let s = SceneSnapshot {
            bounds: vec![bounds_only()],
            ..empty()
        };
        assert!(!s.is_empty());
    }

    #[test]
    fn fully_empty_snapshot_is_empty() {
        assert!(empty().is_empty());
    }
}
