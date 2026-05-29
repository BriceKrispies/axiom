//! The single public facade of the `axiom-scene` module.

use axiom_frame::FrameContext;
use axiom_math::{Mat4, MathApi, Transform, Vec3};

use crate::camera::Camera;
use crate::camera_id::CameraId;
use crate::light::Light;
use crate::light_id::LightId;
use crate::material_ref::MaterialRef;
use crate::mesh_ref::MeshRef;
use crate::renderable::Renderable;
use crate::renderable_id::RenderableId;
use crate::scene::Scene;
use crate::scene_node_id::SceneNodeId;
use crate::scene_result::SceneResult;
use crate::scene_snapshot::SceneSnapshot;

/// The only public export of `axiom-scene`.
///
/// `SceneApi` is a zero-sized facade. Every scene mutation —
/// creating/removing nodes, parenting, transforming, adding cameras /
/// lights / renderables, propagating world transforms, taking snapshots
/// — goes through one of its methods. Future apps and engine systems
/// reach scene state **only** through this facade; the internal types
/// (`Scene`, `Camera`, `Light`, `Renderable`, snapshots) are public
/// inside the crate but never re-exported from `lib.rs`.
///
/// The facade is also where Layer 02 ([`MathApi`]) and Layer 04
/// ([`FrameContext`]) integration lives — see [`Self::advance`].
#[derive(Debug, Clone, Copy, Default)]
pub struct SceneApi {
    _sealed: (),
}

impl SceneApi {
    /// Construct the facade.
    pub const fn new() -> Self {
        SceneApi { _sealed: () }
    }

    // --- Scene construction ---

    /// Create a fresh, empty scene.
    pub fn empty_scene(&self) -> Scene {
        Scene::new()
    }

    // --- Nodes ---

    /// Create a node with the identity local transform.
    pub fn create_node(&self, scene: &mut Scene) -> SceneNodeId {
        scene.create_node(Transform::IDENTITY)
    }

    /// Create a node with an explicit local transform.
    pub fn create_node_with_transform(
        &self,
        scene: &mut Scene,
        local: Transform,
    ) -> SceneNodeId {
        scene.create_node(local)
    }

    /// Remove a node, detaching its children (they become roots) and
    /// removing any cameras/lights/renderables that were attached to it.
    pub fn remove_node(&self, scene: &mut Scene, id: SceneNodeId) -> SceneResult<()> {
        scene.remove_node(id)
    }

    /// Set a node's local transform.
    pub fn set_local_transform(
        &self,
        scene: &mut Scene,
        id: SceneNodeId,
        transform: Transform,
    ) -> SceneResult<()> {
        scene.set_local(id, transform)
    }

    /// Get a node's local transform.
    pub fn local_transform(
        &self,
        scene: &Scene,
        id: SceneNodeId,
    ) -> SceneResult<Transform> {
        Ok(scene.node(id)?.local())
    }

    /// Get a node's cached world transform. Returns the value computed
    /// by the most recent [`Self::update_world_transforms`] call; the
    /// caller is responsible for keeping that fresh.
    pub fn world_transform(
        &self,
        scene: &Scene,
        id: SceneNodeId,
    ) -> SceneResult<Transform> {
        Ok(scene.node(id)?.world())
    }

    // --- Hierarchy ---

    /// Make `child` a child of `parent`. Rejects self-parenting,
    /// cycles, and missing ids.
    pub fn set_parent(
        &self,
        scene: &mut Scene,
        child: SceneNodeId,
        parent: SceneNodeId,
    ) -> SceneResult<()> {
        scene.set_parent(child, parent)
    }

    /// Detach `child` from its parent (if any), leaving it as a root.
    /// Local transform is preserved.
    pub fn clear_parent(
        &self,
        scene: &mut Scene,
        child: SceneNodeId,
    ) -> SceneResult<()> {
        scene.clear_parent(child)
    }

    // --- Transform propagation ---

    /// Recompute every node's world transform deterministically.
    /// Parents are always updated before children, and siblings are
    /// processed in ascending [`SceneNodeId`] order.
    pub fn update_world_transforms(&self, scene: &mut Scene) -> SceneResult<()> {
        scene.update_world_transforms()
    }

