//! The deterministic scene, backed by the ECS world model.
//!
//! `Scene` is a thin semantic adapter over [`axiom_ecs::World`]: nodes are
//! entities, and every node fact — local transform, parent link, camera, light,
//! renderable — is a [`axiom_ecs::ComponentColumn`] keyed by the node entity.
//! There is no parallel retained graph; the ECS world *is* the scene. World
//! transforms are produced by the shared [`crate::scene_storage::propagate`]
//! routine (the body of the [`crate::scene_storage::TransformPropagation`]
//! world-system), so on-demand updates and per-frame advances run identical
//! logic.

use std::collections::BTreeMap;

use axiom_frame::FrameContext;
use axiom_kernel::{BinaryReader, BinaryWriter, EntityId, KernelResult, Meters, Reflect};
use axiom_math::{Transform, Vec3};

use crate::controller_command::decode_controller;
use crate::player_command::decode_move;
use crate::procanim::ProcAnim;
use crate::scene_error::SceneError;
use crate::scene_node_id::SceneNodeId;
use crate::scene_result::SceneResult;
use crate::scene_snapshot::SceneSnapshot;
use crate::scene_storage::{
    apply_controller, propagate, ControllerState, ControllerSystem, PlayerMoveSystem,
    ProcAnimSystem, SceneStorage, SpinSystem, TransformPropagation,
};
use crate::spin::Spin;

use axiom_ecs::World;

/// Bounding volumes and the spatial queries (raycast / overlap) over them —
/// Category 2 of the game vocabulary. A child module so it reaches `Scene`'s
/// internals while keeping this file within the engine's per-file size budget.
mod queries;

/// Runtime node lifecycle (despawn) — Category 3 of the game vocabulary. A child
/// module for the same reason.
mod lifecycle;

/// Component attachment (camera / light / renderable) on nodes. A child module
/// so neither `impl Scene` block exceeds the impl-block size budget and this
/// file stays within the per-file size budget.
mod components;

/// Parent/child hierarchy linkage and its cycle guard. A child module for the
/// same size-budget reasons.
mod hierarchy;

/// The deterministic 3D scene: an [`axiom_ecs::World<SceneStorage>`].
///
/// Constructed empty through [`crate::SceneApi::empty_scene`]; every mutation
/// goes through [`crate::SceneApi`] so validation lives in one place. Iteration
/// is by ascending entity id everywhere (the registry and every column are
/// `BTreeMap`/`BTreeSet`-backed).
#[derive(Debug)]
pub struct Scene {
    world: World<SceneStorage>,
}

impl Scene {
    /// Construct an empty scene with the standard systems registered: the
    /// tick-driven animation systems (spin, then procedural bob/spin) run first,
    /// then the per-tick player-move and controller systems, then transform
    /// propagation turns locals into world transforms.
    pub fn new() -> Self {
        let mut world = World::new();
        world.register_system(Box::new(SpinSystem));
        world.register_system(Box::new(ProcAnimSystem));
        world.register_system(Box::new(PlayerMoveSystem));
        world.register_system(Box::new(ControllerSystem));
        world.register_system(Box::new(TransformPropagation));
        Scene { world }
    }

    /// Map a public node id to its backing entity id (identity on the raw u64).
    const fn entity(id: SceneNodeId) -> EntityId {
        EntityId::from_raw(id.raw())
    }

    /// Borrow the backing world (for snapshot construction).
    pub(crate) fn world(&self) -> &World<SceneStorage> {
        &self.world
    }

    /// The number of nodes (entities carrying a local transform).
    pub fn node_count(&self) -> usize {
        self.world.storage().locals.len()
    }

    pub fn camera_count(&self) -> usize {
        self.world.storage().cameras.len()
    }

    pub fn light_count(&self) -> usize {
        self.world.storage().lights.len()
    }

    pub fn renderable_count(&self) -> usize {
        self.world.storage().renderables.len()
    }

    pub fn sdf_shape_count(&self) -> usize {
        self.world.storage().sdf_shapes.len()
    }

    /// Whether `id` names a live node (an entity with a local transform).
    pub(crate) fn is_node(&self, id: SceneNodeId) -> bool {
        self.world.storage().locals.contains(Self::entity(id))
    }

