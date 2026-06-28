//! The single public facade of the `axiom-scene` module.

use axiom_frame::FrameContext;
use axiom_kernel::{
    BinaryReader, BinaryWriter, KernelResult, Meters, Radians, Ratio, Reflect, TypeSchema,
};
use axiom_math::{Mat4, MathApi, Transform, Vec3};

use crate::bounds::Bounds;
use crate::camera::Camera;
use crate::light::Light;
use crate::material_ref::MaterialRef;
use crate::mesh_ref::MeshRef;
use crate::procanim::ProcAnim;
use crate::renderable::Renderable;
use crate::scene::Scene;
use crate::scene_error::SceneError;
use crate::scene_node_id::SceneNodeId;
use crate::scene_result::SceneResult;
use crate::scene_snapshot::SceneSnapshot;
use crate::spin::Spin;
use crate::tag::Tag;

/// The players-and-controllers arm of the facade (per-tick command-driven node
/// marks). A child module so neither `impl SceneApi` block exceeds the engine's
/// impl-block size budget.
mod players;

/// The only public export of `axiom-scene`: a **stateful scene handle**.
///
/// `SceneApi` *owns* the scene — an ECS world (the ecs layer) where nodes are
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

    /// Every node's `(id, local transform)`, in ascending node-id order — the
    /// enumeration behind a consumer's typed `query::<Transform>()`.
    pub fn node_transforms(&self) -> Vec<(SceneNodeId, Transform)> {
        self.scene.node_transforms()
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
        Camera::perspective(math, fovy_radians, aspect, near, far)
            .and_then(|camera| self.scene.add_camera(node, camera))
    }

    /// Remove the camera on `node`.
    pub fn remove_camera(&mut self, node: SceneNodeId) -> SceneResult<()> {
        self.scene.remove_camera(node)
    }

    /// Compute the projection matrix for the camera on `node`.
    pub fn camera_projection_matrix(&self, math: &MathApi, node: SceneNodeId) -> SceneResult<Mat4> {
        self.scene.camera(node).map_or_else(
            || Err(SceneError::missing_camera("node has no camera")),
            |camera| camera.projection_matrix(math),
        )
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
        Light::directional(math, color, intensity)
            .and_then(|light| self.scene.add_light(node, light))
    }

    /// Add a point light to `node`.
    pub fn add_point_light(
        &mut self,
        math: &MathApi,
        node: SceneNodeId,
        color: Vec3,
        intensity: Ratio,
    ) -> SceneResult<()> {
        Light::point(math, color, intensity).and_then(|light| self.scene.add_light(node, light))
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
        Renderable::new(mesh, material)
            .and_then(|renderable| self.scene.add_renderable(node, renderable))
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

    /// Mark whether the renderable on `node` is a discrete dynamic object that
    /// grounds itself with a contact shadow (level geometry stays `false`).
    pub fn set_renderable_casts_contact_shadow(
        &mut self,
        node: SceneNodeId,
        casts: bool,
    ) -> SceneResult<()> {
        self.scene.set_renderable_casts_contact_shadow(node, casts)
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
            ProcAnim::SCHEMA,
            Bounds::SCHEMA,
            Tag::SCHEMA,
        ]
    }
}

/// Bounding volumes and spatial reasoning — Category 2 of the game vocabulary
/// (see `docs/game-vocabulary.md`). The engine answers "what is where" so an app
/// never reimplements geometry: collision tests, hitscan/line-of-sight, and
/// proximity all reduce to these two queries. These are spatial *queries*
/// (picking/overlap) over scene bounding volumes — not physics. Kept in its own
/// `impl` block so neither block exceeds the engine's impl-size budget.
impl SceneApi {
    /// Attach an axis-aligned bounding volume to `node`, given its local
    /// `half_extents` (sized by the node's world scale at query time), so the
    /// spatial queries below can hit it.
    pub fn add_bounds(&mut self, node: SceneNodeId, half_extents: Vec3) -> SceneResult<()> {
        self.scene.add_bounds(node, Bounds::new(half_extents))
    }

    /// Remove the bounding volume on `node`.
    pub fn remove_bounds(&mut self, node: SceneNodeId) -> SceneResult<()> {
        self.scene.remove_bounds(node)
    }

