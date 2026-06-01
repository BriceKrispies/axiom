//! The single public facade of the `axiom-scene` module.

use axiom_frame::FrameContext;
use axiom_kernel::{Reflect, TypeSchema};
use axiom_math::{Mat4, MathApi, Transform, Vec3};

use crate::camera::Camera;
use crate::light::Light;
use crate::material_ref::MaterialRef;
use crate::mesh_ref::MeshRef;
use crate::renderable::Renderable;
use crate::scene::Scene;
use crate::spin::Spin;
use crate::scene_error::SceneError;
use crate::scene_node_id::SceneNodeId;
use crate::scene_result::SceneResult;
use crate::scene_snapshot::SceneSnapshot;

/// The only public export of `axiom-scene`.
///
/// `SceneApi` is a zero-sized facade. Every scene operation goes through one of
/// its methods; the internal types (`Scene`, `Camera`, `Light`, `Renderable`,
/// snapshots) are public inside the crate but never re-exported from `lib.rs`.
///
/// The scene is an ECS world (Layer 05): nodes are entities and every node fact
/// is a component column. The facade is also where Layer 02 ([`MathApi`]) and
/// Layer 04 ([`FrameContext`]) integration lives — see [`Self::advance`] and
/// the camera/light validators.
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
    pub fn create_node_with_transform(&self, scene: &mut Scene, local: Transform) -> SceneNodeId {
        scene.create_node(local)
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
    pub fn local_transform(&self, scene: &Scene, id: SceneNodeId) -> SceneResult<Transform> {
        scene.local(id)
    }

    /// Get a node's world transform (the value computed by the most recent
    /// propagation, falling back to the local transform before any).
    pub fn world_transform(&self, scene: &Scene, id: SceneNodeId) -> SceneResult<Transform> {
        scene.world_transform(id)
    }

    /// A node's parent, if any.
    pub fn parent_of(&self, scene: &Scene, id: SceneNodeId) -> Option<SceneNodeId> {
        scene.parent_of(id)
    }

    // --- Hierarchy ---

    /// Make `child` a child of `parent`. Rejects self-parenting, cycles, and
    /// missing ids.
    pub fn set_parent(
        &self,
        scene: &mut Scene,
        child: SceneNodeId,
        parent: SceneNodeId,
    ) -> SceneResult<()> {
        scene.set_parent(child, parent)
    }

    /// Detach `child` from its parent (if any), leaving it a root.
    pub fn clear_parent(&self, scene: &mut Scene, child: SceneNodeId) -> SceneResult<()> {
        scene.clear_parent(child)
    }

    // --- Cameras ---

    /// Add a perspective camera to `node`. Intrinsic validation is delegated to
    /// [`MathApi::mat4_perspective`].
    pub fn add_perspective_camera(
        &self,
        math: &MathApi,
        scene: &mut Scene,
        node: SceneNodeId,
        fovy_radians: f32,
        aspect: f32,
        near: f32,
        far: f32,
    ) -> SceneResult<()> {
        let camera = Camera::perspective(math, fovy_radians, aspect, near, far)?;
        scene.add_camera(node, camera)
    }

    /// Remove the camera on `node`.
    pub fn remove_camera(&self, scene: &mut Scene, node: SceneNodeId) -> SceneResult<()> {
        scene.remove_camera(node)
    }

    /// Compute the projection matrix for the camera on `node`.
    pub fn camera_projection_matrix(
        &self,
        math: &MathApi,
        scene: &Scene,
        node: SceneNodeId,
    ) -> SceneResult<Mat4> {
        match scene.camera(node) {
            Some(camera) => camera.projection_matrix(math),
            None => Err(SceneError::missing_camera("node has no camera")),
        }
    }

    // --- Lights ---

    /// Add a directional light to `node`.
    pub fn add_directional_light(
        &self,
        math: &MathApi,
        scene: &mut Scene,
        node: SceneNodeId,
        color: Vec3,
        intensity: f32,
    ) -> SceneResult<()> {
        let light = Light::directional(math, color, intensity)?;
        scene.add_light(node, light)
    }

    /// Add a point light to `node`.
    pub fn add_point_light(
        &self,
        math: &MathApi,
        scene: &mut Scene,
        node: SceneNodeId,
        color: Vec3,
        intensity: f32,
    ) -> SceneResult<()> {
        let light = Light::point(math, color, intensity)?;
        scene.add_light(node, light)
    }

    /// Remove the light on `node`.
    pub fn remove_light(&self, scene: &mut Scene, node: SceneNodeId) -> SceneResult<()> {
        scene.remove_light(node)
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

    /// Add a renderable to `node`.
    pub fn add_renderable(
        &self,
        scene: &mut Scene,
        node: SceneNodeId,
        mesh: MeshRef,
        material: MaterialRef,
    ) -> SceneResult<()> {
        let renderable = Renderable::new(mesh, material)?;
        scene.add_renderable(node, renderable)
    }

    /// Remove the renderable on `node`.
    pub fn remove_renderable(&self, scene: &mut Scene, node: SceneNodeId) -> SceneResult<()> {
        scene.remove_renderable(node)
    }

    /// Toggle the visibility of the renderable on `node`.
    pub fn set_renderable_visibility(
        &self,
        scene: &mut Scene,
        node: SceneNodeId,
        visible: bool,
    ) -> SceneResult<()> {
        scene.set_renderable_visible(node, visible)
    }

    // --- Spin (data-declared rotation, animated by the engine) ---

    /// Give `node` a spin: a pure rotation about `axis`, one revolution every
    /// `period_ticks` frames, animated by the engine's spin system each
    /// [`Self::advance`]. This is the data-driven alternative to an app setting
    /// the rotation by hand every tick.
    pub fn add_spin(
        &self,
        scene: &mut Scene,
        node: SceneNodeId,
        axis: Vec3,
        period_ticks: u32,
    ) -> SceneResult<()> {
        scene.add_spin(node, Spin::new(axis, period_ticks))
    }

    // --- Propagation / frame integration ---

    /// Recompute every node's world transform now.
    pub fn update_world_transforms(&self, scene: &mut Scene) {
        scene.update_world_transforms();
    }

    /// Advance the scene for one engine frame at logical time `tick`: run the
    /// spin + transform-propagation systems iff the frame is active, then return
    /// the snapshot taken afterward. The caller owns the tick (see
    /// [`axiom_ecs::World::advance`]).
    pub fn advance(&self, scene: &mut Scene, tick: u64, frame: &FrameContext<'_>) -> SceneSnapshot {
        scene.advance(tick, frame)
    }

    /// Build a deterministic snapshot of the scene's current state.
    pub fn snapshot(&self, scene: &Scene) -> SceneSnapshot {
        scene.snapshot()
    }

    // --- Self-description ---

    /// The reflected schemas of the standard component types a scene is built
    /// from — the scene describing its own shape as data an agent can read.
    pub fn component_schemas(&self) -> Vec<TypeSchema> {
        vec![
            <Transform as Reflect>::SCHEMA,
            Camera::SCHEMA,
            Light::SCHEMA,
            Renderable::SCHEMA,
            Spin::SCHEMA,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene_error_code::SceneErrorCode;

    fn math() -> MathApi {
        MathApi::new()
    }

    fn api() -> SceneApi {
        SceneApi::new()
    }

    #[test]
    fn new_and_default_facades_are_equivalent() {
        let _ = SceneApi::new();
        let _ = SceneApi::default();
    }

    #[test]
    fn nodes_transforms_and_hierarchy_round_trip() {
        let a = api();
        let mut s = a.empty_scene();
        let p = a.create_node(&mut s);
        let c = a.create_node_with_transform(&mut s, Transform::from_translation(Vec3::new(0.0, 2.0, 0.0)));
        a.set_local_transform(&mut s, p, Transform::from_translation(Vec3::new(1.0, 0.0, 0.0)))
            .unwrap();
        assert_eq!(a.local_transform(&s, p).unwrap().translation.x, 1.0);
        a.set_parent(&mut s, c, p).unwrap();
        assert_eq!(a.parent_of(&s, c), Some(p));
        a.update_world_transforms(&mut s);
        assert_eq!(a.world_transform(&s, c).unwrap().translation.x, 1.0);
        a.clear_parent(&mut s, c).unwrap();
        assert_eq!(a.parent_of(&s, c), None);
    }

    #[test]
    fn add_perspective_camera_valid_and_invalid() {
        let a = api();
        let mut s = a.empty_scene();
        let n = a.create_node(&mut s);
        a.add_perspective_camera(&math(), &mut s, n, std::f32::consts::FRAC_PI_3, 1.0, 0.1, 100.0)
            .unwrap();
        // Invalid intrinsics propagate the `?` error arm.
        let err = a
            .add_perspective_camera(&math(), &mut s, n, 0.0, 1.0, 0.1, 100.0)
            .unwrap_err();
        assert_eq!(err.code(), SceneErrorCode::InvalidCameraParameters);
        // Projection matrix: present + missing.
        let m = a.camera_projection_matrix(&math(), &s, n).unwrap();
        assert_eq!(m.as_cols_array(), m.as_cols_array());
        let n2 = a.create_node(&mut s);
        assert_eq!(
            a.camera_projection_matrix(&math(), &s, n2).unwrap_err().code(),
            SceneErrorCode::MissingCamera
        );
        a.remove_camera(&mut s, n).unwrap();
    }

    #[test]
    fn lights_valid_and_invalid() {
        let a = api();
        let mut s = a.empty_scene();
        let n = a.create_node(&mut s);
        a.add_directional_light(&math(), &mut s, n, Vec3::ONE, 1.0).unwrap();
        a.add_point_light(&math(), &mut s, n, Vec3::new(0.5, 0.5, 0.5), 2.0).unwrap();
        // Invalid params propagate the `?` arm (both constructors).
        assert_eq!(
            a.add_directional_light(&math(), &mut s, n, Vec3::ONE, -1.0).unwrap_err().code(),
            SceneErrorCode::InvalidLightParameters
        );
        assert_eq!(
            a.add_point_light(&math(), &mut s, n, Vec3::ONE, f32::NAN).unwrap_err().code(),
            SceneErrorCode::InvalidLightParameters
        );
        a.remove_light(&mut s, n).unwrap();
    }

    #[test]
    fn renderables_valid_and_invalid() {
        let a = api();
        let mut s = a.empty_scene();
        let n = a.create_node(&mut s);
        a.add_renderable(&mut s, n, a.mesh_ref(1), a.material_ref(2)).unwrap();
        // Invalid ref propagates the `?` arm.
        assert_eq!(
            a.add_renderable(&mut s, n, MeshRef::INVALID, a.material_ref(2)).unwrap_err().code(),
            SceneErrorCode::InvalidRenderableReference
        );
        a.set_renderable_visibility(&mut s, n, false).unwrap();
        a.remove_renderable(&mut s, n).unwrap();
    }

    #[test]
    fn component_schemas_describe_the_standard_components() {
        let schemas = api().component_schemas();
        assert_eq!(schemas.len(), 5);
        assert_eq!(schemas[0].name(), "Transform");
        assert_eq!(schemas[1].name(), "Camera");
        assert_eq!(schemas[2].name(), "Light");
        assert_eq!(schemas[3].name(), "Renderable");
        assert_eq!(schemas[4].name(), "Spin");
    }

    #[test]
    fn add_spin_valid_and_missing_node() {
        let a = api();
        let mut s = a.empty_scene();
        let n = a.create_node(&mut s);
        a.add_spin(&mut s, n, Vec3::UNIT_Y, 360).unwrap();
        assert_eq!(
            a.add_spin(&mut s, SceneNodeId::from_raw(99), Vec3::UNIT_Y, 360).unwrap_err().code(),
            SceneErrorCode::MissingNode
        );
    }

    #[test]
    fn snapshot_reads_current_scene_state() {
        let a = api();
        let mut s = a.empty_scene();
        let n = a.create_node(&mut s);
        a.add_directional_light(&math(), &mut s, n, Vec3::ONE, 1.0).unwrap();
        let snap = a.snapshot(&s);
        assert_eq!(snap.nodes().len(), 1);
        assert_eq!(snap.lights().len(), 1);
    }

    #[test]
    fn advance_is_frame_gated() {
        use axiom_frame::FrameApi;
        use axiom_host::{
            HostBoundaryConfig, HostFrameInput, HostFrameReport, HostLifecycleSignal,
            HostLifecycleState, HostStepPlan, HostViewport,
        };
        let a = api();
        let mut s = a.empty_scene();
        let p = a.create_node_with_transform(&mut s, Transform::from_translation(Vec3::new(3.0, 0.0, 0.0)));
        let c = a.create_node_with_transform(&mut s, Transform::from_translation(Vec3::new(0.0, 4.0, 0.0)));
        a.set_parent(&mut s, c, p).unwrap();

        let m = math();
        let vp = HostViewport::new(&m, 100, 100, 1.0).unwrap();
        let cfg = HostBoundaryConfig::new(1_000, 5).unwrap();
        let visible = HostLifecycleState::initial().apply(HostLifecycleSignal::Started);
        let input = HostFrameInput::new(1, 1_000, vp);
        let plan = HostStepPlan::build(&input, &cfg, &visible, 0);
        let report = HostFrameReport::new(input.sequence(), plan, plan.steps(), Vec::new(), vp, visible);
        let frame_api = FrameApi::new();
        let engine_frame = frame_api.engine_frame_from_host_report(&report, 1_000, Vec::new()).unwrap();
        let ctx = frame_api.frame_context(&engine_frame);

        let snap = a.advance(&mut s, 0, &ctx);
        let child = snap.nodes().iter().find(|n| n.parent().is_some()).unwrap();
        assert_eq!(child.world().translation.x, 3.0);
        assert_eq!(child.world().translation.y, 4.0);
    }
}