    pub(crate) fn create_node(&mut self, local: Transform) -> SceneNodeId {
        let entity = self.world.spawn();
        let storage = self.world.storage_mut();
        storage.locals.insert(entity, local);
        storage.world_dirty = true;
        SceneNodeId::from_raw(entity.raw())
    }

    pub(crate) fn set_local(&mut self, id: SceneNodeId, local: Transform) -> SceneResult<()> {
        self.is_node(id)
            .then_some(())
            .ok_or_else(|| SceneError::missing_node("set_local: node id not in scene"))
            .map(|()| {
                let storage = self.world.storage_mut();
                storage.locals.insert(Self::entity(id), local);
                storage.world_dirty = true;
            })
    }

    pub(crate) fn local(&self, id: SceneNodeId) -> SceneResult<Transform> {
        self.world
            .storage()
            .locals
            .get(Self::entity(id))
            .copied()
            .ok_or_else(|| SceneError::missing_node("scene does not contain that node"))
    }

    pub(crate) fn world_transform(&self, id: SceneNodeId) -> SceneResult<Transform> {
        let storage = self.world.storage();
        let entity = Self::entity(id);
        storage
            .locals
            .get(entity)
            .copied()
            .map(|local| storage.worlds.get(entity).copied().unwrap_or(local))
            .ok_or_else(|| SceneError::missing_node("scene does not contain that node"))
    }

    /// Every node's `(id, local transform)`, in ascending node-id order — the
    /// enumeration behind typed `query::<Transform>()`. Reads the `locals` column
    /// (entity present ⟺ live node), so it lists exactly the live nodes.
    pub(crate) fn node_transforms(&self) -> Vec<(SceneNodeId, Transform)> {
        self.world
            .storage()
            .locals
            .iter()
            .map(|(entity, local)| (SceneNodeId::from_raw(entity.raw()), *local))
            .collect()
    }

    /// The local-space translation of the node marked with `player` index, if
    /// such a node exists. Player nodes are unparented, so this is also their
    /// world translation. It resolves the index exactly as the player-move system
    /// does (deterministic `BTreeMap` scan), giving a read-only projection of
    /// authoritative scene state — not a separate, divergeable source of truth.
    pub(crate) fn player_translation(&self, player: u32) -> Option<Vec3> {
        let storage = self.world.storage();
        storage
            .players
            .iter()
            .find_map(|(&entity, &index)| (index == player).then_some(entity))
            .and_then(|entity| storage.locals.get(entity).map(|local| local.translation))
    }

    /// The node handle marked with `player` index, if any. Resolves the index the
    /// same deterministic way [`Self::player_translation`] does, but hands back
    /// the node's [`SceneNodeId`] — so a caller that authored players by index can
    /// recover the first-class handle to address them by Entity afterward.
    pub(crate) fn player_entity(&self, player: u32) -> Option<SceneNodeId> {
        self.world
            .storage()
            .players
            .iter()
            .find_map(|(&entity, &index)| {
                (index == player).then(|| SceneNodeId::from_raw(entity.raw()))
            })
    }

    pub(crate) fn parent_of(&self, id: SceneNodeId) -> Option<SceneNodeId> {
        self.world
            .storage()
            .parents
            .get(Self::entity(id))
            .map(|p| SceneNodeId::from_raw(p.raw()))
    }

    /// Mark `node` as the controllable node for `player` index, so per-tick move
    /// commands addressed to that index translate it (via [`PlayerMoveSystem`]).
    pub(crate) fn add_player(&mut self, node: SceneNodeId, player: u32) -> SceneResult<()> {
        self.is_node(node)
            .then_some(())
            .ok_or_else(|| SceneError::missing_node("add_player: node id not in scene"))
            .map(|()| {
                self.world
                    .storage_mut()
                    .players
                    .insert(Self::entity(node), player);
            })
    }

    /// Mark `node` as the first-person controller node for `index`, so per-tick
    /// controller inputs addressed to that index yaw and move it (via
    /// [`ControllerSystem`]).
    pub(crate) fn add_controller(&mut self, node: SceneNodeId, index: u32) -> SceneResult<()> {
        self.is_node(node)
            .then_some(())
            .ok_or_else(|| SceneError::missing_node("add_controller: node id not in scene"))
            .map(|()| {
                self.world.storage_mut().controllers.insert(
                    Self::entity(node),
                    ControllerState {
                        index,
                        yaw: 0.0,
                        pitch: 0.0,
                    },
                );
            })
    }

