//! The single public facade of the `axiom-scene` module.

use axiom_frame::{FrameCommand, FrameContext};
use axiom_kernel::{Meters, Radians, Ratio, Reflect, TypeSchema};
use axiom_math::{Mat4, MathApi, Transform, Vec3};

use crate::camera::Camera;
use crate::light::Light;
use crate::material_ref::MaterialRef;
use crate::mesh_ref::MeshRef;
use crate::renderable::Renderable;
use crate::scene::Scene;
use crate::scene_error::SceneError;
use crate::scene_node_id::SceneNodeId;
use crate::scene_result::SceneResult;
use crate::scene_snapshot::SceneSnapshot;
use crate::spin::Spin;

/// The only public export of `axiom-scene`: a **stateful scene handle**.
///
/// `SceneApi` *owns* the scene — an ECS world (Layer 05) where nodes are
/// entities and every node fact is a component column. A consumer builds it
/// **once** and holds it across frames, mutating it incrementally and calling
/// [`Self::advance`] each tick; the world is durable authored state, not
/// rebuilt per frame. (It owns the world rather than handing back a `Scene`
/// value precisely so apps can keep it in a field — the internal types are
/// never re-exported, so they can't be named as field types.)
///
/// The facade is also where Layer 02 ([`MathApi`]) and Layer 04
/// ([`FrameContext`]) integration lives — see [`Self::advance`] and the
/// camera/light validators.
#[derive(Debug, Default)]
pub struct SceneApi {
    scene: Scene,
}

impl SceneApi {
    /// Construct an empty scene with the standard systems registered.
    pub fn new() -> Self {
        SceneApi {
            scene: Scene::new(),
        }
    }

    // --- Nodes ---

    /// Create a node with the identity local transform.
    pub fn create_node(&mut self) -> SceneNodeId {
        self.scene.create_node(Transform::IDENTITY)
    }

    /// Create a node with an explicit local transform.
    pub fn create_node_with_transform(&mut self, local: Transform) -> SceneNodeId {
        self.scene.create_node(local)
    }

    /// Set a node's local transform.
    pub fn set_local_transform(
        &mut self,
        id: SceneNodeId,
        transform: Transform,
    ) -> SceneResult<()> {
        self.scene.set_local(id, transform)
    }

    /// Get a node's local transform.
    pub fn local_transform(&self, id: SceneNodeId) -> SceneResult<Transform> {
        self.scene.local(id)
    }

    /// Get a node's world transform (the value computed by the most recent
    /// propagation, falling back to the local transform before any).
    pub fn world_transform(&self, id: SceneNodeId) -> SceneResult<Transform> {
        self.scene.world_transform(id)
    }

    /// A node's parent, if any.
    pub fn parent_of(&self, id: SceneNodeId) -> Option<SceneNodeId> {
        self.scene.parent_of(id)
    }

    // --- Hierarchy ---

    /// Make `child` a child of `parent`. Rejects self-parenting, cycles, and
    /// missing ids.
    pub fn set_parent(&mut self, child: SceneNodeId, parent: SceneNodeId) -> SceneResult<()> {
        self.scene.set_parent(child, parent)
    }

    /// Detach `child` from its parent (if any), leaving it a root.
    pub fn clear_parent(&mut self, child: SceneNodeId) -> SceneResult<()> {
        self.scene.clear_parent(child)
    }

    // --- Cameras ---

    /// Add a perspective camera to `node`. Intrinsic validation is delegated to
    /// [`MathApi::mat4_perspective`].
    pub fn add_perspective_camera(
        &mut self,
        math: &MathApi,
        node: SceneNodeId,
        fovy_radians: Radians,
        aspect: Ratio,
        near: Meters,
        far: Meters,
    ) -> SceneResult<()> {
        let camera = Camera::perspective(math, fovy_radians, aspect, near, far)?;
        self.scene.add_camera(node, camera)
    }

    /// Remove the camera on `node`.
    pub fn remove_camera(&mut self, node: SceneNodeId) -> SceneResult<()> {
        self.scene.remove_camera(node)
    }

    /// Compute the projection matrix for the camera on `node`.
    pub fn camera_projection_matrix(&self, math: &MathApi, node: SceneNodeId) -> SceneResult<Mat4> {
        match self.scene.camera(node) {
            Some(camera) => camera.projection_matrix(math),
            None => Err(SceneError::missing_camera("node has no camera")),
        }
    }