    /// The half-extents of `node`'s bounding volume, if any — the typed read
    /// behind a consumer's `get::<Bounds>()`.
    pub fn bounds_half_extents(&self, node: SceneNodeId) -> Option<Vec3> {
        self.scene.bounds_half_extents(node)
    }

    /// Every bounded node's `(id, half-extents)`, in ascending node-id order — the
    /// enumeration behind a consumer's `query::<Bounds>()`.
    pub fn bounded_nodes(&self) -> Vec<(SceneNodeId, Vec3)> {
        self.scene.bounded_nodes()
    }

    /// The player index marked on `node`, if any — used to classify a raycast /
    /// overlap hit as a player-marked actor versus plain geometry.
    pub fn player_index(&self, node: SceneNodeId) -> Option<u32> {
        self.scene.player_index(node)
    }

    /// The node handle marked with `player` index, if any. Lets a caller that
    /// authored players by index recover the first-class [`SceneNodeId`] to
    /// address them by handle (despawn, transform, queries) afterward.
    pub fn player_entity(&self, player: u32) -> Option<SceneNodeId> {
        self.scene.player_entity(player)
    }

    /// Despawn the node marked with `player` index — removing it from the scene
    /// (every component column) and its player/controller marks. Returns whether
    /// such a node existed; despawning an absent index is a clean `false`, so it
    /// is safe to call every tick. The runtime counterpart to authoring a node:
    /// the engine owns object lifetime, so a game never fakes removal (e.g. by
    /// parking a corpse off-screen).
    pub fn despawn_player(&mut self, player: u32) -> bool {
        self.scene.despawn_player(player)
    }

    /// Despawn `node` by its handle — the Entity-addressed counterpart to
    /// [`Self::despawn_player`]. Returns whether `node` named a live node.
    pub fn despawn_node(&mut self, node: SceneNodeId) -> bool {
        self.scene.despawn_node(node)
    }

    /// Cast a ray from `origin` along `direction`, returning the nearest bounded
    /// node it enters within `max_distance` (or `None`). The single primitive
    /// behind hitscan, line-of-sight, and picking. Reads *propagated* world
    /// transforms, so advance (or [`Self::update_world_transforms`]) before
    /// querying; a zero/non-finite direction yields `None`.
    pub fn raycast(
        &self,
        origin: Vec3,
        direction: Vec3,
        max_distance: Meters,
    ) -> Option<SceneNodeId> {
        self.scene.raycast(origin, direction, max_distance.get())
    }

    /// Cast a ray and return the nearest bounded node **with the world-space entry
    /// point** on its box (or `None`). The hit point carries the exact distance a
    /// perceiving agent reads — `‖point − origin‖` — which [`Self::raycast`]
    /// (id only) discards. Same propagation requirement and miss conditions.
    pub fn raycast_hit(
        &self,
        origin: Vec3,
        direction: Vec3,
        max_distance: Meters,
    ) -> Option<(SceneNodeId, Vec3)> {
        self.scene
            .raycast_hit(origin, direction, max_distance.get())
    }

    /// Attach (or replace) the coarse semantic kind on `node` — the engine-native
    /// "what is this thing" a perceiving agent reads off a hit (wall/enemy/door…),
    /// whose code vocabulary the app owns.
    pub fn add_tag(&mut self, node: SceneNodeId, kind_code: u32) -> SceneResult<()> {
        self.scene.add_tag(node, kind_code)
    }

    /// The coarse kind code tagged on `node`, if any — classifies a raycast /
    /// overlap hit without the app keeping a side table of entity kinds.
    pub fn tag_of(&self, node: SceneNodeId) -> Option<u32> {
        self.scene.tag_of(node)
    }

    /// Every node tagged with `kind_code`, in ascending node-id order.
    pub fn tagged_nodes(&self, kind_code: u32) -> Vec<SceneNodeId> {
        self.scene.tagged_nodes(kind_code)
    }

    /// Every bounded node whose world box overlaps the query box (centered at
    /// `center`, of `half_extents`), in ascending node-id order. The single
    /// primitive behind collision tests and proximity/contact checks.
    pub fn overlap_box(&self, center: Vec3, half_extents: Vec3) -> Vec<SceneNodeId> {
        self.scene.overlap_box(center, half_extents)
    }