    /// Apply one first-person controller input to controller `index`'s node
    /// **immediately** — accumulate yaw/pitch, rebuild the node rotation, move it
    /// along the yaw-only frame — then recompute world transforms now. The
    /// zero-lag path a host that owns its own frame loop uses to drive the camera
    /// between ticks (the SDK's `RunningApp::control`); it reuses the exact logic
    /// the per-tick [`ControllerSystem`] runs, so the two never diverge. An unknown
    /// index is a no-op.
    pub(crate) fn control_now(
        &mut self,
        index: u32,
        move_local: Vec3,
        yaw_delta: f32,
        pitch_delta: f32,
        seat_y: Option<Meters>,
    ) {
        apply_controller(
            self.world.storage_mut(),
            index,
            move_local,
            yaw_delta,
            pitch_delta,
            seat_y,
        );
        // `apply_controller` writes locals straight to storage (not via `set_local`), so
        // mark dirty here before the propagation below observes it.
        self.world.storage_mut().world_dirty = true;
        self.update_world_transforms();
    }

    /// Propagate every node's world transform from its local — but ONLY if a transform
    /// or parent link changed since the last propagation (`world_dirty`). This coalescing
    /// is what lets a caller that moves N nodes and then reads once pay a single
    /// whole-scene propagation instead of one per moved node: the runtime marks each
    /// `setNodeTransform` dirty (O(1)) and this runs once before the frame is read.
    pub(crate) fn update_world_transforms(&mut self) {
        self.world.storage().world_dirty.then(|| {
            let ids: Vec<EntityId> = self.world.entities().iter().collect();
            propagate(ids.into_iter(), self.world.storage_mut());
            self.world.storage_mut().world_dirty = false;
        });
    }

    /// Advance the scene for one engine frame: run the registered systems
    /// (spin, player-move, controller, then transform propagation) iff the
    /// frame is active (not skipped, ran at least one runtime step), then
    /// return the deterministic snapshot taken after whatever update happened.
    pub(crate) fn advance(&mut self, tick: u64, frame: &FrameContext<'_>) -> SceneSnapshot {
        let moves: Vec<(u32, Vec3)> = frame.commands().iter().filter_map(decode_move).collect();
        let controls: Vec<(u32, Vec3, f32, f32, Option<Meters>)> = frame
            .commands()
            .iter()
            .filter_map(decode_controller)
            .collect();
        self.world.storage_mut().pending_moves = moves;
        self.world.storage_mut().pending_controls = controls;
        self.world.advance(tick, frame);
        self.snapshot()
    }

    /// A deterministic value snapshot of the scene's current state.
    pub(crate) fn snapshot(&self) -> SceneSnapshot {
        SceneSnapshot::from_scene(self)
    }
}

/// Data-declared animation authoring: attach the engine's tick-driven animation
/// components to a node. Kept in its own `impl` block so neither block exceeds the
/// engine's impl-block size budget.
impl Scene {
    pub(crate) fn add_spin(&mut self, node: SceneNodeId, spin: Spin) -> SceneResult<()> {
        self.is_node(node)
            .then_some(())
            .ok_or_else(|| SceneError::missing_node("add_spin: node id not in scene"))
            .map(|()| {
                self.world
                    .storage_mut()
                    .spins
                    .insert(Self::entity(node), spin);
            })
    }

    pub(crate) fn add_procanim(&mut self, node: SceneNodeId, anim: ProcAnim) -> SceneResult<()> {
        self.is_node(node)
            .then_some(())
            .ok_or_else(|| SceneError::missing_node("add_procanim: node id not in scene"))
            .map(|()| {
                self.world
                    .storage_mut()
                    .procanims
                    .insert(Self::entity(node), anim);
            })
    }
}