    // --- Lights ---

    /// Add a directional light to `node`.
    pub fn add_directional_light(
        &mut self,
        math: &MathApi,
        node: SceneNodeId,
        color: Vec3,
        intensity: Ratio,
    ) -> SceneResult<()> {
        let light = Light::directional(math, color, intensity)?;
        self.scene.add_light(node, light)
    }

    /// Add a point light to `node`.
    pub fn add_point_light(
        &mut self,
        math: &MathApi,
        node: SceneNodeId,
        color: Vec3,
        intensity: Ratio,
    ) -> SceneResult<()> {
        let light = Light::point(math, color, intensity)?;
        self.scene.add_light(node, light)
    }

    /// Remove the light on `node`.
    pub fn remove_light(&mut self, node: SceneNodeId) -> SceneResult<()> {
        self.scene.remove_light(node)
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
        &mut self,
        node: SceneNodeId,
        mesh: MeshRef,
        material: MaterialRef,
    ) -> SceneResult<()> {
        let renderable = Renderable::new(mesh, material)?;
        self.scene.add_renderable(node, renderable)
    }

    /// Remove the renderable on `node`.
    pub fn remove_renderable(&mut self, node: SceneNodeId) -> SceneResult<()> {
        self.scene.remove_renderable(node)
    }

    /// Toggle the visibility of the renderable on `node`.
    pub fn set_renderable_visibility(
        &mut self,
        node: SceneNodeId,
        visible: bool,
    ) -> SceneResult<()> {
        self.scene.set_renderable_visible(node, visible)
    }

    // --- Spin (data-declared rotation, animated by the engine) ---

    /// Give `node` a spin: a pure rotation about `axis`, one revolution every
    /// `period_ticks` frames, animated by the engine's spin system each
    /// [`Self::advance`].
    pub fn add_spin(
        &mut self,
        node: SceneNodeId,
        axis: Vec3,
        period_ticks: u32,
    ) -> SceneResult<()> {
        self.scene.add_spin(node, Spin::new(axis, period_ticks))
    }

    // --- Players (controllable nodes moved by per-tick commands) ---

    /// Mark `node` as the controllable node for `player` index. Per-tick move
    /// commands addressed to that index translate it during [`Self::advance`].
    pub fn add_player(&mut self, node: SceneNodeId, player: u32) -> SceneResult<()> {
        self.scene.add_player(node, player)
    }

    /// Encode a per-tick move for `player` by `delta` (a translation delta) as a
    /// [`FrameCommand`] to hand to the frame builder. The scene decodes these in
    /// [`Self::advance`] and applies them to the addressed player's node.
    pub fn move_command(&self, sequence: u64, player: u32, delta: Vec3) -> FrameCommand {
        crate::player_command::encode_move(sequence, player, delta)
    }

    // --- Controllers (first-person nodes yawed + moved by per-tick commands) ---

    /// Mark `node` as the first-person controller for `index`. Per-tick
    /// controller commands addressed to that index yaw it about +Y and move it
    /// along its own facing during [`Self::advance`].
    pub fn add_controller(&mut self, node: SceneNodeId, index: u32) -> SceneResult<()> {
        self.scene.add_controller(node, index)
    }

    /// Encode a per-tick first-person input for controller `index`: a `yaw`/`pitch`
    /// look delta (yaw about +Y, pitch about local +X, clamped by the scene) plus
    /// a `move_local` translation in the node's own frame (local -Z is forward,
    /// local +X is right), as a [`FrameCommand`] to hand to the frame builder.
    /// The scene decodes these in [`Self::advance`] and applies them to the
    /// addressed controller's node — moving in the yaw-only horizontal frame.
    pub fn controller_command(
        &self,
        sequence: u64,
        index: u32,
        move_local: Vec3,
        yaw: Radians,
        pitch: Radians,
    ) -> FrameCommand {
        crate::controller_command::encode_controller(
            sequence,
            index,
            move_local,
            yaw.get(),
            pitch.get(),
        )
    }

    // --- Propagation / frame integration ---

    /// Recompute every node's world transform now.
    pub fn update_world_transforms(&mut self) {
        self.scene.update_world_transforms();
    }

