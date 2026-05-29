//! The deterministic scene graph storage.

use std::collections::{BTreeMap, BTreeSet};

use axiom_math::Transform;

use crate::camera::Camera;
use crate::camera_id::CameraId;
use crate::light::Light;
use crate::light_id::LightId;
use crate::renderable::Renderable;
use crate::renderable_id::RenderableId;
use crate::scene_error::SceneError;
use crate::scene_node::SceneNode;
use crate::scene_node_id::SceneNodeId;
use crate::scene_result::SceneResult;

/// The deterministic 3D scene graph.
///
/// Stores nodes (each carrying its own transform topology) and three
/// component bags — cameras, lights, renderables — keyed by stable ids.
/// Every collection is a `BTreeMap`, so iteration order is by ascending
/// id on every platform.
///
/// `Scene` is constructed empty through [`crate::SceneApi::empty_scene`];
/// every mutation goes through [`crate::SceneApi`] so validation and
/// invariant maintenance live in one place.
#[derive(Debug, Clone)]
pub struct Scene {
    nodes: BTreeMap<SceneNodeId, SceneNode>,
    next_node_id: u64,
    cameras: BTreeMap<CameraId, Camera>,
    next_camera_id: u64,
    lights: BTreeMap<LightId, Light>,
    next_light_id: u64,
    renderables: BTreeMap<RenderableId, Renderable>,
    next_renderable_id: u64,
}

impl Scene {
    /// Construct an empty scene. Used by [`crate::SceneApi::empty_scene`].
    pub fn new() -> Self {
        Scene {
            nodes: BTreeMap::new(),
            next_node_id: 1,
            cameras: BTreeMap::new(),
            next_camera_id: 1,
            lights: BTreeMap::new(),
            next_light_id: 1,
            renderables: BTreeMap::new(),
            next_renderable_id: 1,
        }
    }

    // --- Counts / iteration helpers (read-only) ---

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn camera_count(&self) -> usize {
        self.cameras.len()
    }

    pub fn light_count(&self) -> usize {
        self.lights.len()
    }

    pub fn renderable_count(&self) -> usize {
        self.renderables.len()
    }

    /// Iterate `(node id, &node)` in ascending node-id order.
    pub fn nodes_in_order(&self) -> impl Iterator<Item = (SceneNodeId, &SceneNode)> {
        self.nodes.iter().map(|(id, n)| (*id, n))
    }

    pub fn cameras_in_order(&self) -> impl Iterator<Item = (CameraId, &Camera)> {
        self.cameras.iter().map(|(id, c)| (*id, c))
    }

    pub fn lights_in_order(&self) -> impl Iterator<Item = (LightId, &Light)> {
        self.lights.iter().map(|(id, l)| (*id, l))
    }

    pub fn renderables_in_order(&self) -> impl Iterator<Item = (RenderableId, &Renderable)> {
        self.renderables.iter().map(|(id, r)| (*id, r))
    }

    pub fn node(&self, id: SceneNodeId) -> SceneResult<&SceneNode> {
        self.nodes
            .get(&id)
            .ok_or_else(|| SceneError::missing_node("scene does not contain that node"))
    }

    pub fn camera(&self, id: CameraId) -> SceneResult<&Camera> {
        self.cameras
            .get(&id)
            .ok_or_else(|| SceneError::missing_camera("scene does not contain that camera"))
    }

    pub fn light(&self, id: LightId) -> SceneResult<&Light> {
        self.lights
            .get(&id)
            .ok_or_else(|| SceneError::missing_light("scene does not contain that light"))
    }

    pub fn renderable(&self, id: RenderableId) -> SceneResult<&Renderable> {
        self.renderables
            .get(&id)
            .ok_or_else(|| {
                SceneError::missing_renderable("scene does not contain that renderable")
            })
    }

    // --- Mutation (crate-private; reached through SceneApi) ---

    pub(crate) fn create_node(&mut self, local: Transform) -> SceneNodeId {
        let id = SceneNodeId::from_raw(self.next_node_id);
        self.next_node_id = self.next_node_id.saturating_add(1);
        self.nodes.insert(id, SceneNode::new(local));
        id
    }