/// Scene state (de)serialization for fork / replay. Kept in its own `impl` block
/// so neither block exceeds the engine's impl-block size budget.
impl Scene {
    /// Serialize the scene's full restorable state: the ECS world snapshot
    /// (entity identity + every component column) followed by the two persistent
    /// non-column maps — `players` and `controllers` (the latter carries each
    /// first-person camera's accumulated yaw/pitch). The transient per-frame
    /// `pending_*` command queues are deliberately omitted; they are re-staged
    /// from frame commands every tick and carry no state across frames.
    pub(crate) fn write_state(&self, writer: &mut BinaryWriter) {
        self.world.write_snapshot(writer);
        let storage = self.world.storage();
        writer.write_u32(storage.players.len() as u32);
        storage.players.iter().for_each(|(entity, &index)| {
            entity.reflect_write(writer);
            index.reflect_write(writer);
        });
        writer.write_u32(storage.controllers.len() as u32);
        storage.controllers.iter().for_each(|(entity, state)| {
            entity.reflect_write(writer);
            state.reflect_write(writer);
        });
    }

    /// Restore a scene from bytes produced by [`Self::write_state`]: the ECS world
    /// (identity + columns) then the `players` and `controllers` maps. Systems are
    /// untouched (they are code, not state). A truncated/incompatible buffer
    /// returns a deterministic error rather than panicking.
    pub(crate) fn read_state(&mut self, reader: &mut BinaryReader<'_>) -> KernelResult<()> {
        self.world
            .read_snapshot(reader)
            .and_then(|()| read_players(reader))
            .and_then(|players| read_controllers(reader).map(|controllers| (players, controllers)))
            .map(|(players, controllers)| {
                let storage = self.world.storage_mut();
                storage.players = players;
                storage.controllers = controllers;
                // Restored locals need a fresh propagation before any query/present reads
                // world transforms (the serialized `worlds` may predate propagation).
                storage.world_dirty = true;
            })
    }
}

/// Read the `players` map: a count then that many `(entity, index)` pairs.
fn read_players(reader: &mut BinaryReader<'_>) -> KernelResult<BTreeMap<EntityId, u32>> {
    reader.read_u32().and_then(|count| {
        (0..count).try_fold(BTreeMap::new(), |mut players, _| {
            EntityId::reflect_read(reader).and_then(|entity| {
                u32::reflect_read(reader).map(|index| {
                    players.insert(entity, index);
                    players
                })
            })
        })
    })
}

/// Read the `controllers` map: a count then that many `(entity, state)` pairs.
fn read_controllers(
    reader: &mut BinaryReader<'_>,
) -> KernelResult<BTreeMap<EntityId, ControllerState>> {
    reader.read_u32().and_then(|count| {
        (0..count).try_fold(BTreeMap::new(), |mut controllers, _| {
            EntityId::reflect_read(reader).and_then(|entity| {
                ControllerState::reflect_read(reader).map(|state| {
                    controllers.insert(entity, state);
                    controllers
                })
            })
        })
    })
}

