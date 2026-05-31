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
        self.nodes
            .get_mut(&child)
            .expect("set_parent: child present (validated by contains_key above)")
            .set_parent(Some(parent));
        self.nodes
            .get_mut(&parent)
            .expect("set_parent: parent present (validated by contains_key above)")
            .add_child(child);
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
        self.nodes
            .get_mut(&child)
            .expect("clear_parent: child present (validated by contains_key above)")
            .set_parent(None);
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
            // `id` was just popped from the stack, which only ever holds ids
            // taken from the live node map (roots, and children validated
            // present below), so indexing cannot miss.
            let node = &self.nodes[&id];
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
            self.nodes
                .get_mut(&id)
                .expect("propagation: every id in `updates` was just read from the live map")
                .set_world(world);
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
    fn populated_scene_reports_nonzero_node_and_renderable_counts() {
        use crate::material_ref::MaterialRef;
        use crate::mesh_ref::MeshRef;
        // Kills `node_count -> 0` and `renderable_count -> 0`: build a scene
        // with a known non-zero number of nodes and renderables.
        let mut s = Scene::new();
        let n1 = s.create_node(Transform::IDENTITY);
        let _n2 = s.create_node(Transform::IDENTITY);
        assert_eq!(s.node_count(), 2);
        let r =
            Renderable::new(n1, MeshRef::from_raw(1), MaterialRef::from_raw(2)).unwrap();
        s.add_renderable(r).unwrap();
        assert_eq!(s.renderable_count(), 1);
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

#[cfg(test)]
mod cov {
    use super::*;
    use crate::material_ref::MaterialRef;
    use crate::mesh_ref::MeshRef;
    use crate::scene_error_code::SceneErrorCode;
    use axiom_math::{MathApi, Vec3};

    fn math() -> MathApi {
        MathApi::new()
    }

    fn cam_on(s: &mut Scene, node: SceneNodeId) -> CameraId {
        let cam =
            Camera::perspective(&math(), node, std::f32::consts::FRAC_PI_2, 1.0, 0.1, 100.0)
                .unwrap();
        s.add_camera(cam).unwrap()
    }

    fn light_on(s: &mut Scene, node: SceneNodeId) -> LightId {
        let l = Light::directional(&math(), node, Vec3::ONE, 1.0).unwrap();
        s.add_light(l).unwrap()
    }

    fn renderable_on(s: &mut Scene, node: SceneNodeId) -> RenderableId {
        let r = Renderable::new(node, MeshRef::from_raw(1), MaterialRef::from_raw(2)).unwrap();
        s.add_renderable(r).unwrap()
    }

    // ---------- read queries: Ok (Some) + Err (None) arms ----------

    #[test]
    fn camera_query_returns_present_and_missing() {
        let mut s = Scene::new();
        let n = s.create_node(Transform::IDENTITY);
        let id = cam_on(&mut s, n);
        assert!(s.camera(id).is_ok());
        let err = s.camera(CameraId::from_raw(999)).unwrap_err();
        assert_eq!(err.code(), SceneErrorCode::MissingCamera);
    }

    #[test]
    fn light_query_returns_present_and_missing() {
        let mut s = Scene::new();
        let n = s.create_node(Transform::IDENTITY);
        let id = light_on(&mut s, n);
        assert!(s.light(id).is_ok());
        let err = s.light(LightId::from_raw(999)).unwrap_err();
        assert_eq!(err.code(), SceneErrorCode::MissingLight);
    }

    #[test]
    fn renderable_query_returns_present_and_missing() {
        let mut s = Scene::new();
        let n = s.create_node(Transform::IDENTITY);
        let id = renderable_on(&mut s, n);
        assert!(s.renderable(id).is_ok());
        let err = s.renderable(RenderableId::from_raw(999)).unwrap_err();
        assert_eq!(err.code(), SceneErrorCode::MissingRenderable);
    }

    #[test]
    fn component_iterators_yield_attached_components() {
        let mut s = Scene::new();
        let n = s.create_node(Transform::IDENTITY);
        cam_on(&mut s, n);
        light_on(&mut s, n);
        renderable_on(&mut s, n);
        assert_eq!(s.cameras_in_order().count(), 1);
        assert_eq!(s.lights_in_order().count(), 1);
        assert_eq!(s.renderables_in_order().count(), 1);
    }

    // ---------- remove_node ----------

    #[test]
    fn remove_missing_node_fails() {
        let mut s = Scene::new();
        let err = s.remove_node(SceneNodeId::from_raw(7)).unwrap_err();
        assert_eq!(err.code(), SceneErrorCode::MissingNode);
    }

    #[test]
    fn remove_child_node_detaches_from_parent_and_drops_components() {
        // Removing a node that HAS a parent exercises the parent-detach
        // arm; the node also carries camera/light/renderable so the three
        // `retain` calls drop them.
        let mut s = Scene::new();
        let p = s.create_node(Transform::IDENTITY);
        let c = s.create_node(Transform::IDENTITY);
        s.set_parent(c, p).unwrap();
        cam_on(&mut s, c);
        light_on(&mut s, c);
        renderable_on(&mut s, c);
        assert_eq!(s.camera_count(), 1);

        s.remove_node(c).unwrap();
        assert!(s.node(c).is_err());
        // Parent no longer lists the removed child.
        assert!(s.node(p).unwrap().children().is_empty());
        // Components attached to the removed node are gone.
        assert_eq!(s.camera_count(), 0);
        assert_eq!(s.light_count(), 0);
        assert_eq!(s.renderable_count(), 0);
    }

    // ---------- set_local ----------

    #[test]
    fn set_local_missing_node_fails() {
        let mut s = Scene::new();
        let err = s
            .set_local(SceneNodeId::from_raw(5), Transform::IDENTITY)
            .unwrap_err();
        assert_eq!(err.code(), SceneErrorCode::MissingNode);
    }

    // ---------- set_parent ----------

    #[test]
    fn set_parent_missing_child_fails() {
        let mut s = Scene::new();
        let p = s.create_node(Transform::IDENTITY);
        let err = s
            .set_parent(SceneNodeId::from_raw(999), p)
            .unwrap_err();
        assert_eq!(err.code(), SceneErrorCode::MissingNode);
    }

    #[test]
    fn reparenting_detaches_from_old_parent() {
        // First parent assignment leaves child with no old parent (None
        // arm); the second reparent exercises the old-parent Some arm.
        let mut s = Scene::new();
        let p1 = s.create_node(Transform::IDENTITY);
        let p2 = s.create_node(Transform::IDENTITY);
        let c = s.create_node(Transform::IDENTITY);
        s.set_parent(c, p1).unwrap();
        assert!(s.node(p1).unwrap().children().contains(&c));
        s.set_parent(c, p2).unwrap();
        assert_eq!(s.node(c).unwrap().parent(), Some(p2));
        assert!(s.node(p1).unwrap().children().is_empty());
        assert!(s.node(p2).unwrap().children().contains(&c));
    }

    // ---------- clear_parent ----------

    #[test]
    fn clear_parent_missing_child_fails() {
        let mut s = Scene::new();
        let err = s.clear_parent(SceneNodeId::from_raw(5)).unwrap_err();
        assert_eq!(err.code(), SceneErrorCode::MissingNode);
    }

    #[test]
    fn clear_parent_on_root_is_a_noop() {
        // A root has no old parent: the `if let Some(old)` arm is skipped.
        let mut s = Scene::new();
        let n = s.create_node(Transform::IDENTITY);
        s.clear_parent(n).unwrap();
        assert!(s.node(n).unwrap().parent().is_none());
    }

    // ---------- propagation ----------

    #[test]
    fn update_world_transforms_scales_through_hierarchy() {
        let mut s = Scene::new();
        let p = s.create_node(Transform::from_translation(Vec3::new(1.0, 0.0, 0.0)));
        let c = s.create_node(Transform::from_translation(Vec3::new(2.0, 0.0, 0.0)));
        let g = s.create_node(Transform::from_translation(Vec3::new(3.0, 0.0, 0.0)));
        s.set_parent(c, p).unwrap();
        s.set_parent(g, c).unwrap();
        s.update_world_transforms().unwrap();
        // Translations accumulate down the chain: 1 + 2 + 3 = 6.
        assert_eq!(s.node(g).unwrap().world().translation.x, 6.0);
    }

    #[test]
    fn update_world_transforms_is_idempotent() {
        let mut s = Scene::new();
        let p = s.create_node(Transform::from_translation(Vec3::new(1.0, 0.0, 0.0)));
        let c = s.create_node(Transform::from_translation(Vec3::new(0.0, 2.0, 0.0)));
        s.set_parent(c, p).unwrap();
        s.update_world_transforms().unwrap();
        let first = s.node(c).unwrap().world();
        s.update_world_transforms().unwrap();
        let second = s.node(c).unwrap().world();
        assert_eq!(first, second);
    }

    // ---------- component removal ----------

    #[test]
    fn remove_light_present_and_missing() {
        let mut s = Scene::new();
        let n = s.create_node(Transform::IDENTITY);
        let id = light_on(&mut s, n);
        s.remove_light(id).unwrap();
        let err = s.remove_light(id).unwrap_err();
        assert_eq!(err.code(), SceneErrorCode::MissingLight);
    }

    #[test]
    fn remove_renderable_present_and_missing() {
        let mut s = Scene::new();
        let n = s.create_node(Transform::IDENTITY);
        let id = renderable_on(&mut s, n);
        s.remove_renderable(id).unwrap();
        let err = s.remove_renderable(id).unwrap_err();
        assert_eq!(err.code(), SceneErrorCode::MissingRenderable);
    }

    #[test]
    fn remove_camera_present_succeeds() {
        let mut s = Scene::new();
        let n = s.create_node(Transform::IDENTITY);
        let id = cam_on(&mut s, n);
        s.remove_camera(id).unwrap();
        assert_eq!(s.camera_count(), 0);
    }

    // ---------- visibility ----------

    #[test]
    fn set_renderable_visible_present_and_missing() {
        let mut s = Scene::new();
        let n = s.create_node(Transform::IDENTITY);
        let id = renderable_on(&mut s, n);
        s.set_renderable_visible(id, false).unwrap();
        assert!(!s.renderable(id).unwrap().visible());
        s.set_renderable_visible(id, true).unwrap();
        assert!(s.renderable(id).unwrap().visible());
        let err = s
            .set_renderable_visible(RenderableId::from_raw(999), true)
            .unwrap_err();
        assert_eq!(err.code(), SceneErrorCode::MissingRenderable);
    }

    // ---------- Default ----------

    #[test]
    fn default_scene_is_empty() {
        let s = Scene::default();
        assert_eq!(s.node_count(), 0);
    }

    // ---------- defensive / invariant-guard arms ----------
    //
    // The following tests build *deliberately inconsistent* graphs through
    // the crate-internal API (private `nodes` map + `SceneNode` mutators)
    // to exercise the defensive arms that intact public-API invariants can
    // never reach: dangling parent ids, dangling child ids, and the
    // pre-existing-cycle guard in `would_introduce_cycle`. These are NOT
    // reachable through `SceneApi`; they protect against future internal
    // bugs and must still execute under test.

    /// Insert a node directly into the private map at a chosen id, keeping
    /// `next_node_id` ahead of it.
    fn raw_insert(s: &mut Scene, raw: u64, node: SceneNode) -> SceneNodeId {
        let id = SceneNodeId::from_raw(raw);
        s.nodes.insert(id, node);
        s.next_node_id = s.next_node_id.max(raw + 1);
        id
    }

    #[test]
    fn remove_node_with_dangling_parent_skips_parent_detach() {
        // Node records a parent that does not exist: the inner
        // `if let Some(parent_node) = get_mut(&pid)` takes its None arm.
        let mut s = Scene::new();
        let mut child = SceneNode::new(Transform::IDENTITY);
        child.set_parent(Some(SceneNodeId::from_raw(404)));
        let c = raw_insert(&mut s, 1, child);
        s.remove_node(c).unwrap();
        assert!(s.node(c).is_err());
    }

    #[test]
    fn remove_node_with_dangling_child_skips_child_detach() {
        // Node lists a child id that is not in the map: the
        // `if let Some(child_node) = get_mut(&child)` takes its None arm.
        let mut s = Scene::new();
        let mut parent = SceneNode::new(Transform::IDENTITY);
        parent.add_child(SceneNodeId::from_raw(404));
        let p = raw_insert(&mut s, 1, parent);
        s.remove_node(p).unwrap();
        assert!(s.node(p).is_err());
    }

    #[test]
    fn set_parent_with_dangling_old_parent_skips_old_detach() {
        // child's recorded old parent does not exist: the inner
        // `if let Some(parent_node) = get_mut(&old)` of set_parent takes
        // its None arm while the reparent still completes.
        let mut s = Scene::new();
        let mut child = SceneNode::new(Transform::IDENTITY);
        child.set_parent(Some(SceneNodeId::from_raw(404)));
        let c = raw_insert(&mut s, 1, child);
        let p = s.create_node(Transform::IDENTITY);
        s.set_parent(c, p).unwrap();
        assert_eq!(s.node(c).unwrap().parent(), Some(p));
    }

    #[test]
    fn clear_parent_with_dangling_old_parent_skips_old_detach() {
        // child's recorded old parent does not exist: the inner
        // `if let Some(parent_node) = get_mut(&old)` of clear_parent takes
        // its None arm while the clear still completes.
        let mut s = Scene::new();
        let mut child = SceneNode::new(Transform::IDENTITY);
        child.set_parent(Some(SceneNodeId::from_raw(404)));
        let c = raw_insert(&mut s, 1, child);
        s.clear_parent(c).unwrap();
        assert!(s.node(c).unwrap().parent().is_none());
    }

    #[test]
    fn would_introduce_cycle_detects_preexisting_cycle() {
        // Hand-build a 2-node cycle (a.parent = b, b.parent = a) that the
        // public API can never produce, so the parent-walk in
        // `would_introduce_cycle` revisits a node and hits its
        // visited-guard `return true`.
        let mut s = Scene::new();
        let mut na = SceneNode::new(Transform::IDENTITY);
        let mut nb = SceneNode::new(Transform::IDENTITY);
        let a = SceneNodeId::from_raw(1);
        let b = SceneNodeId::from_raw(2);
        na.set_parent(Some(b));
        nb.set_parent(Some(a));
        raw_insert(&mut s, 1, na);
        raw_insert(&mut s, 2, nb);
        // Walk upward from `b` looking for an unrelated target; the cycle
        // a<->b is revisited, tripping the guard.
        assert!(s.would_introduce_cycle(SceneNodeId::from_raw(99), b));
    }

    #[test]
    fn update_world_transforms_errors_on_dangling_child() {
        // A root node lists a child id that does not exist: propagation's
        // child lookup `get(&child_id).ok_or_else(...)` returns the
        // hierarchy-update-failed error.
        let mut s = Scene::new();
        let mut root = SceneNode::new(Transform::IDENTITY);
        root.add_child(SceneNodeId::from_raw(404));
        raw_insert(&mut s, 1, root);
        let err = s.update_world_transforms().unwrap_err();
        assert_eq!(err.code(), SceneErrorCode::HierarchyUpdateFailed);
    }

    #[test]
    fn advance_propagates_update_world_transforms_error() {
        // `SceneApi::advance` runs `update_world_transforms?` on an active
        // host frame with runtime steps. A corrupt scene (dangling child)
        // makes that call fail, exercising the `?` error arm in `advance`.
        // This is the only place a corrupt scene can be fed to `advance`,
        // and the private `nodes` map is reachable only from this module.
        use crate::scene_api::SceneApi;
        use axiom_frame::FrameApi;
        use axiom_host::{
            HostBoundaryConfig, HostFrameInput, HostFrameReport, HostLifecycleSignal,
            HostLifecycleState, HostStepPlan, HostViewport,
        };

        let m = math();
        let vp = HostViewport::new(&m, 100, 100, 1.0).unwrap();
        let cfg = HostBoundaryConfig::new(1_000, 5).unwrap();
        let visible = HostLifecycleState::initial().apply(HostLifecycleSignal::Started);
        // elapsed >= fixed_step => at least one runtime step, not skipped.
        let input = HostFrameInput::new(1, 1_000, vp);
        let plan = HostStepPlan::build(&input, &cfg, &visible, 0);
        assert!(!plan.is_skipped());
        assert!(plan.steps() > 0);
        let report = HostFrameReport::new(
            input.sequence(),
            plan,
            plan.steps(),
            Vec::new(),
            vp,
            visible,
        );
        let frame_api = FrameApi::new();
        let engine_frame = frame_api
            .engine_frame_from_host_report(&report, 1_000, Vec::new())
            .unwrap();
        let ctx = frame_api.frame_context(&engine_frame);

        let mut s = Scene::new();
        let mut root = SceneNode::new(Transform::IDENTITY);
        root.add_child(SceneNodeId::from_raw(404));
        raw_insert(&mut s, 1, root);

        let err = SceneApi::new().advance(&mut s, &ctx).unwrap_err();
        assert_eq!(err.code(), SceneErrorCode::HierarchyUpdateFailed);
    }
}
