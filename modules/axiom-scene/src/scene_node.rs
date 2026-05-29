//! One node of the scene graph: local + cached world transforms + topology.

use std::collections::BTreeSet;

use axiom_math::Transform;

use crate::scene_node_id::SceneNodeId;

/// One scene-graph node.
///
/// Plain data: a local transform, a cached world transform (kept fresh by
/// [`crate::SceneApi::update_world_transforms`]), the optional parent id,
/// and the deterministically-ordered set of children. Children are
/// stored in a `BTreeSet` so iteration is by ascending [`SceneNodeId`] —
/// the same order across runs, the same order across platforms.
#[derive(Debug, Clone)]
pub struct SceneNode {
    parent: Option<SceneNodeId>,
    children: BTreeSet<SceneNodeId>,
    local: Transform,
    world: Transform,
}

impl SceneNode {
    /// Construct a fresh node with `local` as its local transform. The
    /// world transform starts equal to `local`; propagation will replace
    /// it once the node is parented and
    /// [`crate::SceneApi::update_world_transforms`] is called.
    pub fn new(local: Transform) -> Self {
        SceneNode {
            parent: None,
            children: BTreeSet::new(),
            local,
            world: local,
        }
    }

    pub fn parent(&self) -> Option<SceneNodeId> {
        self.parent
    }

    pub fn children(&self) -> &BTreeSet<SceneNodeId> {
        &self.children
    }

    pub fn local(&self) -> Transform {
        self.local
    }

    pub fn world(&self) -> Transform {
        self.world
    }

    pub(crate) fn set_parent(&mut self, parent: Option<SceneNodeId>) {
        self.parent = parent;
    }

    pub(crate) fn add_child(&mut self, child: SceneNodeId) {
        self.children.insert(child);
    }

    pub(crate) fn remove_child(&mut self, child: SceneNodeId) {
        self.children.remove(&child);
    }

    pub(crate) fn set_local(&mut self, local: Transform) {
        self.local = local;
    }

    pub(crate) fn set_world(&mut self, world: Transform) {
        self.world = world;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_math::Vec3;

    #[test]
    fn new_starts_with_identity_world_when_local_is_identity() {
        let n = SceneNode::new(Transform::IDENTITY);
        assert!(n.parent().is_none());
        assert!(n.children().is_empty());
    }

    #[test]
    fn world_starts_equal_to_local() {
        let local = Transform::from_translation(Vec3::new(1.0, 2.0, 3.0));
        let n = SceneNode::new(local);
        assert_eq!(n.world().translation.x, 1.0);
    }

    #[test]
    fn children_iterate_in_ascending_id_order() {
        let mut n = SceneNode::new(Transform::IDENTITY);
        n.add_child(SceneNodeId::from_raw(3));
        n.add_child(SceneNodeId::from_raw(1));
        n.add_child(SceneNodeId::from_raw(2));
        let ids: Vec<u64> = n.children().iter().map(|c| c.raw()).collect();
        assert_eq!(ids, vec![1, 2, 3]);
    }

    #[test]
    fn set_and_clear_parent_round_trip() {
        let mut n = SceneNode::new(Transform::IDENTITY);
        n.set_parent(Some(SceneNodeId::from_raw(5)));
        assert_eq!(n.parent(), Some(SceneNodeId::from_raw(5)));
        n.set_parent(None);
        assert!(n.parent().is_none());
    }

    #[test]
    fn set_local_updates_local_transform_only() {
        let mut n = SceneNode::new(Transform::IDENTITY);
        let new_local = Transform::from_translation(Vec3::new(7.0, 0.0, 0.0));
        n.set_local(new_local);
        assert_eq!(n.local().translation.x, 7.0);
        // World is still identity until propagation runs.
        assert_eq!(n.world().translation.x, 0.0);
    }
}
