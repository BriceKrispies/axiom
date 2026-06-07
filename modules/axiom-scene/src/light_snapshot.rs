//! One light entry inside a [`crate::SceneSnapshot`].

use axiom_kernel::Ratio;
use axiom_math::Vec3;

use crate::light_kind::LightKind;
use crate::scene_node_id::SceneNodeId;

/// One light entry in a deterministic scene snapshot, keyed by its node.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LightSnapshot {
    node: SceneNodeId,
    kind: LightKind,
    color: Vec3,
    intensity: Ratio,
}

impl LightSnapshot {
    pub const fn new(node: SceneNodeId, kind: LightKind, color: Vec3, intensity: Ratio) -> Self {
        LightSnapshot {
            node,
            kind,
            color,
            intensity,
        }
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

    pub const fn intensity(&self) -> Ratio {
        self.intensity
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rat(x: f32) -> Ratio {
        Ratio::new(x).unwrap()
    }

    #[test]
    fn accessors_round_trip_constructed_values() {
        let s = LightSnapshot::new(SceneNodeId::from_raw(7), LightKind::Point, Vec3::new(0.5, 0.5, 0.5), rat(3.0));
        assert_eq!(s.node().raw(), 7);
        assert_eq!(s.kind(), LightKind::Point);
        assert_eq!(s.color().x, 0.5);
        assert_eq!(s.intensity().get(), 3.0);
    }

    #[test]
    fn equality_requires_all_fields() {
        let a = LightSnapshot::new(SceneNodeId::from_raw(1), LightKind::Directional, Vec3::ONE, rat(1.0));
        let b = LightSnapshot::new(SceneNodeId::from_raw(1), LightKind::Directional, Vec3::ONE, rat(1.0));
        let c = LightSnapshot::new(SceneNodeId::from_raw(1), LightKind::Point, Vec3::ONE, rat(1.0));
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