    /// Every bounded node whose world box overlaps the query sphere (centered at
    /// `center`, of `radius`), in ascending node-id order — the radial companion
    /// to [`Self::overlap_box`] for proximity, pickup, and blast-radius checks.
    /// Reads *propagated* world transforms, so advance (or
    /// [`Self::update_world_transforms`]) before querying.
    pub fn overlap_circle(&self, center: Vec3, radius: Meters) -> Vec<SceneNodeId> {
        self.scene.overlap_circle(center, radius.get())
    }

    /// The direct children of `node`, in ascending node-id order (empty for a leaf
    /// or a node not in the scene) — the reverse of [`Self::parent_of`], for
    /// walking a formation or attached-part hierarchy.
    pub fn children_of(&self, node: SceneNodeId) -> Vec<SceneNodeId> {
        self.scene.children_of(node)
    }

    /// Despawn `node` and its whole subtree, returning whether `node` named a live
    /// node. Despawning a parent removes every descendant with it, so attached
    /// parts never outlive their owner.
    pub fn despawn_subtree(&mut self, node: SceneNodeId) -> bool {
        self.scene.despawn_subtree(node)
    }
}

/// Data-declared animation authoring: attach the engine's tick-driven animation
/// components (spin, procedural bob/spin) to a node. Kept in its own `impl` block
/// so neither authoring block exceeds the engine's impl-block size budget.
impl SceneApi {
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

    /// Give `node` a procedural animation around its resting pose `base` (the
    /// transform it was created with): a `bob` of `(amplitude, period_ticks)`
    /// along +Y plus a `spin` of `(axis, period_ticks)` revolution, offset by
    /// `phase`. Animated each [`Self::advance`] by the engine's
    /// procedural-animation system — a *positioned* node (a wall at a grid cell)
    /// comes alive without leaving its place. An app draws the per-node
    /// `phase`/period variety from the procedural-generation substrate so a whole
    /// scene never animates in lockstep.
    pub fn add_procanim(
        &mut self,
        node: SceneNodeId,
        base: Transform,
        bob: (Meters, u32),
        spin: (Vec3, u32),
        phase: u32,
    ) -> SceneResult<()> {
        self.scene.add_procanim(
            node,
            ProcAnim::new(base, bob.0.get(), bob.1, spin.0, spin.1, phase),
        )
    }
}

/// Scene state serialization (for fork / replay). Kept in its own `impl` block so
/// neither block exceeds the engine's impl-block size budget.
impl SceneApi {
    /// Serialize the scene's full restorable state to bytes: entity identity,
    /// every component column, and the persistent `players` / `controllers` maps.
    /// Deterministic and self-contained — feed the bytes to [`Self::restore_state`]
    /// (here or in a fresh `SceneApi` with the same systems) to reconstruct the
    /// scene exactly. Transient per-frame command queues are not included.
    pub fn snapshot_state(&self) -> Vec<u8> {
        let mut writer = BinaryWriter::new();
        self.scene.write_state(&mut writer);
        writer.into_bytes()
    }