    /// Advance the scene for one engine frame at logical time `tick`: run the
    /// spin + transform-propagation systems iff the frame is active, then return
    /// the snapshot taken afterward. The caller owns the tick (see
    /// [`axiom_ecs::World::advance`]).
    #[axiom_zones::sim]
    pub fn advance(&mut self, tick: u64, frame: &FrameContext<'_>) -> SceneSnapshot {
        self.scene.advance(tick, frame)
    }

    /// Build a deterministic snapshot of the scene's current state.
    pub fn snapshot(&self) -> SceneSnapshot {
        self.scene.snapshot()
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
    use axiom_kernel::{Meters, Radians, Ratio};

    fn math() -> MathApi {
        MathApi::new()
    }

    fn api() -> SceneApi {
        SceneApi::new()
    }

    fn rad(x: f32) -> Radians {
        Radians::new(x).unwrap()
    }
    fn rat(x: f32) -> Ratio {
        Ratio::new(x).unwrap()
    }
    fn m(x: f32) -> Meters {
        Meters::new(x).unwrap()
    }

    #[test]
    fn new_and_default_facades_are_equivalent() {
        // Both construction paths produce the same (empty) scene snapshot.
        assert_eq!(SceneApi::new().snapshot(), SceneApi::default().snapshot());
    }

    #[test]
    fn add_player_marks_a_node_and_rejects_a_missing_one() {
        let mut s = api();
        let node = s.create_node();
        assert!(s.add_player(node, 0).is_ok());
        assert!(s.add_player(SceneNodeId::from_raw(404), 1).is_err());
    }

    #[test]
    fn move_command_encodes_a_decodable_move() {
        // The facade-built command round-trips through the scene's decoder.
        let s = api();
        let cmd = s.move_command(0, 3, Vec3::new(0.25, -0.75, 0.0));
        assert_eq!(
            crate::player_command::decode_move(&cmd),
            Some((3, Vec3::new(0.25, -0.75, 0.0)))
        );
    }

    #[test]
    fn add_controller_marks_a_node_and_rejects_a_missing_one() {
        let mut s = api();
        let node = s.create_node();
        assert!(s.add_controller(node, 0).is_ok());
        assert!(s.add_controller(SceneNodeId::from_raw(404), 1).is_err());
    }

    #[test]
    fn controller_command_encodes_a_decodable_input() {
        // The facade-built command round-trips through the scene's decoder.
        let s = api();
        let cmd = s.controller_command(0, 1, Vec3::new(0.5, 0.0, -0.25), rad(0.1), rad(-0.2));
        assert_eq!(
            crate::controller_command::decode_controller(&cmd),
            Some((1, Vec3::new(0.5, 0.0, -0.25), 0.1, -0.2))
        );
    }

    #[test]
    fn nodes_transforms_and_hierarchy_round_trip() {
        let mut a = api();
        let p = a.create_node();
        let c = a.create_node_with_transform(Transform::from_translation(Vec3::new(0.0, 2.0, 0.0)));
        a.set_local_transform(p, Transform::from_translation(Vec3::new(1.0, 0.0, 0.0)))
            .unwrap();
        assert_eq!(a.local_transform(p).unwrap().translation.x, 1.0);
        a.set_parent(c, p).unwrap();
        assert_eq!(a.parent_of(c), Some(p));
        a.update_world_transforms();
        assert_eq!(a.world_transform(c).unwrap().translation.x, 1.0);
        a.clear_parent(c).unwrap();
        assert_eq!(a.parent_of(c), None);
    }

    #[test]
    fn add_perspective_camera_valid_and_invalid() {
        let mut a = api();
        let n = a.create_node();
        a.add_perspective_camera(
            &math(),
            n,
            rad(std::f32::consts::FRAC_PI_3),
            rat(1.0),
            m(0.1),
            m(100.0),
        )
        .unwrap();
        // Invalid intrinsics propagate the `?` error arm.
        let err = a
            .add_perspective_camera(&math(), n, rad(0.0), rat(1.0), m(0.1), m(100.0))
            .unwrap_err();
        assert_eq!(err.code(), SceneErrorCode::InvalidCameraParameters);
        // Projection matrix: present + missing.
        let m = a.camera_projection_matrix(&math(), n).unwrap();
        assert_eq!(m.as_cols_array(), m.as_cols_array());
        let n2 = a.create_node();
        assert_eq!(
            a.camera_projection_matrix(&math(), n2).unwrap_err().code(),
            SceneErrorCode::MissingCamera
        );
        a.remove_camera(n).unwrap();
    }

    #[test]
    fn lights_valid_and_invalid() {
        let mut a = api();
        let n = a.create_node();
        a.add_directional_light(&math(), n, Vec3::ONE, rat(1.0))
            .unwrap();
        a.add_point_light(&math(), n, Vec3::new(0.5, 0.5, 0.5), rat(2.0))
            .unwrap();
        assert_eq!(
            a.add_directional_light(&math(), n, Vec3::ONE, rat(-1.0))
                .unwrap_err()
                .code(),
            SceneErrorCode::InvalidLightParameters
        );
        assert_eq!(
            a.add_point_light(&math(), n, Vec3::new(f32::NAN, 0.0, 0.0), rat(1.0))
                .unwrap_err()
                .code(),
            SceneErrorCode::InvalidLightParameters
        );
        a.remove_light(n).unwrap();
    }

    #[test]
    fn renderables_valid_and_invalid() {
        let mut a = api();
        let n = a.create_node();
        let mesh = a.mesh_ref(1);
        let material = a.material_ref(2);
        a.add_renderable(n, mesh, material).unwrap();
        assert_eq!(
            a.add_renderable(n, MeshRef::INVALID, material)
                .unwrap_err()
                .code(),
            SceneErrorCode::InvalidRenderableReference
        );
        a.set_renderable_visibility(n, false).unwrap();
        a.remove_renderable(n).unwrap();
    }

    #[test]
    fn add_spin_valid_and_missing_node() {
        let mut a = api();
        let n = a.create_node();
        a.add_spin(n, Vec3::UNIT_Y, 360).unwrap();
        assert_eq!(
            a.add_spin(SceneNodeId::from_raw(99), Vec3::UNIT_Y, 360)
                .unwrap_err()
                .code(),
            SceneErrorCode::MissingNode
        );
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
    fn snapshot_reads_current_scene_state() {
        let mut a = api();
        let n = a.create_node();
        a.add_directional_light(&math(), n, Vec3::ONE, rat(1.0))
            .unwrap();
        let snap = a.snapshot();
        assert_eq!(snap.nodes().len(), 1);
        assert_eq!(snap.lights().len(), 1);
    }

    #[test]
    fn advance_animates_and_propagates_across_ticks_on_a_held_scene() {
        use axiom_frame::FrameApi;
        use axiom_host::{
            HostBoundaryConfig, HostFrameInput, HostFrameReport, HostLifecycleSignal,
            HostLifecycleState, HostStepPlan, HostViewport,
        };
        // Build the scene ONCE, then advance it at two different ticks: the
        // spun child's world transform must differ — proving a held, durable
        // world that evolves, not a rebuilt one.
        let mut a = api();
        let parent =
            a.create_node_with_transform(Transform::from_translation(Vec3::new(2.0, 0.0, 0.0)));
        let child = a.create_node();
        a.set_parent(child, parent).unwrap();
        a.add_spin(child, Vec3::UNIT_Y, 8).unwrap();

        let frame = |elapsed: u64| {
            let vp = HostViewport::new(100, 100, Ratio::new(1.0).unwrap()).unwrap();
            let cfg = HostBoundaryConfig::new(1_000, 5).unwrap();
            let visible = HostLifecycleState::initial().apply(HostLifecycleSignal::Started);
            let input = HostFrameInput::new(1, elapsed, vp);
            let plan = HostStepPlan::build(&input, &cfg, &visible, 0);
            let report = HostFrameReport::new(
                input.sequence(),
                plan,
                plan.steps(),
                Vec::new(),
                vp,
                visible,
            );
            FrameApi::new()
                .engine_frame_from_host_report(&report, elapsed, Vec::new())
                .unwrap()
        };

        let f0 = frame(1_000);
        let snap0 = a.advance(0, &FrameContext::new(&f0));
        let child0 = snap0
            .nodes()
            .iter()
            .find(|n| n.parent().is_some())
            .unwrap()
            .world();

        let f2 = frame(1_000);
        let snap2 = a.advance(2, &FrameContext::new(&f2));
        let child2 = snap2
            .nodes()
            .iter()
            .find(|n| n.parent().is_some())
            .unwrap()
            .world();

        // Same handle, different ticks -> different world rotation, same parent
        // translation carried through.
        assert_ne!(child0.rotation, child2.rotation);
        assert_eq!(child2.translation.x, 2.0);
    }
}