impl Default for Scene {
    fn default() -> Self {
        Scene::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene_error_code::SceneErrorCode;

    fn node(s: &mut Scene) -> SceneNodeId {
        s.create_node(Transform::IDENTITY)
    }

    #[test]
    fn empty_and_default_scenes_have_no_nodes() {
        let a = Scene::new();
        let b = Scene::default();
        assert_eq!(a.node_count(), 0);
        assert_eq!(a.camera_count(), 0);
        assert_eq!(a.light_count(), 0);
        assert_eq!(a.renderable_count(), 0);
        assert_eq!(b.node_count(), 0);
    }

    #[test]
    fn create_node_assigns_monotonic_ids_and_stores_local() {
        let mut s = Scene::new();
        let a = s.create_node(Transform::from_translation(Vec3::new(1.0, 2.0, 3.0)));
        let b = node(&mut s);
        assert_eq!(a.raw(), 1);
        assert_eq!(b.raw(), 2);
        assert_eq!(s.node_count(), 2);
        assert_eq!(s.local(a).unwrap().translation.x, 1.0);
    }

    #[test]
    fn set_local_present_and_missing() {
        let mut s = Scene::new();
        let n = node(&mut s);
        s.set_local(n, Transform::from_translation(Vec3::new(5.0, 0.0, 0.0)))
            .unwrap();
        assert_eq!(s.local(n).unwrap().translation.x, 5.0);
        let err = s
            .set_local(SceneNodeId::from_raw(99), Transform::IDENTITY)
            .unwrap_err();
        assert_eq!(err.code(), SceneErrorCode::MissingNode);
    }

    #[test]
    fn local_query_missing_fails() {
        let s = Scene::new();
        assert_eq!(
            s.local(SceneNodeId::from_raw(7)).unwrap_err().code(),
            SceneErrorCode::MissingNode
        );
    }

    #[test]
    fn world_transform_default_present_and_missing() {
        let mut s = Scene::new();
        let n = s.create_node(Transform::from_translation(Vec3::new(4.0, 0.0, 0.0)));
        // No propagation yet: world falls back to local.
        assert_eq!(s.world_transform(n).unwrap().translation.x, 4.0);
        assert_eq!(
            s.world_transform(SceneNodeId::from_raw(99))
                .unwrap_err()
                .code(),
            SceneErrorCode::MissingNode
        );
    }

    #[test]
    fn add_spin_present_and_missing() {
        let mut s = Scene::new();
        let n = node(&mut s);
        s.add_spin(n, Spin::new(Vec3::UNIT_Y, 360)).unwrap();
        assert_eq!(
            s.add_spin(SceneNodeId::from_raw(99), Spin::new(Vec3::UNIT_Y, 360))
                .unwrap_err()
                .code(),
            SceneErrorCode::MissingNode
        );
    }

    #[test]
    fn update_world_transforms_propagates_parent_to_child() {
        let mut s = Scene::new();
        let p = s.create_node(Transform::from_translation(Vec3::new(1.0, 0.0, 0.0)));
        let c = s.create_node(Transform::from_translation(Vec3::new(0.0, 2.0, 0.0)));
        s.set_parent(c, p).unwrap();
        s.update_world_transforms();
        let w = s.world_transform(c).unwrap();
        assert_eq!(w.translation.x, 1.0);
        assert_eq!(w.translation.y, 2.0);
    }

    #[test]
    fn update_world_transforms_coalesces_when_nothing_changed() {
        // A mutation marks the scene dirty, so the first propagation runs; a second call
        // with no mutation in between is a no-op (the `world_dirty == false` arm), and a
        // subsequent local move re-arms it. This is the coalescing that turns N per-frame
        // `setNodeTransform` calls into one whole-scene propagation.
        let mut s = Scene::new();
        let n = s.create_node(Transform::from_translation(Vec3::new(1.0, 0.0, 0.0)));
        s.update_world_transforms();
        assert_eq!(s.world_transform(n).unwrap().translation.x, 1.0);
        // No mutation ⇒ the second call takes the clean (no-op) arm and leaves worlds intact.
        s.update_world_transforms();
        assert_eq!(s.world_transform(n).unwrap().translation.x, 1.0);
        // A fresh local move re-arms the flag and the next propagation reflects it.
        s.set_local(n, Transform::from_translation(Vec3::new(4.0, 0.0, 0.0)))
            .unwrap();
        s.update_world_transforms();
        assert_eq!(s.world_transform(n).unwrap().translation.x, 4.0);
    }
}

#[cfg(test)]
mod frame_tests {
    use super::*;
    use crate::camera::Camera;
    use crate::light::Light;
    use crate::renderable::Renderable;
    use axiom_frame::FrameApi;
    use axiom_host::{
        HostBoundaryConfig, HostFrameInput, HostFrameReport, HostLifecycleSignal,
        HostLifecycleState, HostStepPlan, HostViewport,
    };
    use axiom_kernel::Ratio;
    use axiom_math::Vec3;

    /// Build an `EngineFrame` for the given elapsed-nanos and lifecycle.
    fn engine_frame(elapsed: u64, started: bool) -> axiom_frame::EngineFrame {
        engine_frame_with(elapsed, started, Vec::new())
    }

    fn engine_frame_with(
        elapsed: u64,
        started: bool,
        commands: Vec<axiom_frame::FrameCommand>,
    ) -> axiom_frame::EngineFrame {
        let vp = HostViewport::new(100, 100, Ratio::new(1.0).unwrap()).unwrap();
        let cfg = HostBoundaryConfig::new(1_000, 5).unwrap();
        let lifecycle = if started {
            HostLifecycleState::initial().apply(HostLifecycleSignal::Started)
        } else {
            HostLifecycleState::initial()
        };
        let input = HostFrameInput::new(1, elapsed, vp);
        let plan = HostStepPlan::build(&input, &cfg, &lifecycle, 0);
        let report = HostFrameReport::new(
            input.sequence(),
            plan,
            plan.steps(),
            Vec::new(),
            vp,
            lifecycle,
        );
        FrameApi::new()
            .engine_frame_from_host_report(&report, elapsed, commands)
            .unwrap()
    }