    pub(crate) fn remove_node(&mut self, id: SceneNodeId) -> SceneResult<()> {
        if !self.nodes.contains_key(&id) {
            return Err(SceneError::missing_node(
                "remove_node: node id not in scene",
            ));
        }
        // Detach from parent.
        let parent = self.nodes.get(&id).and_then(|n| n.parent());
        if let Some(pid) = parent {
            if let Some(parent_node) = self.nodes.get_mut(&pid) {
                parent_node.remove_child(id);
            }
        }
        // Detach every child (children become roots).
        let children: Vec<SceneNodeId> = self
            .nodes
            .get(&id)
            .map(|n| n.children().iter().copied().collect())
            .unwrap_or_default();
        for child in children {
            if let Some(child_node) = self.nodes.get_mut(&child) {
                child_node.set_parent(None);
            }
        }
        self.nodes.remove(&id);
        // Remove any components attached to this node.
        self.cameras.retain(|_, c| c.node() != id);
        self.lights.retain(|_, l| l.node() != id);
        self.renderables.retain(|_, r| r.node() != id);
        Ok(())
    }

    pub(crate) fn set_local(
        &mut self,
        id: SceneNodeId,
        local: Transform,
    ) -> SceneResult<()> {
        let node = self.nodes.get_mut(&id).ok_or_else(|| {
            SceneError::missing_node("set_local: node id not in scene")
        })?;
        node.set_local(local);
        Ok(())
    }

    pub(crate) fn set_parent(
        &mut self,
        child: SceneNodeId,
        parent: SceneNodeId,
    ) -> SceneResult<()> {
        if child == parent {
            return Err(SceneError::self_parenting(
                "set_parent: a node cannot be its own parent",
            ));
        }
        if !self.nodes.contains_key(&child) {
            return Err(SceneError::missing_node(
                "set_parent: child id not in scene",
            ));
        }
        if !self.nodes.contains_key(&parent) {
            return Err(SceneError::missing_node(
                "set_parent: parent id not in scene",
            ));
        }
        if self.would_introduce_cycle(child, parent) {
            return Err(SceneError::hierarchy_cycle(
                "set_parent: assignment would introduce a cycle",
            ));
        }
        let old_parent = self.nodes[&child].parent();
        if let Some(old) = old_parent {
            if let Some(parent_node) = self.nodes.get_mut(&old) {
                parent_node.remove_child(child);
            }
        }
        if let Some(child_node) = self.nodes.get_mut(&child) {
            child_node.set_parent(Some(parent));
        }
        if let Some(parent_node) = self.nodes.get_mut(&parent) {
            parent_node.add_child(child);
        }
        Ok(())
    }

    pub(crate) fn clear_parent(&mut self, child: SceneNodeId) -> SceneResult<()> {
        if !self.nodes.contains_key(&child) {
            return Err(SceneError::missing_node(
                "clear_parent: child id not in scene",
            ));
        }
        let old_parent = self.nodes[&child].parent();
        if let Some(old) = old_parent {
            if let Some(parent_node) = self.nodes.get_mut(&old) {
                parent_node.remove_child(child);
            }
        }
        if let Some(child_node) = self.nodes.get_mut(&child) {
            child_node.set_parent(None);
        }
        Ok(())
    }

    fn would_introduce_cycle(&self, child: SceneNodeId, new_parent: SceneNodeId) -> bool {
        // Walk from `new_parent` upward; if we ever reach `child`, the
        // reparent would create a cycle.
        let mut cursor = Some(new_parent);
        let mut visited: BTreeSet<SceneNodeId> = BTreeSet::new();
        while let Some(id) = cursor {
            if id == child {
                return true;
            }
            if !visited.insert(id) {
                // We already visited this node — guards against the
                // (theoretically impossible) pre-existing cycle.
                return true;
            }
            cursor = self.nodes.get(&id).and_then(|n| n.parent());
        }
        false
    }

    // --- Transform propagation ---