    // --- Cameras ---

    /// Add a perspective camera attached to `node`. Intrinsic
    /// validation is delegated to [`MathApi::mat4_perspective`].
    pub fn add_perspective_camera(
        &self,
        math: &MathApi,
        scene: &mut Scene,
        node: SceneNodeId,
        fovy_radians: f32,
        aspect: f32,
        near: f32,
        far: f32,
    ) -> SceneResult<CameraId> {
        let camera = Camera::perspective(math, node, fovy_radians, aspect, near, far)?;
        scene.add_camera(camera)
    }

    /// Remove a camera by id.
    pub fn remove_camera(&self, scene: &mut Scene, id: CameraId) -> SceneResult<()> {
        scene.remove_camera(id)
    }

    /// Compute the projection matrix for an existing camera.
    pub fn camera_projection_matrix(
        &self,
        math: &MathApi,
        scene: &Scene,
        id: CameraId,
    ) -> SceneResult<Mat4> {
        scene.camera(id)?.projection_matrix(math)
    }

    // --- Lights ---

    /// Add a directional light attached to `node`.
    pub fn add_directional_light(
        &self,
        math: &MathApi,
        scene: &mut Scene,
        node: SceneNodeId,
        color: Vec3,
        intensity: f32,
    ) -> SceneResult<LightId> {
        let light = Light::directional(math, node, color, intensity)?;
        scene.add_light(light)
    }

    /// Add a point light attached to `node`.
    pub fn add_point_light(
        &self,
        math: &MathApi,
        scene: &mut Scene,
        node: SceneNodeId,
        color: Vec3,
        intensity: f32,
    ) -> SceneResult<LightId> {
        let light = Light::point(math, node, color, intensity)?;
        scene.add_light(light)
    }

    /// Remove a light by id.
    pub fn remove_light(&self, scene: &mut Scene, id: LightId) -> SceneResult<()> {
        scene.remove_light(id)
    }

    // --- Renderables ---

    /// Construct an opaque [`MeshRef`].
    pub const fn mesh_ref(&self, raw: u64) -> MeshRef {
        MeshRef::from_raw(raw)
    }

    /// Construct an opaque [`MaterialRef`].
    pub const fn material_ref(&self, raw: u64) -> MaterialRef {
        MaterialRef::from_raw(raw)
    }

    /// Add a renderable attached to `node`.
    pub fn add_renderable(
        &self,
        scene: &mut Scene,
        node: SceneNodeId,
        mesh: MeshRef,
        material: MaterialRef,
    ) -> SceneResult<RenderableId> {
        let renderable = Renderable::new(node, mesh, material)?;
        scene.add_renderable(renderable)
    }

    /// Remove a renderable by id.
    pub fn remove_renderable(
        &self,
        scene: &mut Scene,
        id: RenderableId,
    ) -> SceneResult<()> {
        scene.remove_renderable(id)
    }

    /// Toggle a renderable's visibility flag.
    pub fn set_renderable_visibility(
        &self,
        scene: &mut Scene,
        id: RenderableId,
        visible: bool,
    ) -> SceneResult<()> {
        scene.set_renderable_visible(id, visible)
    }

    // --- Snapshot ---

    /// Build a deterministic snapshot of the scene's current state.
    pub fn snapshot(&self, scene: &Scene) -> SceneSnapshot {
        SceneSnapshot::from_scene(scene)
    }

    // --- Frame / runtime integration ---

