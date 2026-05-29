//! One light entry inside a [`crate::SceneSnapshot`].

use axiom_math::Vec3;

use crate::light_id::LightId;
use crate::light_kind::LightKind;
use crate::scene_node_id::SceneNodeId;

/// One light entry in a deterministic scene snapshot.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LightSnapshot {
    id: LightId,
    node: SceneNodeId,
    kind: LightKind,
    color: Vec3,
    intensity: f32,
}

impl LightSnapshot {
    pub const fn new(
        id: LightId,
        node: SceneNodeId,
        kind: LightKind,
        color: Vec3,
        intensity: f32,
    ) -> Self {
        LightSnapshot {
            id,
            node,
            kind,
            color,
            intensity,
        }
    }

    pub const fn id(&self) -> LightId {
        self.id
    }

    pub const fn node(&self) -> SceneNodeId {
        self.node
    }

    pub const fn kind(&self) -> LightKind {
        self.kind
    }

    pub const fn color(&self) -> Vec3 {
        self.color
    }

    pub const fn intensity(&self) -> f32 {
        self.intensity
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accessors_round_trip_constructed_values() {
        let s = LightSnapshot::new(
            LightId::from_raw(2),
            SceneNodeId::from_raw(7),
            LightKind::Point,
            Vec3::new(0.5, 0.5, 0.5),
            3.0,
        );
        assert_eq!(s.id().raw(), 2);
        assert_eq!(s.node().raw(), 7);
        assert_eq!(s.kind(), LightKind::Point);
        assert_eq!(s.color().x, 0.5);
        assert_eq!(s.intensity(), 3.0);
    }

    #[test]
    fn equality_requires_all_fields() {
        let a = LightSnapshot::new(
            LightId::from_raw(1),
            SceneNodeId::from_raw(1),
            LightKind::Directional,
            Vec3::ONE,
            1.0,
        );
        let b = LightSnapshot::new(
            LightId::from_raw(1),
            SceneNodeId::from_raw(1),
            LightKind::Directional,
            Vec3::ONE,
            1.0,
        );
        let c = LightSnapshot::new(
            LightId::from_raw(1),
            SceneNodeId::from_raw(1),
            LightKind::Point,
            Vec3::ONE,
            1.0,
        );
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