    /// Recompute every node's world transform: `world = parent_world *
    /// local`, traversing parents before children in deterministic
    /// ascending-id sibling order.
    pub(crate) fn update_world_transforms(&mut self) -> SceneResult<()> {
        // 1. Collect roots in ascending id order.
        let roots: Vec<SceneNodeId> = self
            .nodes
            .iter()
            .filter(|(_, n)| n.parent().is_none())
            .map(|(id, _)| *id)
            .collect();

        // 2. Iterative pre-order traversal so we never recurse into the
        //    stack (deterministic for any depth). Each entry carries
        //    the parent's already-computed world transform.
        let mut stack: Vec<(SceneNodeId, Transform)> = Vec::new();
        // Push roots in *reverse* so the smallest id is popped first
        // (BTreeMap keeps children in ascending order; reversing here
        // matches that order under LIFO).
        for &id in roots.iter().rev() {
            let local = self.nodes[&id].local();
            stack.push((id, local));
        }

        let mut updates: BTreeMap<SceneNodeId, Transform> = BTreeMap::new();
        while let Some((id, world)) = stack.pop() {
            let node = self.nodes.get(&id).ok_or_else(|| {
                SceneError::hierarchy_update_failed(
                    "propagation visited a node that no longer exists",
                )
            })?;
            updates.insert(id, world);
            // Push children in reverse order so smallest id is processed
            // next under LIFO traversal.
            let children: Vec<SceneNodeId> = node.children().iter().copied().collect();
            for &child_id in children.iter().rev() {
                let child_local = self
                    .nodes
                    .get(&child_id)
                    .ok_or_else(|| {
                        SceneError::hierarchy_update_failed(
                            "propagation referenced a missing child",
                        )
                    })?
                    .local();
                let child_world = Transform::combine(world, child_local);
                stack.push((child_id, child_world));
            }
        }

        for (id, world) in updates {
            if let Some(node) = self.nodes.get_mut(&id) {
                node.set_world(world);
            }
        }
        Ok(())
    }

    // --- Component mutation ---

    pub(crate) fn add_camera(&mut self, camera: Camera) -> SceneResult<CameraId> {
        if !self.nodes.contains_key(&camera.node()) {
            return Err(SceneError::missing_node(
                "add_camera: attached node id not in scene",
            ));
        }
        let id = CameraId::from_raw(self.next_camera_id);
        self.next_camera_id = self.next_camera_id.saturating_add(1);
        self.cameras.insert(id, camera);
        Ok(id)
    }

    pub(crate) fn remove_camera(&mut self, id: CameraId) -> SceneResult<()> {
        if self.cameras.remove(&id).is_none() {
            return Err(SceneError::missing_camera(
                "remove_camera: camera id not in scene",
            ));
        }
        Ok(())
    }

    pub(crate) fn add_light(&mut self, light: Light) -> SceneResult<LightId> {
        if !self.nodes.contains_key(&light.node()) {
            return Err(SceneError::missing_node(
                "add_light: attached node id not in scene",
            ));
        }
        let id = LightId::from_raw(self.next_light_id);
        self.next_light_id = self.next_light_id.saturating_add(1);
        self.lights.insert(id, light);
        Ok(id)
    }

    pub(crate) fn remove_light(&mut self, id: LightId) -> SceneResult<()> {
        if self.lights.remove(&id).is_none() {
            return Err(SceneError::missing_light(
                "remove_light: light id not in scene",
            ));
        }
        Ok(())
    }

    pub(crate) fn add_renderable(
        &mut self,
        renderable: Renderable,
    ) -> SceneResult<RenderableId> {
        if !self.nodes.contains_key(&renderable.node()) {
            return Err(SceneError::missing_node(
                "add_renderable: attached node id not in scene",
            ));
        }
        let id = RenderableId::from_raw(self.next_renderable_id);
        self.next_renderable_id = self.next_renderable_id.saturating_add(1);
        self.renderables.insert(id, renderable);
        Ok(id)
    }

    pub(crate) fn remove_renderable(&mut self, id: RenderableId) -> SceneResult<()> {
        if self.renderables.remove(&id).is_none() {
            return Err(SceneError::missing_renderable(
                "remove_renderable: renderable id not in scene",
            ));
        }
        Ok(())
    }

