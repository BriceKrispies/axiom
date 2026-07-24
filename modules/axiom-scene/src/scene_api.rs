//! The single public facade of the `axiom-scene` module.

use axiom_frame::FrameContext;
use axiom_kernel::{
    BinaryReader, BinaryWriter, KernelResult, Meters, Radians, Ratio, Reflect, TypeSchema,
};
use axiom_math::{Mat4, MathApi, Transform, Vec3};

use crate::animation_ref::AnimationRef;
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
use crate::sdf_shape::SdfShape;
use crate::spin::Spin;
use crate::tag::Tag;
use crate::texture_ref::TextureRef;

/// The players-and-controllers arm of the facade (per-tick command-driven node
/// marks). A child module so neither `impl SceneApi` block exceeds the engine's
/// impl-block size budget.
mod players;

/// The dynamic, kind-keyed component arm of the facade (`set_dynamic`/
/// `get_dynamic`/`query_dynamic`, …) — app-defined components stored type-erased
/// by `Reflect` schema name. A child module so neither this file nor the main
/// `impl SceneApi` block exceeds the engine's size budgets.
mod dynamic;

/// The signed-distance-field authoring arm of the facade (`add_sdf_sphere`/
/// `add_sdf_box`/`add_sdf_plane`/`remove_sdf_shape`). A child module so neither
/// this file nor the main `impl SceneApi` block exceeds the engine's size budgets.
mod sdf;

/// The only public export of `axiom-scene`: a **stateful scene handle**.
/// `SceneApi` *owns* the scene — an ECS world (the ecs layer) where nodes are
/// entities and every node fact is a component column. A consumer builds it
/// **once** and holds it across frames, mutating it incrementally and calling
/// [`Self::advance`] each tick; the world is durable authored state, not
/// rebuilt per frame. (It owns the world rather than handing back a `Scene`
/// value precisely so apps can keep it in a field — the internal types are
/// never re-exported, so they can't be named as field types.)
/// The facade is also where Layer 02 ([`MathApi`]) and Layer 04
/// ([`FrameContext`]) integration lives — see [`Self::advance`] and the
/// camera/light validators.
#[derive(Debug, Default)]
pub struct SceneApi {
    scene: Scene,
    /// A snapshot buffer retained across frames: the render pipeline refreshes it
    /// in place ([`Self::refresh_snapshot`]) and reads it by reference
    /// ([`Self::snapshot_ref`]), so a steady-state frame allocates no fresh
    /// snapshot — the fix for the per-frame wasm-memory churn.
    snapshot: SceneSnapshot,
}

impl SceneApi {
    /// Construct an empty scene with the standard systems registered.
    pub fn new() -> Self {
        SceneApi {
            scene: Scene::new(),
            snapshot: SceneSnapshot::default(),
        }
    }

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

    /// Whether `id` names a live node — an entity that has been created and not
    /// despawned. A despawned or never-created handle reads `false`, so a holder
    /// of a stale [`SceneNodeId`] (e.g. across the wasm boundary) can check
    /// liveness before addressing it.
    pub fn is_alive(&self, id: SceneNodeId) -> bool {
        self.scene.is_node(id)
    }

    /// Make `child` a child of `parent`. Rejects self-parenting, cycles, and
    /// missing ids.
    pub fn set_parent(&mut self, child: SceneNodeId, parent: SceneNodeId) -> SceneResult<()> {
        self.scene.set_parent(child, parent)
    }

    /// Detach `child` from its parent (if any), leaving it a root.
    pub fn clear_parent(&mut self, child: SceneNodeId) -> SceneResult<()> {
        self.scene.clear_parent(child)
    }

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

    /// Advance one engine frame's SYSTEMS only, building no snapshot — the frame
    /// loop steps with this and refreshes the retained snapshot lazily at render
    /// time ([`Self::refresh_snapshot`]), so a stepped frame allocates nothing.
    #[axiom_zones::sim]
    pub fn advance_systems(&mut self, tick: u64, frame: &FrameContext<'_>) {
        self.scene.advance_systems(tick, frame);
    }

    /// Refresh the RETAINED snapshot in place from the scene's current state,
    /// reusing its buffers instead of allocating a fresh snapshot each frame.
    pub fn refresh_snapshot(&mut self) {
        self.snapshot.refresh_from_scene(&self.scene);
    }

    /// The retained snapshot, as of the last [`Self::refresh_snapshot`] — the
    /// render pipeline reads this by reference so a frame allocates none.
    pub fn snapshot_ref(&self) -> &SceneSnapshot {
        &self.snapshot
    }

    /// The reflected schemas of the standard component types a scene is built
    /// from — the scene describing its own shape as data an agent can read.
    pub fn component_schemas(&self) -> Vec<TypeSchema> {
        vec![
            <Transform as Reflect>::SCHEMA,
            Camera::SCHEMA,
            Light::SCHEMA,
            Renderable::SCHEMA,
            SdfShape::SCHEMA,
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

/// The object-binding contract's texture/animation slots: construct the opaque
/// refs and bind them onto a node's renderable. Kept in its own `impl` block so
/// neither block exceeds the engine's impl-block size budget.
impl SceneApi {
    /// Construct an opaque [`TextureRef`] (`0` = the untextured sentinel).
    pub const fn texture_ref(&self, raw: u64) -> TextureRef {
        TextureRef::from_raw(raw)
    }

    /// Construct an opaque [`AnimationRef`] (`0` = the un-animated sentinel).
    pub const fn animation_ref(&self, raw: u64) -> AnimationRef {
        AnimationRef::from_raw(raw)
    }

    /// Bind the albedo texture on `node`'s renderable — the object-binding
    /// contract's texture slot. Pass `texture_ref(0)` to clear it (untextured).
    pub fn set_renderable_texture(
        &mut self,
        node: SceneNodeId,
        texture: TextureRef,
    ) -> SceneResult<()> {
        self.scene.set_renderable_texture(node, texture)
    }

    /// Bind the animation driving `node`'s renderable — the object-binding
    /// contract's animation slot. Pass `animation_ref(0)` to clear it (static).
    pub fn set_renderable_animation(
        &mut self,
        node: SceneNodeId,
        animation: AnimationRef,
    ) -> SceneResult<()> {
        self.scene.set_renderable_animation(node, animation)
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
#[path = "scene_api_tests.rs"]
mod tests;