    fn parented_scene() -> (Scene, SceneNodeId) {
        let mut s = Scene::new();
        let p = s.create_node(Transform::from_translation(Vec3::new(3.0, 0.0, 0.0)));
        let c = s.create_node(Transform::from_translation(Vec3::new(0.0, 4.0, 0.0)));
        s.set_parent(c, p).unwrap();
        (s, c)
    }

    #[test]
    fn advance_propagates_on_active_frame_with_steps() {
        let (mut s, _c) = parented_scene();
        let frame = engine_frame(1_000, true);
        let ctx = FrameContext::new(&frame);
        let snap = s.advance(0, &ctx);
        let child = snap.nodes().iter().find(|n| n.parent().is_some()).unwrap();
        assert_eq!(child.world().translation.x, 3.0);
        assert_eq!(child.world().translation.y, 4.0);
    }

    #[test]
    fn advance_animates_a_spun_node_via_the_registered_spin_system() {
        use crate::spin::Spin;
        let mut s = Scene::new();
        let n = s.create_node(Transform::IDENTITY);
        s.add_spin(n, Spin::new(Vec3::UNIT_Y, 4)).unwrap();
        let frame = engine_frame(1_000, true);
        let ctx = FrameContext::new(&frame);
        s.advance(3, &ctx);
        // The spin system set the node's local to the rotation for tick 3.
        let expected = Spin::new(Vec3::UNIT_Y, 4).rotation_at(3).unwrap();
        assert_eq!(s.local(n).unwrap().rotation, expected);
    }

    #[test]
    fn advance_applies_a_move_command_to_the_addressed_player() {
        use crate::player_command::encode_move;
        let mut s = Scene::new();
        let node = s.create_node(Transform::IDENTITY);
        s.add_player(node, 0).unwrap();
        // A frame carrying a move for player 0 by (+1, +2).
        let frame = engine_frame_with(
            1_000,
            true,
            vec![encode_move(0, 0, Vec3::new(1.0, 2.0, 0.0))],
        );
        let snap = s.advance(0, &FrameContext::new(&frame));
        assert_eq!(s.local(node).unwrap().translation.x, 1.0);
        assert_eq!(s.local(node).unwrap().translation.y, 2.0);
        let moved = snap.nodes().iter().find(|n| n.world().translation.x == 1.0);
        assert!(moved.is_some(), "snapshot reflects the moved player");
    }

    #[test]
    fn advance_with_no_commands_leaves_the_player_put() {
        let mut s = Scene::new();
        let node = s.create_node(Transform::IDENTITY);
        s.add_player(node, 0).unwrap();
        let frame = engine_frame(1_000, true); // no commands
        s.advance(0, &FrameContext::new(&frame));
        assert_eq!(s.local(node).unwrap().translation.x, 0.0);
    }

    #[test]
    fn add_player_rejects_a_missing_node() {
        let mut s = Scene::new();
        assert!(s.add_player(SceneNodeId::from_raw(99), 0).is_err());
    }

    #[test]
    fn advance_applies_a_controller_command_to_the_addressed_node() {
        use crate::controller_command::encode_controller;
        let mut s = Scene::new();
        let node = s.create_node(Transform::IDENTITY);
        s.add_controller(node, 0).unwrap();
        // A frame carrying a forward move (local -Z by 1) for controller 0, no look.
        let frame = encode_controller(0, 0, Vec3::new(0.0, 0.0, -1.0), 0.0, 0.0, None);
        let engine_frame = engine_frame_with(1_000, true, vec![frame]);
        s.advance(0, &FrameContext::new(&engine_frame));
        // Forward with identity facing translates along -Z.
        assert!((s.local(node).unwrap().translation.z + 1.0).abs() < 1.0e-5);
    }

    #[test]
    fn add_controller_rejects_a_missing_node() {
        let mut s = Scene::new();
        assert!(s.add_controller(SceneNodeId::from_raw(99), 0).is_err());
    }

    #[test]
    fn advance_skips_propagation_on_skipped_frame() {
        let (mut s, _c) = parented_scene();
        let frame = engine_frame(1_000, false); // never started -> skipped
        let ctx = FrameContext::new(&frame);
        assert!(ctx.is_skipped());
        let snap = s.advance(0, &ctx);
        let child = snap.nodes().iter().find(|n| n.parent().is_some()).unwrap();
        // No propagation: world fell back to local.
        assert_eq!(child.world().translation.x, 0.0);
        assert_eq!(child.world().translation.y, 4.0);
    }