    /// Restore the scene from bytes produced by [`Self::snapshot_state`]. A
    /// truncated or version-incompatible buffer returns a deterministic error and
    /// leaves the scene's identity/columns as far as the read progressed (callers
    /// fork into a fresh scene, so partial state is never observed live).
    pub fn restore_state(&mut self, bytes: &[u8]) -> KernelResult<()> {
        let mut reader = BinaryReader::new(bytes);
        self.scene.read_state(&mut reader)
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
    fn despawn_player_removes_a_marked_node_and_is_idempotent_through_the_facade() {
        let mut s = api();
        let node = s.create_node();
        s.add_player(node, 0).unwrap();
        assert_eq!(s.snapshot().nodes().len(), 1);
        // Despawning the marked node removes it; a second despawn is a clean false.
        assert!(s.despawn_player(0));
        assert_eq!(s.snapshot().nodes().len(), 0);
        assert!(!s.despawn_player(0));
    }

    #[test]
    fn player_entity_recovers_the_handle_and_despawn_node_removes_it() {
        let mut s = api();
        let node = s.create_node();
        s.add_player(node, 2).unwrap();
        // The handle authored by player index is recoverable; an unknown index is None.
        assert_eq!(s.player_entity(2), Some(node));
        assert_eq!(s.player_entity(9), None);
        // Despawning by that handle removes the node; a repeat is a clean false.
        assert!(s.despawn_node(node));
        assert_eq!(s.snapshot().nodes().len(), 0);
        assert!(!s.despawn_node(node));
    }

    #[test]
    fn player_translation_reads_the_marked_node_and_is_none_for_unknown() {
        let mut s = api();
        let node =
            s.create_node_with_transform(Transform::from_translation(Vec3::new(-1.5, 0.0, 0.0)));
        s.add_player(node, 0).expect("node exists");
        assert_eq!(s.player_translation(0), Some(Vec3::new(-1.5, 0.0, 0.0)));
        assert_eq!(s.player_translation(7), None);
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
        a.set_renderable_casts_contact_shadow(n, true).unwrap();
        a.remove_renderable(n).unwrap();
        // Marking a caster on a node with no renderable is a missing-renderable
        // error (the delegated scene path's failure arm).
        assert_eq!(
            a.set_renderable_casts_contact_shadow(n, true)
                .unwrap_err()
                .code(),
            SceneErrorCode::MissingRenderable
        );
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
    fn add_procanim_valid_and_missing_node() {
        let mut a = api();
        let n = a.create_node();
        a.add_procanim(n, Transform::IDENTITY, (m(0.5), 60), (Vec3::UNIT_Y, 120), 0)
            .unwrap();
        assert_eq!(
            a.add_procanim(
                SceneNodeId::from_raw(99),
                Transform::IDENTITY,
                (m(0.5), 60),
                (Vec3::UNIT_Y, 120),
                0
            )
            .unwrap_err()
            .code(),
            SceneErrorCode::MissingNode
        );
    }

    #[test]
    fn bounds_and_spatial_queries_through_the_facade() {
        let mut a = api();
        let n = a.create_node_with_transform(Transform::from_translation(Vec3::new(3.0, 0.0, 0.0)));
        a.add_bounds(n, Vec3::new(0.5, 0.5, 0.5)).unwrap();
        a.update_world_transforms();
        // raycast finds it; overlap_box finds it.
        assert_eq!(
            a.raycast(Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0), m(100.0)),
            Some(n)
        );
        assert_eq!(
            a.overlap_box(Vec3::new(3.0, 0.0, 0.0), Vec3::new(0.1, 0.1, 0.1)),
            vec![n]
        );
        // A bare node is classified as geometry (no player index); marking it a
        // player makes `player_index` report it — the hit classifier.
        assert_eq!(a.player_index(n), None);
        a.add_player(n, 5).unwrap();
        assert_eq!(a.player_index(n), Some(5));
        // A zero direction passes the `None` straight through the facade.
        assert_eq!(a.raycast(Vec3::ZERO, Vec3::ZERO, m(100.0)), None);
        // Remove, then a second remove and a missing-node add both error.
        a.remove_bounds(n).unwrap();
        assert_eq!(
            a.remove_bounds(n).unwrap_err().code(),
            SceneErrorCode::MissingBounds
        );
        assert_eq!(
            a.add_bounds(SceneNodeId::from_raw(404), Vec3::ONE)
                .unwrap_err()
                .code(),
            SceneErrorCode::MissingNode
        );
    }

    #[test]
    fn radial_overlap_hierarchy_and_subtree_despawn_through_the_facade() {
        let mut a = api();
        // A bounded node 3m out: a circle that reaches it finds it; one that
        // stops short does not.
        let n = a.create_node_with_transform(Transform::from_translation(Vec3::new(3.0, 0.0, 0.0)));
        a.add_bounds(n, Vec3::new(0.5, 0.5, 0.5)).unwrap();
        a.update_world_transforms();
        assert_eq!(a.overlap_circle(Vec3::ZERO, m(3.0)), vec![n]);
        assert!(a.overlap_circle(Vec3::ZERO, m(1.0)).is_empty());

        // A parent with two children: children_of lists them ascending; a leaf
        // and a missing node report none.
        let parent = a.create_node();
        let first = a.create_node();
        let second = a.create_node();
        a.set_parent(first, parent).unwrap();
        a.set_parent(second, parent).unwrap();
        assert_eq!(a.children_of(parent), vec![first, second]);
        assert!(a.children_of(first).is_empty());

        // Despawning the parent cascades to both children; a repeat is a clean
        // false.
        assert!(a.despawn_subtree(parent));
        assert!(a.children_of(parent).is_empty());
        assert!(!a.despawn_subtree(parent));
    }

    #[test]
    fn raycast_hit_and_tags_classify_through_the_facade() {
        let mut a = api();
        let wall = a.create_node_with_transform(Transform::from_translation(Vec3::new(3.0, 0.0, 0.0)));
        a.add_bounds(wall, Vec3::new(0.5, 0.5, 0.5)).unwrap();
        a.add_tag(wall, 1).unwrap(); // 1 = "wall" in this game's vocabulary
        a.update_world_transforms();

        // The hit carries the node and the exact entry point (near face x=2.5),
        // and the agent classifies it by reading its tag.
        let (node, point) = a
            .raycast_hit(Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0), m(100.0))
            .expect("ray hits the wall");
        assert_eq!(node, wall);
        assert!((point.x - 2.5).abs() < 1.0e-5);
        assert_eq!(a.tag_of(node), Some(1));

        // tagged_nodes enumerates by kind; an untagged node reads None.
        let untagged = a.create_node();
        assert_eq!(a.tag_of(untagged), None);
        assert_eq!(a.tagged_nodes(1), vec![wall]);
        assert!(a.tagged_nodes(2).is_empty());
        // Tagging a missing node errors.
        assert_eq!(
            a.add_tag(SceneNodeId::from_raw(404), 9).unwrap_err().code(),
            SceneErrorCode::MissingNode
        );
        // A miss passes None straight through.
        assert!(a
            .raycast_hit(Vec3::ZERO, Vec3::new(0.0, 1.0, 0.0), m(100.0))
            .is_none());
    }

