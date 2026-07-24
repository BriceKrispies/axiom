//! The active-camera arm of the [`SceneApi`] facade: finding and collapsing the
//! scene to a single camera node. A child module so neither `impl SceneApi`
//! block exceeds the engine's impl-block size budget, and so a composition root
//! re-authoring a moving camera every frame can *reuse* one node instead of
//! spawning a fresh one per frame — the unbounded scene-node leak that slowly
//! starved a long-running session.

use super::SceneApi;
use crate::scene_node_id::SceneNodeId;

impl SceneApi {
    /// The first active camera node (ascending id order), if any — the sole
    /// camera the render path uses. A cheap column read (no allocation), so the
    /// app can reuse this node every frame — reposition it, replace its
    /// intrinsics — instead of churning a fresh camera node per frame.
    pub fn first_camera_node(&self) -> Option<SceneNodeId> {
        self.scene.first_camera_node()
    }

    /// Despawn every camera node except `keep`, collapsing the scene back to a
    /// single active camera. Churn-free when `keep` is already the only camera
    /// (the steady state), so it is safe to call every frame.
    pub fn despawn_cameras_except(&mut self, keep: SceneNodeId) {
        self.scene.despawn_cameras_except(keep)
    }
}

#[cfg(test)]
mod tests {
    use super::SceneApi;
    use crate::scene_node_id::SceneNodeId;
    use axiom_kernel::{Meters, Radians, Ratio};
    use axiom_math::MathApi;

    fn api() -> SceneApi {
        SceneApi::new()
    }

    fn add_camera(s: &mut SceneApi, node: SceneNodeId) {
        s.add_perspective_camera(
            &MathApi::new(),
            node,
            Radians::new(1.0).unwrap(),
            Ratio::new(1.0).unwrap(),
            Meters::new(0.1).unwrap(),
            Meters::new(100.0).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn first_camera_node_reports_the_sole_camera_or_none() {
        let mut s = api();
        assert_eq!(s.first_camera_node(), None);
        let node = s.create_node();
        add_camera(&mut s, node);
        assert_eq!(s.first_camera_node(), Some(node));
    }

    #[test]
    fn despawn_cameras_except_collapses_to_the_kept_camera_and_is_a_noop_when_already_sole() {
        let mut s = api();
        let keep = s.create_node();
        let extra = s.create_node();
        add_camera(&mut s, keep);
        add_camera(&mut s, extra);
        // Two cameras → keep `keep`, fully despawn the extra NODE (not just its
        // camera component): `extra`'s local transform is gone afterward.
        s.despawn_cameras_except(keep);
        assert_eq!(s.first_camera_node(), Some(keep));
        assert!(s.local_transform(extra).is_err());
        // Already sole → churn-free no-op; `keep` still the camera and still live.
        s.despawn_cameras_except(keep);
        assert_eq!(s.first_camera_node(), Some(keep));
        assert!(s.local_transform(keep).is_ok());
    }
}