    #[test]
    fn advance_skips_propagation_on_active_zero_step_frame() {
        let (mut s, _c) = parented_scene();
        let frame = engine_frame(0, true); // visible but elapsed 0 -> zero steps
        let ctx = FrameContext::new(&frame);
        assert!(!ctx.is_skipped());
        assert_eq!(ctx.runtime_step_count(), 0);
        let snap = s.advance(0, &ctx);
        let child = snap.nodes().iter().find(|n| n.parent().is_some()).unwrap();
        assert_eq!(child.world().translation.x, 0.0);
    }

    #[test]
    fn write_state_read_state_round_trips_full_scene_and_resumes_deterministically() {
        use crate::controller_command::encode_controller;
        use crate::material_ref::MaterialRef;
        use crate::mesh_ref::MeshRef;
        use crate::player_command::encode_move;
        use axiom_kernel::{Meters, Radians, Ratio};
        use axiom_math::MathApi;

        let math = MathApi::new();
        let mut s = Scene::new();
        // One of each persistent column plus both non-column maps: a camera +
        // first-person controller node, a light, a spinning renderable, a player.
        let cam = s.create_node(Transform::from_translation(Vec3::new(0.0, 1.0, 5.0)));
        s.add_camera(
            cam,
            Camera::perspective(
                &math,
                Radians::new(1.2).unwrap(),
                Ratio::new(1.5).unwrap(),
                Meters::new(0.1).unwrap(),
                Meters::new(100.0).unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        s.add_controller(cam, 0).unwrap();
        let light = s.create_node(Transform::IDENTITY);
        s.add_light(
            light,
            Light::directional(&math, Vec3::ONE, Ratio::new(1.0).unwrap()).unwrap(),
        )
        .unwrap();
        let cube = s.create_node(Transform::IDENTITY);
        s.add_renderable(
            cube,
            Renderable::new(MeshRef::from_raw(1), MaterialRef::from_raw(1)).unwrap(),
        )
        .unwrap();
        s.add_spin(cube, Spin::new(Vec3::UNIT_Y, 120)).unwrap();
        let player = s.create_node(Transform::IDENTITY);
        s.add_player(player, 1).unwrap();

        // Drive a controller look + a player move so the controller accumulates
        // yaw/pitch and the player translates — genuine per-frame state.
        let look = encode_controller(0, 0, Vec3::new(0.0, 0.0, -1.0), 0.5, 0.3, None);
        let mv = encode_move(1, 1, Vec3::new(2.0, 0.0, 0.0));
        let frame = engine_frame_with(1_000, true, vec![look, mv]);
        s.advance(7, &FrameContext::new(&frame));

        let mut writer = BinaryWriter::new();
        s.write_state(&mut writer);
        let bytes = writer.into_bytes();
        let mut restored = Scene::new();
        restored.read_state(&mut BinaryReader::new(&bytes)).unwrap();

        assert_eq!(restored.node_count(), s.node_count());
        assert_eq!(
            restored.world.storage().controllers,
            s.world.storage().controllers
        );
        assert_eq!(restored.world.storage().players, s.world.storage().players);
        let ctrl = restored
            .world
            .storage()
            .controllers
            .values()
            .next()
            .unwrap();
        assert_eq!(ctrl.yaw, 0.5);
        assert_eq!(ctrl.pitch, 0.3);

        // Resuming the restored scene equals resuming the original: forking is
        // deterministic.
        let next = engine_frame(1_000, true);
        s.advance(8, &FrameContext::new(&next));
        restored.advance(8, &FrameContext::new(&next));
        assert_eq!(
            restored.world.storage().controllers,
            s.world.storage().controllers
        );
        assert_eq!(
            restored.local(player).unwrap().translation.x,
            s.local(player).unwrap().translation.x
        );
        assert_eq!(restored.local(cube).unwrap(), s.local(cube).unwrap());
    }

    #[test]
    fn read_state_rejects_truncated_bytes() {
        let mut s = Scene::new();
        assert!(s.read_state(&mut BinaryReader::new(&[1, 2, 3])).is_err());
    }
}
