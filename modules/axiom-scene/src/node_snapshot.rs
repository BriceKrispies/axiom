//! One node entry inside a [`crate::SceneSnapshot`].

use axiom_math::Transform;

use crate::scene_node_id::SceneNodeId;

/// One node entry in a deterministic scene snapshot: id, parent id (if
/// any), and the node's local + world transforms.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NodeSnapshot {
    id: SceneNodeId,
    parent: Option<SceneNodeId>,
    local: Transform,
    world: Transform,
}

impl NodeSnapshot {
    pub const fn new(
        id: SceneNodeId,
        parent: Option<SceneNodeId>,
        local: Transform,
        world: Transform,
    ) -> Self {
        NodeSnapshot {
            id,
            parent,
            local,
            world,
        }
    }

    pub const fn id(&self) -> SceneNodeId {
        self.id
    }

    pub const fn parent(&self) -> Option<SceneNodeId> {
        self.parent
    }

    pub const fn local(&self) -> Transform {
        self.local
    }

    pub const fn world(&self) -> Transform {
        self.world
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_math::Vec3;

    #[test]
    fn accessors_round_trip_constructed_values() {
        let s = NodeSnapshot::new(
            SceneNodeId::from_raw(3),
            Some(SceneNodeId::from_raw(1)),
            Transform::IDENTITY,
            Transform::from_translation(Vec3::new(1.0, 0.0, 0.0)),
        );
        assert_eq!(s.id().raw(), 3);
        assert_eq!(s.parent().unwrap().raw(), 1);
        assert_eq!(s.world().translation.x, 1.0);
    }

    #[test]
    fn equality_requires_all_fields() {
        let a = NodeSnapshot::new(
            SceneNodeId::from_raw(1),
            None,
            Transform::IDENTITY,
            Transform::IDENTITY,
        );
        let b = NodeSnapshot::new(
            SceneNodeId::from_raw(1),
            None,
            Transform::IDENTITY,
            Transform::IDENTITY,
        );
        let c = NodeSnapshot::new(
            SceneNodeId::from_raw(2),
            None,
            Transform::IDENTITY,
            Transform::IDENTITY,
        );
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