    pub(crate) fn set_renderable_visible(
        &mut self,
        id: RenderableId,
        visible: bool,
    ) -> SceneResult<()> {
        let r = self.renderables.get_mut(&id).ok_or_else(|| {
            SceneError::missing_renderable(
                "set_renderable_visible: renderable id not in scene",
            )
        })?;
        r.set_visible(visible);
        Ok(())
    }
}

impl Default for Scene {
    fn default() -> Self {
        Scene::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene_error_code::SceneErrorCode;
    use axiom_math::{MathApi, Vec3};

    fn math() -> MathApi {
        MathApi::new()
    }

    #[test]
    fn empty_scene_has_no_nodes() {
        let s = Scene::new();
        assert_eq!(s.node_count(), 0);
        assert_eq!(s.camera_count(), 0);
        assert_eq!(s.light_count(), 0);
        assert_eq!(s.renderable_count(), 0);
    }

    #[test]
    fn node_ids_are_assigned_monotonically() {
        let mut s = Scene::new();
        let a = s.create_node(Transform::IDENTITY);
        let b = s.create_node(Transform::IDENTITY);
        let c = s.create_node(Transform::IDENTITY);
        assert_eq!(a.raw(), 1);
        assert_eq!(b.raw(), 2);
        assert_eq!(c.raw(), 3);
    }

    #[test]
    fn missing_node_query_fails_deterministically() {
        let s = Scene::new();
        let err = s.node(SceneNodeId::from_raw(99)).unwrap_err();
        assert_eq!(err.code(), SceneErrorCode::MissingNode);
    }

    #[test]
    fn set_local_round_trips() {
        let mut s = Scene::new();
        let id = s.create_node(Transform::IDENTITY);
        let t = Transform::from_translation(Vec3::new(1.0, 2.0, 3.0));
        s.set_local(id, t).unwrap();
        assert_eq!(s.node(id).unwrap().local().translation.x, 1.0);
    }

    #[test]
    fn set_parent_links_both_sides() {
        let mut s = Scene::new();
        let p = s.create_node(Transform::IDENTITY);
        let c = s.create_node(Transform::IDENTITY);
        s.set_parent(c, p).unwrap();
        assert_eq!(s.node(c).unwrap().parent(), Some(p));
        assert!(s.node(p).unwrap().children().contains(&c));
    }

    #[test]
    fn self_parenting_fails() {
        let mut s = Scene::new();
        let n = s.create_node(Transform::IDENTITY);
        let err = s.set_parent(n, n).unwrap_err();
        assert_eq!(err.code(), SceneErrorCode::SelfParenting);
    }

    #[test]
    fn cycle_creation_fails() {
        let mut s = Scene::new();
        let a = s.create_node(Transform::IDENTITY);
        let b = s.create_node(Transform::IDENTITY);
        let c = s.create_node(Transform::IDENTITY);
        s.set_parent(b, a).unwrap();
        s.set_parent(c, b).unwrap();
        // a's chain: a is root. Now make a a child of c → would cycle.
        let err = s.set_parent(a, c).unwrap_err();
        assert_eq!(err.code(), SceneErrorCode::HierarchyCycle);
    }

    #[test]
    fn missing_parent_id_fails() {
        let mut s = Scene::new();
        let c = s.create_node(Transform::IDENTITY);
        let err = s
            .set_parent(c, SceneNodeId::from_raw(99))
            .unwrap_err();
        assert_eq!(err.code(), SceneErrorCode::MissingNode);
    }

    #[test]
    fn clear_parent_detaches_cleanly() {
        let mut s = Scene::new();
        let p = s.create_node(Transform::IDENTITY);
        let c = s.create_node(Transform::IDENTITY);
        s.set_parent(c, p).unwrap();
        s.clear_parent(c).unwrap();
        assert!(s.node(c).unwrap().parent().is_none());
        assert!(s.node(p).unwrap().children().is_empty());
    }

    #[test]
    fn remove_node_detaches_children_and_components() {
        let mut s = Scene::new();
        let p = s.create_node(Transform::IDENTITY);
        let c = s.create_node(Transform::IDENTITY);
        s.set_parent(c, p).unwrap();
        s.remove_node(p).unwrap();
        assert!(s.node(p).is_err());
        // Child becomes a root.
        assert!(s.node(c).unwrap().parent().is_none());
    }

    #[test]
    fn remove_node_id_not_reused() {
        let mut s = Scene::new();
        let a = s.create_node(Transform::IDENTITY);
        s.remove_node(a).unwrap();
        let b = s.create_node(Transform::IDENTITY);
        assert_ne!(a, b, "ids must never be reused");
        assert_eq!(b.raw(), 2);
    }

    #[test]
    fn world_transform_starts_at_identity_by_default() {
        let mut s = Scene::new();
        let id = s.create_node(Transform::IDENTITY);
        let t = s.node(id).unwrap().world();
        assert_eq!(t.translation.x, 0.0);
    }

    #[test]
    fn update_world_transforms_propagates_parent_to_child() {
        let mut s = Scene::new();
        let p = s.create_node(Transform::from_translation(Vec3::new(1.0, 0.0, 0.0)));
        let c = s.create_node(Transform::from_translation(Vec3::new(0.0, 2.0, 0.0)));
        s.set_parent(c, p).unwrap();
        s.update_world_transforms().unwrap();
        let world_c = s.node(c).unwrap().world();
        assert_eq!(world_c.translation.x, 1.0);
        assert_eq!(world_c.translation.y, 2.0);
    }

    #[test]
    fn update_world_transforms_is_deterministic_across_runs() {
        let make = || {
            let mut s = Scene::new();
            let a = s.create_node(Transform::from_translation(Vec3::new(1.0, 0.0, 0.0)));
            let b = s.create_node(Transform::from_translation(Vec3::new(0.0, 1.0, 0.0)));
            s.set_parent(b, a).unwrap();
            s.update_world_transforms().unwrap();
            let aw = s.node(a).unwrap().world();
            let bw = s.node(b).unwrap().world();
            (aw.translation.x, aw.translation.y, bw.translation.x, bw.translation.y)
        };
        assert_eq!(make(), make());
    }

    #[test]
    fn camera_attached_to_missing_node_fails() {
        let mut s = Scene::new();
        let cam = Camera::perspective(
            &math(),
            SceneNodeId::from_raw(42),
            std::f32::consts::FRAC_PI_2,
            1.0,
            0.1,
            100.0,
        )
        .unwrap();
        let err = s.add_camera(cam).unwrap_err();
        assert_eq!(err.code(), SceneErrorCode::MissingNode);
    }

    #[test]
    fn remove_camera_missing_id_fails() {
        let mut s = Scene::new();
        let err = s.remove_camera(CameraId::from_raw(1)).unwrap_err();
        assert_eq!(err.code(), SceneErrorCode::MissingCamera);
    }

    #[test]
    fn light_attached_to_missing_node_fails() {
        let mut s = Scene::new();
        let l =
            Light::directional(&math(), SceneNodeId::from_raw(42), Vec3::ONE, 1.0)
                .unwrap();
        let err = s.add_light(l).unwrap_err();
        assert_eq!(err.code(), SceneErrorCode::MissingNode);
    }

    #[test]
    fn renderable_attached_to_missing_node_fails() {
        use crate::material_ref::MaterialRef;
        use crate::mesh_ref::MeshRef;
        let mut s = Scene::new();
        let r = Renderable::new(
            SceneNodeId::from_raw(42),
            MeshRef::from_raw(1),
            MaterialRef::from_raw(1),
        )
        .unwrap();
        let err = s.add_renderable(r).unwrap_err();
        assert_eq!(err.code(), SceneErrorCode::MissingNode);
    }

    #[test]
    fn nodes_iterate_in_ascending_id_order() {
        let mut s = Scene::new();
        let a = s.create_node(Transform::IDENTITY);
        let b = s.create_node(Transform::IDENTITY);
        let c = s.create_node(Transform::IDENTITY);
        let ids: Vec<u64> = s.nodes_in_order().map(|(id, _)| id.raw()).collect();
        assert_eq!(ids, vec![a.raw(), b.raw(), c.raw()]);
    }
}
