//! One camera entry inside a [`crate::SceneSnapshot`].

use crate::camera_id::CameraId;
use crate::scene_node_id::SceneNodeId;

/// One camera entry in a deterministic scene snapshot: the camera id,
/// the node it is attached to, and its intrinsic projection parameters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraSnapshot {
    id: CameraId,
    node: SceneNodeId,
    fovy_radians: f32,
    aspect: f32,
    near: f32,
    far: f32,
}

impl CameraSnapshot {
    pub const fn new(
        id: CameraId,
        node: SceneNodeId,
        fovy_radians: f32,
        aspect: f32,
        near: f32,
        far: f32,
    ) -> Self {
        CameraSnapshot {
            id,
            node,
            fovy_radians,
            aspect,
            near,
            far,
        }
    }

    pub const fn id(&self) -> CameraId {
        self.id
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
        let s = CameraSnapshot::new(
            CameraId::from_raw(2),
            SceneNodeId::from_raw(5),
            1.5,
            1.0,
            0.1,
            100.0,
        );
        assert_eq!(s.id().raw(), 2);
        assert_eq!(s.node().raw(), 5);
        assert_eq!(s.fovy_radians(), 1.5);
        assert_eq!(s.aspect(), 1.0);
        assert_eq!(s.near(), 0.1);
        assert_eq!(s.far(), 100.0);
    }

    #[test]
    fn aspect_is_the_constructed_value() {
        // Kills `aspect -> 1.0`: use an aspect distinct from 1.0.
        let s = CameraSnapshot::new(
            CameraId::from_raw(1),
            SceneNodeId::from_raw(1),
            1.5,
            16.0 / 9.0,
            0.1,
            100.0,
        );
        assert_eq!(s.aspect(), 16.0 / 9.0);
    }

    #[test]
    fn equality_requires_all_fields() {
        let a = CameraSnapshot::new(
            CameraId::from_raw(1),
            SceneNodeId::from_raw(1),
            1.0,
            1.0,
            0.1,
            100.0,
        );
        let b = CameraSnapshot::new(
            CameraId::from_raw(1),
            SceneNodeId::from_raw(1),
            1.0,
            1.0,
            0.1,
            100.0,
        );
        let c = CameraSnapshot::new(
            CameraId::from_raw(2),
            SceneNodeId::from_raw(1),
            1.0,
            1.0,
            0.1,
            100.0,
        );
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
