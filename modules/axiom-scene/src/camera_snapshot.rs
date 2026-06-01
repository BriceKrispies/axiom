//! One camera entry inside a [`crate::SceneSnapshot`].

use crate::scene_node_id::SceneNodeId;

/// One camera entry in a deterministic scene snapshot: the node it is attached
/// to and its intrinsic projection parameters. (A camera is keyed by its node,
/// so there is no separate camera id.)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraSnapshot {
    node: SceneNodeId,
    fovy_radians: f32,
    aspect: f32,
    near: f32,
    far: f32,
}

impl CameraSnapshot {
    pub const fn new(
        node: SceneNodeId,
        fovy_radians: f32,
        aspect: f32,
        near: f32,
        far: f32,
    ) -> Self {
        CameraSnapshot {
            node,
            fovy_radians,
            aspect,
            near,
            far,
        }
    }

    pub const fn node(&self) -> SceneNodeId {
        self.node
    }

    pub const fn fovy_radians(&self) -> f32 {
        self.fovy_radians
    }

    pub const fn aspect(&self) -> f32 {
        self.aspect
    }

    pub const fn near(&self) -> f32 {
        self.near
    }

    pub const fn far(&self) -> f32 {
        self.far
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accessors_round_trip_constructed_values() {
        let s = CameraSnapshot::new(SceneNodeId::from_raw(5), 1.5, 16.0 / 9.0, 0.1, 100.0);
        assert_eq!(s.node().raw(), 5);
        assert_eq!(s.fovy_radians(), 1.5);
        assert_eq!(s.aspect(), 16.0 / 9.0);
        assert_eq!(s.near(), 0.1);
        assert_eq!(s.far(), 100.0);
    }

    #[test]
    fn equality_requires_all_fields() {
        let a = CameraSnapshot::new(SceneNodeId::from_raw(1), 1.0, 1.0, 0.1, 100.0);
        let b = CameraSnapshot::new(SceneNodeId::from_raw(1), 1.0, 1.0, 0.1, 100.0);
        let c = CameraSnapshot::new(SceneNodeId::from_raw(2), 1.0, 1.0, 0.1, 100.0);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
