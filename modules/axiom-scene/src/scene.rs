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

use std::collections::BTreeSet;

use axiom_frame::FrameContext;
use axiom_kernel::EntityId;
use axiom_math::{Transform, Vec3};

use crate::camera::Camera;
use crate::controller_command::decode_controller;
use crate::light::Light;
use crate::player_command::decode_move;
use crate::renderable::Renderable;
use crate::scene_error::SceneError;
use crate::scene_node_id::SceneNodeId;
use crate::scene_result::SceneResult;
use crate::scene_snapshot::SceneSnapshot;
use crate::scene_storage::{
    propagate, ControllerSystem, PlayerMoveSystem, SceneStorage, SpinSystem, TransformPropagation,
};
use crate::spin::Spin;

use axiom_ecs::World;

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
    /// Construct an empty scene with the standard systems registered: spin runs
    /// first (it drives local rotations from the tick), then transform
    /// propagation turns locals into world transforms.
    pub fn new() -> Self {
        let mut world = World::new();
        world.register_system(Box::new(SpinSystem));
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

    // --- Counts (read-only) ---

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

    /// Whether `id` names a live node (an entity with a local transform).
    fn is_node(&self, id: SceneNodeId) -> bool {
        self.world.storage().locals.contains(Self::entity(id))
    }

    // --- Node lifecycle / transforms (crate-private; reached through SceneApi) ---

    pub(crate) fn create_node(&mut self, local: Transform) -> SceneNodeId {
        let entity = self.world.spawn();
        self.world.storage_mut().locals.insert(entity, local);
        SceneNodeId::from_raw(entity.raw())
    }

    pub(crate) fn set_local(&mut self, id: SceneNodeId, local: Transform) -> SceneResult<()> {
        if !self.is_node(id) {
            return Err(SceneError::missing_node("set_local: node id not in scene"));
        }
        self.world
            .storage_mut()
            .locals
            .insert(Self::entity(id), local);
        Ok(())
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
        match storage.locals.get(entity) {
            None => Err(SceneError::missing_node("scene does not contain that node")),
            Some(&local) => Ok(storage.worlds.get(entity).copied().unwrap_or(local)),
        }
    }

    pub(crate) fn parent_of(&self, id: SceneNodeId) -> Option<SceneNodeId> {
        self.world
            .storage()
            .parents
            .get(Self::entity(id))
            .map(|p| SceneNodeId::from_raw(p.raw()))
    }

    // --- Hierarchy ---

    pub(crate) fn set_parent(
        &mut self,
        child: SceneNodeId,
        parent: SceneNodeId,
    ) -> SceneResult<()> {
        if child == parent {
            return Err(SceneError::self_parenting(
                "set_parent: a node cannot be its own parent",
            ));
        }
        if !self.is_node(child) {
            return Err(SceneError::missing_node(
                "set_parent: child id not in scene",
            ));
        }
        if !self.is_node(parent) {
            return Err(SceneError::missing_node(
                "set_parent: parent id not in scene",
            ));
        }
        if self.would_introduce_cycle(Self::entity(child), Self::entity(parent)) {
            return Err(SceneError::hierarchy_cycle(
                "set_parent: assignment would introduce a cycle",
            ));
        }
        self.world
            .storage_mut()
            .parents
            .insert(Self::entity(child), Self::entity(parent));
        Ok(())
    }

    pub(crate) fn clear_parent(&mut self, child: SceneNodeId) -> SceneResult<()> {
        if !self.is_node(child) {
            return Err(SceneError::missing_node(
                "clear_parent: child id not in scene",
            ));
        }
        self.world.storage_mut().parents.remove(Self::entity(child));
        Ok(())
    }

    /// Walk upward from `new_parent`; the assignment cycles iff we reach
    /// `child` (a direct ancestor loop) or revisit a node (a pre-existing
    /// cycle, only reachable by corrupting the parents column directly).
    fn would_introduce_cycle(&self, child: EntityId, new_parent: EntityId) -> bool {
        let parents = &self.world.storage().parents;
        let mut cursor = Some(new_parent);
        let mut visited: BTreeSet<EntityId> = BTreeSet::new();
        while let Some(id) = cursor {
            if id == child {
                return true;
            }
            if !visited.insert(id) {
                return true;
            }
            cursor = parents.get(id).copied();
        }
        false
    }

    // --- Components ---

    pub(crate) fn add_camera(&mut self, node: SceneNodeId, camera: Camera) -> SceneResult<()> {
        if !self.is_node(node) {
            return Err(SceneError::missing_node("add_camera: node id not in scene"));
        }
        self.world
            .storage_mut()
            .cameras
            .insert(Self::entity(node), camera);
        Ok(())
    }

    pub(crate) fn camera(&self, node: SceneNodeId) -> Option<&Camera> {
        self.world.storage().cameras.get(Self::entity(node))
    }

    pub(crate) fn remove_camera(&mut self, node: SceneNodeId) -> SceneResult<()> {
        if self
            .world
            .storage_mut()
            .cameras
            .remove(Self::entity(node))
            .is_none()
        {
            return Err(SceneError::missing_camera(
                "remove_camera: node has no camera",
            ));
        }
        Ok(())
    }

    pub(crate) fn add_light(&mut self, node: SceneNodeId, light: Light) -> SceneResult<()> {
        if !self.is_node(node) {
            return Err(SceneError::missing_node("add_light: node id not in scene"));
        }
        self.world
            .storage_mut()
            .lights
            .insert(Self::entity(node), light);
        Ok(())
    }

    pub(crate) fn remove_light(&mut self, node: SceneNodeId) -> SceneResult<()> {
        if self
            .world
            .storage_mut()
            .lights
            .remove(Self::entity(node))
            .is_none()
        {
            return Err(SceneError::missing_light("remove_light: node has no light"));
        }
        Ok(())
    }

    pub(crate) fn add_renderable(
        &mut self,
        node: SceneNodeId,
        renderable: Renderable,
    ) -> SceneResult<()> {
        if !self.is_node(node) {
            return Err(SceneError::missing_node(
                "add_renderable: node id not in scene",
            ));
        }
        self.world
            .storage_mut()
            .renderables
            .insert(Self::entity(node), renderable);
        Ok(())
    }

    pub(crate) fn remove_renderable(&mut self, node: SceneNodeId) -> SceneResult<()> {
        if self
            .world
            .storage_mut()
            .renderables
            .remove(Self::entity(node))
            .is_none()
        {
            return Err(SceneError::missing_renderable(
                "remove_renderable: node has no renderable",
            ));
        }
        Ok(())
    }

    pub(crate) fn set_renderable_visible(
        &mut self,
        node: SceneNodeId,
        visible: bool,
    ) -> SceneResult<()> {
        match self
            .world
            .storage_mut()
            .renderables
            .get_mut(Self::entity(node))
        {
            Some(r) => {
                r.set_visible(visible);
                Ok(())
            }
            None => Err(SceneError::missing_renderable(
                "set_renderable_visible: node has no renderable",
            )),
        }
    }

    pub(crate) fn add_spin(&mut self, node: SceneNodeId, spin: Spin) -> SceneResult<()> {
        if !self.is_node(node) {
            return Err(SceneError::missing_node("add_spin: node id not in scene"));
        }
        self.world
            .storage_mut()
            .spins
            .insert(Self::entity(node), spin);
        Ok(())
    }

    /// Mark `node` as the controllable node for `player` index, so per-tick move
    /// commands addressed to that index translate it (via [`PlayerMoveSystem`]).
    pub(crate) fn add_player(&mut self, node: SceneNodeId, player: u32) -> SceneResult<()> {
        if !self.is_node(node) {
            return Err(SceneError::missing_node("add_player: node id not in scene"));
        }
        self.world
            .storage_mut()
            .players
            .insert(Self::entity(node), player);
        Ok(())
    }

    /// Mark `node` as the first-person controller node for `index`, so per-tick
    /// controller inputs addressed to that index yaw and move it (via
    /// [`ControllerSystem`]).
    pub(crate) fn add_controller(&mut self, node: SceneNodeId, index: u32) -> SceneResult<()> {
        if !self.is_node(node) {
            return Err(SceneError::missing_node(
                "add_controller: node id not in scene",
            ));
        }
        self.world
            .storage_mut()
            .controllers
            .insert(Self::entity(node), index);
        Ok(())
    }

    // --- Transform propagation ---

    /// Recompute every node's world transform now, regardless of frame state.
    pub(crate) fn update_world_transforms(&mut self) {
        let ids: Vec<EntityId> = self.world.entities().iter().collect();
        propagate(ids.into_iter(), self.world.storage_mut());
    }

    /// Advance the scene for one engine frame: recompute world transforms iff
    /// the frame is active (not skipped, ran at least one runtime step), then
    /// return the deterministic snapshot taken after whatever update happened.
    pub(crate) fn advance(&mut self, tick: u64, frame: &FrameContext<'_>) -> SceneSnapshot {
        // Stage this frame's player-move and controller commands for the move and
        // controller systems to apply.
        let moves: Vec<(u32, Vec3)> = frame.commands().iter().filter_map(decode_move).collect();
        let controls: Vec<(u32, Vec3, f32)> = frame
            .commands()
            .iter()
            .filter_map(decode_controller)
            .collect();
        self.world.storage_mut().pending_moves = moves;
        self.world.storage_mut().pending_controls = controls;
        // The ECS scheduler runs the registered systems (spin, player-move,
        // controller, then transform propagation) at logical time `tick`, gated on
        // the frame (skipped / zero-step frames run nothing).
        self.world.advance(tick, frame);
        self.snapshot()
    }

    /// A deterministic value snapshot of the scene's current state.
    pub(crate) fn snapshot(&self) -> SceneSnapshot {
        SceneSnapshot::from_scene(self)
    }
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
    use axiom_kernel::{Meters, Radians, Ratio};
    use axiom_math::{MathApi, Vec3};

    fn math() -> MathApi {
        MathApi::new()
    }

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
        // Missing node.
        assert_eq!(
            s.world_transform(SceneNodeId::from_raw(99))
                .unwrap_err()
                .code(),
            SceneErrorCode::MissingNode
        );
    }

    #[test]
    fn set_parent_links_and_parent_of_reports_it() {
        let mut s = Scene::new();
        let p = node(&mut s);
        let c = node(&mut s);
        s.set_parent(c, p).unwrap();
        assert_eq!(s.parent_of(c), Some(p));
        assert_eq!(s.parent_of(p), None);
    }

    #[test]
    fn self_parenting_fails() {
        let mut s = Scene::new();
        let n = node(&mut s);
        assert_eq!(
            s.set_parent(n, n).unwrap_err().code(),
            SceneErrorCode::SelfParenting
        );
    }

    #[test]
    fn set_parent_missing_child_or_parent_fails() {
        let mut s = Scene::new();
        let p = node(&mut s);
        assert_eq!(
            s.set_parent(SceneNodeId::from_raw(99), p)
                .unwrap_err()
                .code(),
            SceneErrorCode::MissingNode
        );
        let c = node(&mut s);
        assert_eq!(
            s.set_parent(c, SceneNodeId::from_raw(99))
                .unwrap_err()
                .code(),
            SceneErrorCode::MissingNode
        );
    }

    #[test]
    fn cycle_assignment_is_rejected() {
        // chain: a <- b <- c ; making a a child of c would loop.
        let mut s = Scene::new();
        let a = node(&mut s);
        let b = node(&mut s);
        let c = node(&mut s);
        s.set_parent(b, a).unwrap();
        s.set_parent(c, b).unwrap();
        assert_eq!(
            s.set_parent(a, c).unwrap_err().code(),
            SceneErrorCode::HierarchyCycle
        );
    }

    #[test]
    fn preexisting_cycle_trips_the_visited_guard() {
        // Corrupt the parents column into a 2-cycle (a->b->a), unreachable via
        // the public API, then walk from `a` looking for an unrelated child:
        // the walk revisits `a` and the visited-guard returns true.
        let mut s = Scene::new();
        let a = node(&mut s);
        let b = node(&mut s);
        s.world
            .storage_mut()
            .parents
            .insert(Scene::entity(a), Scene::entity(b));
        s.world
            .storage_mut()
            .parents
            .insert(Scene::entity(b), Scene::entity(a));
        assert!(s.would_introduce_cycle(Scene::entity(SceneNodeId::from_raw(99)), Scene::entity(a)));
    }

    #[test]
    fn clear_parent_present_and_missing() {
        let mut s = Scene::new();
        let p = node(&mut s);
        let c = node(&mut s);
        s.set_parent(c, p).unwrap();
        s.clear_parent(c).unwrap();
        assert_eq!(s.parent_of(c), None);
        // Clearing a root (no parent) still succeeds.
        s.clear_parent(p).unwrap();
        // Missing node fails.
        assert_eq!(
            s.clear_parent(SceneNodeId::from_raw(99))
                .unwrap_err()
                .code(),
            SceneErrorCode::MissingNode
        );
    }

    #[test]
    fn add_and_remove_camera() {
        let mut s = Scene::new();
        let n = node(&mut s);
        let cam = Camera::perspective(
            &math(),
            Radians::new(std::f32::consts::FRAC_PI_2).unwrap(),
            Ratio::new(1.0).unwrap(),
            Meters::new(0.1).unwrap(),
            Meters::new(100.0).unwrap(),
        )
        .unwrap();
        s.add_camera(n, cam).unwrap();
        assert_eq!(s.camera_count(), 1);
        // Missing node.
        assert_eq!(
            s.add_camera(SceneNodeId::from_raw(99), cam)
                .unwrap_err()
                .code(),
            SceneErrorCode::MissingNode
        );
        s.remove_camera(n).unwrap();
        assert_eq!(s.camera_count(), 0);
        // Removing absent camera fails.
        assert_eq!(
            s.remove_camera(n).unwrap_err().code(),
            SceneErrorCode::MissingCamera
        );
    }

    #[test]
    fn add_and_remove_light() {
        let mut s = Scene::new();
        let n = node(&mut s);
        let l = Light::directional(&math(), Vec3::ONE, Ratio::new(1.0).unwrap()).unwrap();
        s.add_light(n, l).unwrap();
        assert_eq!(s.light_count(), 1);
        assert_eq!(
            s.add_light(SceneNodeId::from_raw(99), l)
                .unwrap_err()
                .code(),
            SceneErrorCode::MissingNode
        );
        s.remove_light(n).unwrap();
        assert_eq!(
            s.remove_light(n).unwrap_err().code(),
            SceneErrorCode::MissingLight
        );
    }

    #[test]
    fn add_remove_and_toggle_renderable() {
        use crate::material_ref::MaterialRef;
        use crate::mesh_ref::MeshRef;
        let mut s = Scene::new();
        let n = node(&mut s);
        let r = Renderable::new(MeshRef::from_raw(1), MaterialRef::from_raw(2)).unwrap();
        s.add_renderable(n, r).unwrap();
        assert_eq!(s.renderable_count(), 1);
        assert_eq!(
            s.add_renderable(SceneNodeId::from_raw(99), r)
                .unwrap_err()
                .code(),
            SceneErrorCode::MissingNode
        );
        // Toggle visibility present + missing.
        s.set_renderable_visible(n, false).unwrap();
        assert_eq!(
            s.set_renderable_visible(SceneNodeId::from_raw(99), true)
                .unwrap_err()
                .code(),
            SceneErrorCode::MissingRenderable
        );
        s.remove_renderable(n).unwrap();
        assert_eq!(
            s.remove_renderable(n).unwrap_err().code(),
            SceneErrorCode::MissingRenderable
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
}

#[cfg(test)]
mod frame_tests {
    use super::*;
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
        // The player node's local — and its propagated world — moved by the delta.
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
        // A frame carrying a forward move (local -Z by 1) for controller 0, no turn.
        let frame = encode_controller(0, 0, Vec3::new(0.0, 0.0, -1.0), 0.0);
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
}