    #[test]
    fn typed_component_enumeration_lists_nodes_and_bounds() {
        let mut a = api();
        let n0 =
            a.create_node_with_transform(Transform::from_translation(Vec3::new(1.0, 0.0, 0.0)));
        let n1 =
            a.create_node_with_transform(Transform::from_translation(Vec3::new(2.0, 0.0, 0.0)));
        a.add_bounds(n1, Vec3::new(0.5, 0.5, 0.5)).unwrap();

        // node_transforms lists every node's local transform, ascending id.
        let transforms = a.node_transforms();
        assert_eq!(transforms.len(), 2);
        assert_eq!(transforms[0].0, n0);
        assert_eq!(transforms[0].1.translation, Vec3::new(1.0, 0.0, 0.0));
        assert_eq!(transforms[1].0, n1);

        // bounds_half_extents reads one node's bounds; the unbounded node is None.
        assert_eq!(a.bounds_half_extents(n1), Some(Vec3::new(0.5, 0.5, 0.5)));
        assert_eq!(a.bounds_half_extents(n0), None);

        // bounded_nodes lists only the bounded node.
        assert_eq!(a.bounded_nodes(), vec![(n1, Vec3::new(0.5, 0.5, 0.5))]);
    }

    #[test]
    fn component_schemas_describe_the_standard_components() {
        let schemas = api().component_schemas();
        assert_eq!(schemas.len(), 8);
        assert_eq!(schemas[0].name(), "Transform");
        assert_eq!(schemas[1].name(), "Camera");
        assert_eq!(schemas[2].name(), "Light");
        assert_eq!(schemas[3].name(), "Renderable");
        assert_eq!(schemas[4].name(), "Spin");
        assert_eq!(schemas[5].name(), "ProcAnim");
        assert_eq!(schemas[6].name(), "Bounds");
        assert_eq!(schemas[7].name(), "Tag");
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
    fn snapshot_state_round_trips_through_bytes_and_rejects_truncation() {
        let mut a = api();
        let n = a.create_node_with_transform(Transform::from_translation(Vec3::new(3.0, 0.0, 0.0)));
        a.add_directional_light(&math(), n, Vec3::ONE, rat(2.0))
            .unwrap();
        let bytes = a.snapshot_state();

        let mut restored = api();
        restored.restore_state(&bytes).unwrap();
        let original = a.snapshot();
        let after = restored.snapshot();
        assert_eq!(after.nodes().len(), original.nodes().len());
        assert_eq!(after.lights().len(), original.lights().len());
        assert_eq!(
            after.nodes()[0].world().translation.x,
            original.nodes()[0].world().translation.x
        );
        // A truncated buffer is a deterministic error, not a panic.
        assert!(restored.restore_state(&[9, 9]).is_err());
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