    /// Advance the scene for one engine frame.
    ///
    /// This is the small, opt-in Layer-04 integration: if `frame` is
    /// `Active` and ran at least one runtime step, world transforms
    /// are recomputed. If the frame was skipped (hidden / suspended /
    /// shutting down), the scene is left untouched. The method always
    /// returns the deterministic snapshot taken *after* whatever
    /// update happened, so callers can compare engine frames and scene
    /// snapshots side by side.
    pub fn advance(
        &self,
        scene: &mut Scene,
        frame: &FrameContext<'_>,
    ) -> SceneResult<SceneSnapshot> {
        if !frame.is_skipped() && frame.runtime_step_count() > 0 {
            self.update_world_transforms(scene)?;
        }
        Ok(self.snapshot(scene))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::light_kind::LightKind;
    use crate::scene_error_code::SceneErrorCode;

    fn math() -> MathApi {
        MathApi::new()
    }

    fn api() -> SceneApi {
        SceneApi::new()
    }

    // ---------- nodes / hierarchy ----------

    #[test]
    fn new_and_default_facades_are_equivalent() {
        let _ = SceneApi::new();
        let _ = SceneApi::default();
    }

    #[test]
    fn empty_scene_starts_empty() {
        let s = api().empty_scene();
        assert_eq!(s.node_count(), 0);
    }

    #[test]
    fn create_node_returns_monotonic_ids() {
        let mut s = api().empty_scene();
        let a = api().create_node(&mut s);
        let b = api().create_node(&mut s);
        assert!(a.raw() < b.raw());
    }

    #[test]
    fn create_node_with_transform_stores_the_local() {
        let mut s = api().empty_scene();
        let t = Transform::from_translation(Vec3::new(1.0, 2.0, 3.0));
        let id = api().create_node_with_transform(&mut s, t);
        assert_eq!(api().local_transform(&s, id).unwrap().translation.x, 1.0);
    }

    #[test]
    fn remove_node_works() {
        let mut s = api().empty_scene();
        let id = api().create_node(&mut s);
        api().remove_node(&mut s, id).unwrap();
        assert_eq!(s.node_count(), 0);
    }

    #[test]
    fn remove_missing_node_fails() {
        let mut s = api().empty_scene();
        let err = api().remove_node(&mut s, SceneNodeId::from_raw(99)).unwrap_err();
        assert_eq!(err.code(), SceneErrorCode::MissingNode);
    }

    #[test]
    fn set_and_get_local_transform_round_trip() {
        let mut s = api().empty_scene();
        let id = api().create_node(&mut s);
        let t = Transform::from_translation(Vec3::new(5.0, 0.0, 0.0));
        api().set_local_transform(&mut s, id, t).unwrap();
        assert_eq!(api().local_transform(&s, id).unwrap().translation.x, 5.0);
    }

    #[test]
    fn world_transform_starts_identity() {
        let mut s = api().empty_scene();
        let id = api().create_node(&mut s);
        assert_eq!(
            api().world_transform(&s, id).unwrap().translation.x,
            0.0
        );
    }

    #[test]
    fn set_parent_and_clear_parent_round_trip() {
        let mut s = api().empty_scene();
        let p = api().create_node(&mut s);
        let c = api().create_node(&mut s);
        api().set_parent(&mut s, c, p).unwrap();
        assert_eq!(s.node(c).unwrap().parent(), Some(p));
        api().clear_parent(&mut s, c).unwrap();
        assert!(s.node(c).unwrap().parent().is_none());
    }

    #[test]
    fn self_parenting_through_facade_fails() {
        let mut s = api().empty_scene();
        let n = api().create_node(&mut s);
        assert_eq!(
            api().set_parent(&mut s, n, n).unwrap_err().code(),
            SceneErrorCode::SelfParenting
        );
    }

    #[test]
    fn cycle_creation_through_facade_fails() {
        let mut s = api().empty_scene();
        let a = api().create_node(&mut s);
        let b = api().create_node(&mut s);
        api().set_parent(&mut s, b, a).unwrap();
        assert_eq!(
            api().set_parent(&mut s, a, b).unwrap_err().code(),
            SceneErrorCode::HierarchyCycle
        );
    }

    // ---------- transform propagation ----------

    #[test]
    fn update_world_transforms_propagates_translation() {
        let mut s = api().empty_scene();
        let p = api().create_node_with_transform(
            &mut s,
            Transform::from_translation(Vec3::new(1.0, 0.0, 0.0)),
        );
        let c = api().create_node_with_transform(
            &mut s,
            Transform::from_translation(Vec3::new(0.0, 2.0, 0.0)),
        );
        api().set_parent(&mut s, c, p).unwrap();
        api().update_world_transforms(&mut s).unwrap();
        let w = api().world_transform(&s, c).unwrap();
        assert_eq!(w.translation.x, 1.0);
        assert_eq!(w.translation.y, 2.0);
    }

    #[test]
    fn update_world_transforms_propagates_uniform_scale() {
        let mut s = api().empty_scene();
        let p = api().create_node_with_transform(
            &mut s,
            Transform::from_scale(Vec3::new(2.0, 2.0, 2.0)),
        );
        let c = api().create_node_with_transform(
            &mut s,
            Transform::from_translation(Vec3::new(1.0, 0.0, 0.0)),
        );
        api().set_parent(&mut s, c, p).unwrap();
        api().update_world_transforms(&mut s).unwrap();
        let w = api().world_transform(&s, c).unwrap();
        assert_eq!(w.translation.x, 2.0);
    }

    #[test]
    fn repeated_propagation_with_no_changes_is_idempotent() {
        let mut s = api().empty_scene();
        let p = api().create_node_with_transform(
            &mut s,
            Transform::from_translation(Vec3::new(1.0, 0.0, 0.0)),
        );
        let c = api().create_node_with_transform(
            &mut s,
            Transform::from_translation(Vec3::new(0.0, 1.0, 0.0)),
        );
        api().set_parent(&mut s, c, p).unwrap();
        api().update_world_transforms(&mut s).unwrap();
        let a = api().world_transform(&s, c).unwrap();
        api().update_world_transforms(&mut s).unwrap();
        let b = api().world_transform(&s, c).unwrap();
        assert_eq!(a.translation.x, b.translation.x);
        assert_eq!(a.translation.y, b.translation.y);
    }

    // ---------- cameras ----------

    #[test]
    fn add_perspective_camera_works() {
        let mut s = api().empty_scene();
        let n = api().create_node(&mut s);
        let id = api()
            .add_perspective_camera(&math(), &mut s, n, 1.5, 1.0, 0.1, 100.0)
            .unwrap();
        assert!(id.is_valid());
        assert_eq!(s.camera_count(), 1);
    }

    #[test]
    fn add_camera_to_missing_node_fails() {
        let mut s = api().empty_scene();
        let err = api()
            .add_perspective_camera(
                &math(),
                &mut s,
                SceneNodeId::from_raw(42),
                1.5,
                1.0,
                0.1,
                100.0,
            )
            .unwrap_err();
        assert_eq!(err.code(), SceneErrorCode::MissingNode);
    }

    #[test]
    fn camera_with_invalid_intrinsics_fails() {
        let mut s = api().empty_scene();
        let n = api().create_node(&mut s);
        let err = api()
            .add_perspective_camera(&math(), &mut s, n, 0.0, 1.0, 0.1, 100.0)
            .unwrap_err();
        assert_eq!(err.code(), SceneErrorCode::InvalidCameraParameters);
    }

    #[test]
    fn remove_camera_works() {
        let mut s = api().empty_scene();
        let n = api().create_node(&mut s);
        let id = api()
            .add_perspective_camera(&math(), &mut s, n, 1.5, 1.0, 0.1, 100.0)
            .unwrap();
        api().remove_camera(&mut s, id).unwrap();
        assert_eq!(s.camera_count(), 0);
    }

    #[test]
    fn camera_projection_matrix_is_deterministic() {
        let mut s = api().empty_scene();
        let n = api().create_node(&mut s);
        let id = api()
            .add_perspective_camera(&math(), &mut s, n, 1.5, 1.0, 0.1, 100.0)
            .unwrap();
        let a = api().camera_projection_matrix(&math(), &s, id).unwrap();
        let b = api().camera_projection_matrix(&math(), &s, id).unwrap();
        assert_eq!(a.as_cols_array(), b.as_cols_array());
    }

    // ---------- lights ----------

    #[test]
    fn add_directional_and_point_light_works() {
        let mut s = api().empty_scene();
        let n = api().create_node(&mut s);
        let _d = api()
            .add_directional_light(&math(), &mut s, n, Vec3::ONE, 1.0)
            .unwrap();
        let _p = api()
            .add_point_light(&math(), &mut s, n, Vec3::ONE, 1.0)
            .unwrap();
        assert_eq!(s.light_count(), 2);
    }

    #[test]
    fn light_attached_to_missing_node_fails() {
        let mut s = api().empty_scene();
        let err = api()
            .add_point_light(
                &math(),
                &mut s,
                SceneNodeId::from_raw(42),
                Vec3::ONE,
                1.0,
            )
            .unwrap_err();
        assert_eq!(err.code(), SceneErrorCode::MissingNode);
    }

    #[test]
    fn invalid_light_intensity_fails() {
        let mut s = api().empty_scene();
        let n = api().create_node(&mut s);
        let err = api()
            .add_directional_light(&math(), &mut s, n, Vec3::ONE, -1.0)
            .unwrap_err();
        assert_eq!(err.code(), SceneErrorCode::InvalidLightParameters);
    }

    #[test]
    fn remove_light_works() {
        let mut s = api().empty_scene();
        let n = api().create_node(&mut s);
        let id = api()
            .add_directional_light(&math(), &mut s, n, Vec3::ONE, 1.0)
            .unwrap();
        api().remove_light(&mut s, id).unwrap();
        assert_eq!(s.light_count(), 0);
    }

    // ---------- renderables ----------

    #[test]
    fn mesh_and_material_refs_round_trip() {
        let m = api().mesh_ref(11);
        let mat = api().material_ref(22);
        assert_eq!(m.raw(), 11);
        assert_eq!(mat.raw(), 22);
    }

    #[test]
    fn add_renderable_to_existing_node_works() {
        let mut s = api().empty_scene();
        let n = api().create_node(&mut s);
        let id = api()
            .add_renderable(&mut s, n, api().mesh_ref(1), api().material_ref(2))
            .unwrap();
        assert!(id.is_valid());
    }

    #[test]
    fn add_renderable_to_missing_node_fails() {
        let mut s = api().empty_scene();
        let err = api()
            .add_renderable(
                &mut s,
                SceneNodeId::from_raw(42),
                api().mesh_ref(1),
                api().material_ref(2),
            )
            .unwrap_err();
        assert_eq!(err.code(), SceneErrorCode::MissingNode);
    }

    #[test]
    fn add_renderable_rejects_invalid_refs() {
        let mut s = api().empty_scene();
        let n = api().create_node(&mut s);
        let err = api()
            .add_renderable(&mut s, n, MeshRef::INVALID, api().material_ref(1))
            .unwrap_err();
        assert_eq!(err.code(), SceneErrorCode::InvalidRenderableReference);
    }

    #[test]
    fn set_visibility_works() {
        let mut s = api().empty_scene();
        let n = api().create_node(&mut s);
        let id = api()
            .add_renderable(&mut s, n, api().mesh_ref(1), api().material_ref(2))
            .unwrap();
        api().set_renderable_visibility(&mut s, id, false).unwrap();
        assert!(!s.renderable(id).unwrap().visible());
    }

    #[test]
    fn remove_renderable_works() {
        let mut s = api().empty_scene();
        let n = api().create_node(&mut s);
        let id = api()
            .add_renderable(&mut s, n, api().mesh_ref(1), api().material_ref(2))
            .unwrap();
        api().remove_renderable(&mut s, id).unwrap();
        assert_eq!(s.renderable_count(), 0);
    }

    // ---------- snapshots ----------

    #[test]
    fn snapshot_of_empty_scene_is_empty() {
        let s = api().empty_scene();
        let snap = api().snapshot(&s);
        assert!(snap.is_empty());
    }

    #[test]
    fn snapshot_records_lights_in_deterministic_order() {
        let mut s = api().empty_scene();
        let n = api().create_node(&mut s);
        let _ = api()
            .add_directional_light(&math(), &mut s, n, Vec3::ONE, 1.0)
            .unwrap();
        let _ = api()
            .add_point_light(&math(), &mut s, n, Vec3::ONE, 1.0)
            .unwrap();
        let snap = api().snapshot(&s);
        assert_eq!(snap.lights().len(), 2);
        // Directional was added first (lower id).
        assert_eq!(snap.lights()[0].kind(), LightKind::Directional);
        assert_eq!(snap.lights()[1].kind(), LightKind::Point);
    }

    #[test]
    fn identical_construction_produces_identical_snapshots() {
        let make = || {
            let mut s = api().empty_scene();
            let a = api().create_node_with_transform(
                &mut s,
                Transform::from_translation(Vec3::new(1.0, 0.0, 0.0)),
            );
            let b = api().create_node_with_transform(
                &mut s,
                Transform::from_translation(Vec3::new(0.0, 1.0, 0.0)),
            );
            api().set_parent(&mut s, b, a).unwrap();
            api().update_world_transforms(&mut s).unwrap();
            api().snapshot(&s)
        };
        assert_eq!(make(), make());
    }

    // ---------- frame integration ----------

    #[test]
    fn advance_runs_propagation_when_frame_is_active_with_steps() {
        use axiom_frame::FrameApi;
        use axiom_host::{
            HostBoundaryConfig, HostFrameInput, HostFrameReport, HostLifecycleSignal,
            HostLifecycleState, HostStepPlan, HostViewport,
        };

        // Build a real host frame report with a visible host and one runtime step.
        let m = math();
        let vp = HostViewport::new(&m, 100, 100, 1.0).unwrap();
        let cfg = HostBoundaryConfig::new(1_000, 5).unwrap();
        let visible =
            HostLifecycleState::initial().apply(HostLifecycleSignal::Started);
        let input = HostFrameInput::new(1, 1_000, vp);
        let plan = HostStepPlan::build(&input, &cfg, &visible, 0);
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

        let mut s = api().empty_scene();
        let p = api().create_node_with_transform(
            &mut s,
            Transform::from_translation(Vec3::new(3.0, 0.0, 0.0)),
        );
        let c = api().create_node_with_transform(
            &mut s,
            Transform::from_translation(Vec3::new(0.0, 4.0, 0.0)),
        );
        api().set_parent(&mut s, c, p).unwrap();

        let snap = api().advance(&mut s, &ctx).unwrap();
        // The child node's world translation should reflect parent + child.
        let child_snap = snap
            .nodes()
            .iter()
            .find(|n| n.parent().is_some())
            .expect("expected one child");
        assert_eq!(child_snap.world().translation.x, 3.0);
        assert_eq!(child_snap.world().translation.y, 4.0);
    }

    #[test]
    fn advance_skips_propagation_on_a_skipped_host_frame() {
        use axiom_frame::FrameApi;
        use axiom_host::{
            HostBoundaryConfig, HostFrameInput, HostFrameReport, HostLifecycleState,
            HostStepPlan, HostViewport,
        };

        let m = math();
        let vp = HostViewport::new(&m, 100, 100, 1.0).unwrap();
        let cfg = HostBoundaryConfig::new(1_000, 5).unwrap();
        // Lifecycle: never started → host is "hidden" by default → frame is skipped.
        let hidden = HostLifecycleState::initial();
        let input = HostFrameInput::new(1, 1_000, vp);
        let plan = HostStepPlan::build(&input, &cfg, &hidden, 0);
        let report = HostFrameReport::new(
            input.sequence(),
            plan,
            plan.steps(),
            Vec::new(),
            vp,
            hidden,
        );
        let frame_api = FrameApi::new();
        let engine_frame = frame_api
            .engine_frame_from_host_report(&report, 1_000, Vec::new())
            .unwrap();
        let ctx = frame_api.frame_context(&engine_frame);

        let mut s = api().empty_scene();
        let p = api().create_node_with_transform(
            &mut s,
            Transform::from_translation(Vec3::new(3.0, 0.0, 0.0)),
        );
        let c = api().create_node_with_transform(
            &mut s,
            Transform::from_translation(Vec3::new(0.0, 4.0, 0.0)),
        );
        api().set_parent(&mut s, c, p).unwrap();

        // Skipped frame must NOT update world transforms.
        let snap = api().advance(&mut s, &ctx).unwrap();
        let child_snap = snap
            .nodes()
            .iter()
            .find(|n| n.parent().is_some())
            .expect("expected one child");
        // World stayed at the default (identity-copy of local) because
        // propagation didn't run.
        assert_eq!(child_snap.world().translation.x, 0.0);
        assert_eq!(child_snap.world().translation.y, 4.0);
    }
}
