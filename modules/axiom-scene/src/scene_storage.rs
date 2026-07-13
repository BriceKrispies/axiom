//! The scene's ECS component storage and the transform-hierarchy system.
//!
//! This is where `axiom-scene` becomes a *semantic adapter over the ECS layer*:
//! the scene's standard component columns live in [`SceneStorage`] (the `S` a
//! generic [`axiom_ecs::World`] holds), and [`TransformPropagation`] is the one
//! [`axiom_ecs::WorldSystem`] that turns local transforms + parent links into
//! world transforms — the engine embodiment of "a transform hierarchy is just a
//! system over the world."

use std::collections::BTreeMap;

use axiom_ecs::{
    ColumnSet, ComponentColumn, DynamicComponents, EntityRegistry, ErasedColumn, WorldStep,
    WorldSystem,
};
use axiom_kernel::{
    BinaryReader, BinaryWriter, EntityId, FieldSchema, KernelResult, Meters, Reflect, TypeSchema,
};
use axiom_math::{Quat, Transform, Vec3};

use crate::bounds::Bounds;
use crate::camera::Camera;
use crate::light::Light;
use crate::procanim::ProcAnim;
use crate::renderable::Renderable;
use crate::sdf_shape::SdfShape;
use crate::spin::Spin;
use crate::tag::Tag;

/// The scene's component storage: one sparse column per standard component
/// type. This is the `S` the generic [`axiom_ecs::World<S>`] holds.
///
/// An entity (node) appears in `locals` iff it is a real node; `worlds` is the
/// system-computed output; `parents` records the child→parent link; `cameras` /
/// `lights` / `renderables` carry at most one of each per node.
#[derive(Debug, Default)]
pub struct SceneStorage {
    pub locals: ComponentColumn<Transform>,
    pub worlds: ComponentColumn<Transform>,
    pub parents: ComponentColumn<EntityId>,
    pub cameras: ComponentColumn<Camera>,
    pub lights: ComponentColumn<Light>,
    pub renderables: ComponentColumn<Renderable>,
    /// Raymarched SDF shapes, keyed by node entity — the node renders as a
    /// signed-distance primitive (sphere / box / plane) the backends march.
    pub sdf_shapes: ComponentColumn<SdfShape>,
    pub spins: ComponentColumn<Spin>,
    pub procanims: ComponentColumn<ProcAnim>,
    /// Axis-aligned bounding volumes, keyed by node entity. The queryable
    /// spatial extent the [`crate::SceneApi`] raycast / overlap queries fold over.
    pub bounds: ComponentColumn<Bounds>,
    /// Coarse semantic kinds, keyed by node entity — what each thing *is*, the
    /// classification a perceiving agent reads off a raycast / overlap hit.
    pub tags: ComponentColumn<Tag>,
    /// App-defined components the engine was never told about at compile time,
    /// keyed by node entity and stored type-erased (by `Reflect` schema name).
    /// This is the scene's *open* component arm — the home for a retained world
    /// authored over the wasm boundary (`@axiom/game`'s `world.set(e, {kind,…})`),
    /// where the schema is a closed game vocabulary the engine need not name. The
    /// engine's typed columns above stay the zero-cost borrowed path; this serves
    /// the app-blind path. Cleared per-entity on despawn (see
    /// `Scene::despawn_entity`).
    pub dynamic: DynamicComponents,
    /// Controllable nodes, keyed entity → player index. Authored once; the
    /// bridge that lets a per-tick move command address a node by player index.
    pub players: BTreeMap<EntityId, u32>,
    /// The per-tick move deltas to apply this frame, `(player index, delta)`.
    /// Staged from frame commands by [`crate::scene::Scene::advance`] and drained
    /// by [`PlayerMoveSystem`]; transient, never serialized.
    pub pending_moves: Vec<(u32, Vec3)>,
    /// First-person controller nodes, keyed entity → [`ControllerState`].
    /// Authored once; the bridge that lets a per-tick controller input address a
    /// node by index, and the home of that controller's accumulated yaw/pitch.
    pub controllers: BTreeMap<EntityId, ControllerState>,
    /// The per-tick controller inputs to apply this frame, `(index, move_local,
    /// yaw_delta, pitch_delta, seat_y)`. `seat_y` is the optional absolute
    /// vertical seat (metres) that, when present, overrides `move_local`'s
    /// vertical delta. Staged from frame commands by
    /// [`crate::scene::Scene::advance`] and drained by [`ControllerSystem`];
    /// transient, never serialized.
    pub pending_controls: Vec<(u32, Vec3, f32, f32, Option<Meters>)>,
    /// Whether a local transform / parent link changed since world transforms were
    /// last propagated — the coalescing flag behind [`crate::scene::Scene::update_world_transforms`].
    /// Set by every transform/hierarchy mutation, cleared when a propagation runs, so
    /// moving N nodes then reading once costs ONE whole-scene propagation instead of N
    /// (the difference between O(N·nodes) and O(nodes) per frame). Transient, never serialized.
    pub world_dirty: bool,
}

/// A first-person controller node's persistent orientation: the index it answers
/// to, plus accumulated `yaw` (about world +Y) and `pitch` (about its local +X).
/// The [`ControllerSystem`] rebuilds the node rotation from these every tick, so
/// orientation never drifts and pitch can be clamped.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ControllerState {
    pub(crate) index: u32,
    pub(crate) yaw: f32,
    pub(crate) pitch: f32,
}

impl Reflect for ControllerState {
    const SCHEMA: TypeSchema = TypeSchema::new(
        "ControllerState",
        &[
            FieldSchema::new("index", "u32"),
            FieldSchema::new("yaw", "f32"),
            FieldSchema::new("pitch", "f32"),
        ],
    );

    fn reflect_write(&self, writer: &mut BinaryWriter) {
        self.index.reflect_write(writer);
        self.yaw.reflect_write(writer);
        self.pitch.reflect_write(writer);
    }

    fn reflect_read(reader: &mut BinaryReader<'_>) -> KernelResult<Self> {
        u32::reflect_read(reader).and_then(|index| {
            f32::reflect_read(reader).and_then(|yaw| {
                f32::reflect_read(reader).map(|pitch| ControllerState { index, yaw, pitch })
            })
        })
    }
}

/// Exposes the scene's component columns as the type-erased set the generic
/// [`axiom_ecs::World`] serializes. The two non-column maps (`players`,
/// `controllers`) carry persistent state too, but they are not columns — the
/// scene serializes them alongside this set (see `Scene::write_state`). The
/// transient `pending_*` queues are never serialized.
impl ColumnSet for SceneStorage {
    fn columns(&self) -> Vec<(&'static str, &dyn ErasedColumn)> {
        vec![
            ("locals", &self.locals),
            ("worlds", &self.worlds),
            ("parents", &self.parents),
            ("cameras", &self.cameras),
            ("lights", &self.lights),
            ("renderables", &self.renderables),
            ("sdf_shapes", &self.sdf_shapes),
            ("spins", &self.spins),
            ("procanims", &self.procanims),
            ("bounds", &self.bounds),
            ("tags", &self.tags),
        ]
    }

    fn columns_mut(&mut self) -> Vec<(&'static str, &mut dyn ErasedColumn)> {
        vec![
            ("locals", &mut self.locals),
            ("worlds", &mut self.worlds),
            ("parents", &mut self.parents),
            ("cameras", &mut self.cameras),
            ("lights", &mut self.lights),
            ("renderables", &mut self.renderables),
            ("sdf_shapes", &mut self.sdf_shapes),
            ("spins", &mut self.spins),
            ("procanims", &mut self.procanims),
            ("bounds", &mut self.bounds),
            ("tags", &mut self.tags),
        ]
    }
}

/// How far the camera may pitch up or down (radians, ~86°), short of straight
/// up/down so the look basis never degenerates.
const PITCH_LIMIT: f32 = 1.5;

/// The transform-hierarchy system: computes each entity's world transform from
/// its local transform and its parent's world transform
/// (`world = parent_world ∘ local`).
///
/// Entities are processed in ascending entity-id order; a parent's world is
/// computed before any child reads it as long as parents are spawned before
/// their children (the scene mints ids monotonically, and `set_parent` rejects
/// cycles). Deterministic: ordered `iter` over a `BTreeMap`-backed registry and
/// columns.
#[derive(Debug)]
pub struct TransformPropagation;

impl WorldSystem<SceneStorage> for TransformPropagation {
    fn run(&self, _step: &WorldStep, entities: &EntityRegistry, storage: &mut SceneStorage) {
        propagate(entities.iter(), storage);
        // Worlds are now fresh for this tick, so clear the coalescing flag — a subsequent
        // on-demand `update_world_transforms` (present / query) is a no-op until the next
        // authored move re-arms it, avoiding a redundant whole-scene re-propagation.
        storage.world_dirty = false;
    }
}

/// The spin system: drives each entity with a [`Spin`] component to a pure
/// rotation about its axis, derived from the frame tick. Runs before
/// [`TransformPropagation`] so the updated local transforms propagate this
/// frame. Entities whose spin axis cannot form a rotation are left untouched.
#[derive(Debug)]
pub struct SpinSystem;

impl WorldSystem<SceneStorage> for SpinSystem {
    fn run(&self, step: &WorldStep, _entities: &EntityRegistry, storage: &mut SceneStorage) {
        let updates: Vec<(EntityId, Transform)> = storage
            .spins
            .iter()
            .filter_map(|(entity, spin)| {
                spin.rotation_at(step.tick())
                    .map(|q| (entity, Transform::from_rotation(q)))
            })
            .collect();
        updates.into_iter().for_each(|(entity, local)| {
            storage.locals.insert(entity, local);
        });
    }
}

/// The procedural-animation system: drives each entity with a [`ProcAnim`]
/// component to its animated local transform (resting pose + bob + spin) for the
/// frame tick. Runs alongside [`SpinSystem`], before [`TransformPropagation`], so
/// the animated locals propagate this frame. Generalizes the spin system to
/// *positioned* nodes: it composes the animation around each node's resting pose
/// (so a wall at a grid cell keeps its place) instead of overwriting the local
/// with a pure rotation. Every [`ProcAnim`] yields a transform, so none is
/// skipped.
#[derive(Debug)]
pub struct ProcAnimSystem;

impl WorldSystem<SceneStorage> for ProcAnimSystem {
    fn run(&self, step: &WorldStep, _entities: &EntityRegistry, storage: &mut SceneStorage) {
        let updates: Vec<(EntityId, Transform)> = storage
            .procanims
            .iter()
            .map(|(entity, anim)| (entity, anim.local_at(step.tick())))
            .collect();
        updates.into_iter().for_each(|(entity, local)| {
            storage.locals.insert(entity, local);
        });
    }
}

/// The player-move system: applies this frame's staged move deltas to the local
/// translation of each addressed player node, then clears the queue. Runs after
/// [`SpinSystem`] and before [`TransformPropagation`] so the moved locals
/// propagate this frame. A player node carries no [`Spin`], so nothing else
/// writes its local — its translation accumulates across ticks.
#[derive(Debug)]
pub struct PlayerMoveSystem;

impl WorldSystem<SceneStorage> for PlayerMoveSystem {
    fn run(&self, _step: &WorldStep, _entities: &EntityRegistry, storage: &mut SceneStorage) {
        let moves = std::mem::take(&mut storage.pending_moves);
        moves.into_iter().for_each(|(player, delta)| {
            storage
                .players
                .iter()
                .find_map(|(&e, &i)| (i == player).then_some(e))
                .into_iter()
                .for_each(|entity| {
                    let mut local = storage
                        .locals
                        .get(entity)
                        .copied()
                        .unwrap_or(Transform::IDENTITY);
                    local.translation = Vec3::new(
                        local.translation.x + delta.x,
                        local.translation.y + delta.y,
                        local.translation.z + delta.z,
                    );
                    storage.locals.insert(entity, local);
                });
        });
    }
}

/// The first-person controller system: applies this frame's staged controller
/// inputs to each addressed controller node. For each input it accumulates
/// `yaw_delta` (about world +Y) and `pitch_delta` (about local +X, clamped to
/// [`PITCH_LIMIT`]) into the node's [`ControllerState`], then rebuilds the node's
/// rotation as `Ry(yaw)·Rx(pitch)` (the view) and translates it by `move_local`
/// rotated by the **yaw only** — so looking up or down never tilts movement off
/// the horizontal plane. Runs after [`SpinSystem`] and before
/// [`TransformPropagation`]. Rebuilding from stored angles (rather than
/// accumulating into the quaternion) keeps orientation drift-free and lets pitch
/// clamp.
#[derive(Debug)]
pub struct ControllerSystem;

impl WorldSystem<SceneStorage> for ControllerSystem {
    fn run(&self, _step: &WorldStep, _entities: &EntityRegistry, storage: &mut SceneStorage) {
        let controls = std::mem::take(&mut storage.pending_controls);
        controls
            .into_iter()
            .for_each(|(index, move_local, yaw_delta, pitch_delta, seat_y)| {
                apply_controller(storage, index, move_local, yaw_delta, pitch_delta, seat_y);
            });
    }
}

/// Apply one first-person input to controller `index`'s node: accumulate `yaw`
/// (about world +Y) and `pitch` (about local +X, clamped to [`PITCH_LIMIT`]) into
/// its [`ControllerState`], rebuild the node rotation as `yaw·pitch`, and move it
/// by `move_local` rotated by the **yaw only** (so looking up/down never tilts
/// movement). The vertical result is selected by `seat_y`: `None` applies the
/// rotated step's vertical delta (a free-fly controller); `Some(m)` seats the
/// local eye at exactly `m` metres (the deterministic ground-follow path). The
/// horizontal (x, z) step is applied identically either way. Resolving the index
/// is deterministic (BTreeMap iter); an input for an unknown index resolves to
/// `None` and is ignored. Shared by [`ControllerSystem`] (the per-tick drain) and
/// the on-demand [`crate::scene::Scene::control_now`] (the immediate, zero-lag
/// path) so the two never diverge.
pub(crate) fn apply_controller(
    storage: &mut SceneStorage,
    index: u32,
    move_local: Vec3,
    yaw_delta: f32,
    pitch_delta: f32,
    seat_y: Option<Meters>,
) {
    storage
        .controllers
        .iter()
        .find_map(|(&e, s)| (s.index == index).then_some(e))
        .into_iter()
        .for_each(|entity| {
            let state = storage
                .controllers
                .get_mut(&entity)
                .expect("entity was just resolved from this map");
            state.yaw += yaw_delta;
            state.pitch = (state.pitch + pitch_delta).clamp(-PITCH_LIMIT, PITCH_LIMIT);
            // Unit quaternions built directly via `(axis*sin(theta/2), cos(theta/2))`.
            let (yh, ph) = (state.yaw * 0.5, state.pitch * 0.5);
            let yaw = Quat::new(0.0, yh.sin(), 0.0, yh.cos());
            let pitch = Quat::new(ph.sin(), 0.0, 0.0, ph.cos());

            let mut local = storage
                .locals
                .get(entity)
                .copied()
                .unwrap_or(Transform::IDENTITY);
            local.rotation = yaw.multiply(pitch);
            let step = yaw.rotate(move_local);
            // Vertical select (branchless): `None` keeps the delta path
            // (`current + step.y`); `Some(m)` seats the eye at exactly `m`.
            let delta_y = local.translation.y + step.y;
            let seated_y = seat_y.map_or(delta_y, Meters::get);
            local.translation = Vec3::new(
                local.translation.x + step.x,
                seated_y,
                local.translation.z + step.z,
            );
            storage.locals.insert(entity, local);
        });
}

/// Compute world transforms for `ids` (in the given order) from `locals` +
/// `parents`, writing them into `storage.worlds`. The single implementation
/// shared by [`TransformPropagation`] (per-frame, via the ECS world) and
/// [`crate::scene::Scene::update_world_transforms`] (on demand).
pub(crate) fn propagate(ids: impl Iterator<Item = EntityId>, storage: &mut SceneStorage) {
    let mut worlds: BTreeMap<EntityId, Transform> = BTreeMap::new();
    ids.for_each(|id| {
        storage
            .locals
            .get(id)
            .copied()
            .into_iter()
            .for_each(|local| {
                let world = storage
                    .parents
                    .get(id)
                    .and_then(|p| worlds.get(p).copied())
                    .map_or(local, |parent_world| {
                        Transform::combine(parent_world, local)
                    });
                worlds.insert(id, world);
            });
    });
    worlds.into_iter().for_each(|(id, world)| {
        storage.worlds.insert(id, world);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_math::Vec3;

    fn e(raw: u64) -> EntityId {
        EntityId::from_raw(raw)
    }

    fn registry(n: u64) -> EntityRegistry {
        let mut reg = EntityRegistry::new();
        for _ in 0..n {
            reg.spawn();
        }
        reg
    }

    #[test]
    fn default_storage_is_empty() {
        let s = SceneStorage::default();
        assert!(s.locals.is_empty());
        assert!(s.worlds.is_empty());
        assert!(s.parents.is_empty());
        assert!(s.cameras.is_empty());
        assert!(s.lights.is_empty());
        assert!(s.renderables.is_empty());
        assert!(s.sdf_shapes.is_empty());
        assert!(s.spins.is_empty());
        assert!(s.procanims.is_empty());
        assert!(s.bounds.is_empty());
        assert!(s.tags.is_empty());
        assert!(s.players.is_empty());
        assert!(s.pending_moves.is_empty());
        assert!(s.controllers.is_empty());
        assert!(s.pending_controls.is_empty());
    }

    fn ctrl(i: u32) -> ControllerState {
        ControllerState {
            index: i,
            yaw: 0.0,
            pitch: 0.0,
        }
    }

    #[test]
    fn controller_system_yaws_then_moves_relative_to_facing_and_drains() {
        let reg = registry(1);
        let mut storage = SceneStorage::default();
        storage.locals.insert(e(1), Transform::IDENTITY);
        storage.controllers.insert(e(1), ctrl(0));
        let quarter = std::f32::consts::FRAC_PI_2;
        storage
            .pending_controls
            .push((0, Vec3::new(0.0, 0.0, -1.0), quarter, 0.0, None));

        ControllerSystem.run(&WorldStep::new(0), &reg, &mut storage);

        let local = storage.locals.get(e(1)).unwrap();
        assert!(local.rotation.w.abs() < 0.999, "the node yawed");
        assert!(
            local.translation.x < -0.9,
            "forward followed the new facing"
        );
        assert!(local.translation.z.abs() < 1.0e-5);
        assert_eq!(storage.controllers.get(&e(1)).unwrap().yaw, quarter);
        assert!(storage.pending_controls.is_empty());
    }

    #[test]
    fn controller_system_strafes_along_local_right() {
        let reg = registry(1);
        let mut storage = SceneStorage::default();
        storage.locals.insert(e(1), Transform::IDENTITY);
        storage.controllers.insert(e(1), ctrl(0));
        storage
            .pending_controls
            .push((0, Vec3::new(1.0, 0.0, 0.0), 0.0, 0.0, None));
        ControllerSystem.run(&WorldStep::new(0), &reg, &mut storage);
        let local = storage.locals.get(e(1)).unwrap();
        assert!((local.translation.x - 1.0).abs() < 1.0e-5);
        assert!(local.translation.z.abs() < 1.0e-5);
    }

    #[test]
    fn controller_system_accumulates_forward_across_ticks() {
        let reg = registry(1);
        let mut storage = SceneStorage::default();
        storage.locals.insert(e(1), Transform::IDENTITY);
        storage.controllers.insert(e(1), ctrl(0));
        for _ in 0..3 {
            storage
                .pending_controls
                .push((0, Vec3::new(0.0, 0.0, -0.5), 0.0, 0.0, None));
            ControllerSystem.run(&WorldStep::new(0), &reg, &mut storage);
        }
        assert!((storage.locals.get(e(1)).unwrap().translation.z + 1.5).abs() < 1.0e-5);
    }

    #[test]
    fn controller_pitch_tilts_the_view_and_clamps_both_ways() {
        let reg = registry(1);
        let mut storage = SceneStorage::default();
        storage.locals.insert(e(1), Transform::IDENTITY);
        storage.controllers.insert(e(1), ctrl(0));
        storage.pending_controls.push((0, Vec3::ZERO, 0.0, 10.0, None));
        ControllerSystem.run(&WorldStep::new(0), &reg, &mut storage);
        assert_eq!(storage.controllers.get(&e(1)).unwrap().pitch, PITCH_LIMIT);
        assert!(
            storage.locals.get(e(1)).unwrap().rotation.x.abs() > 0.1,
            "pitched"
        );
        storage.pending_controls.push((0, Vec3::ZERO, 0.0, -20.0, None));
        ControllerSystem.run(&WorldStep::new(0), &reg, &mut storage);
        assert_eq!(storage.controllers.get(&e(1)).unwrap().pitch, -PITCH_LIMIT);
    }

    #[test]
    fn controller_movement_stays_horizontal_under_pitch() {
        let reg = registry(1);
        let mut storage = SceneStorage::default();
        storage.locals.insert(e(1), Transform::IDENTITY);
        storage.controllers.insert(e(1), ctrl(0));
        storage
            .pending_controls
            .push((0, Vec3::new(0.0, 0.0, -1.0), 0.0, 1.2, None));
        ControllerSystem.run(&WorldStep::new(0), &reg, &mut storage);
        let local = storage.locals.get(e(1)).unwrap();
        assert!(
            local.translation.y.abs() < 1.0e-6,
            "forward stayed horizontal"
        );
        assert!(
            (local.translation.z + 1.0).abs() < 1.0e-5,
            "moved forward on -Z"
        );
    }

    #[test]
    fn controller_without_a_seat_applies_move_local_vertical_as_a_delta() {
        // `seat_y = None`: the vertical component of `move_local` accumulates as a
        // free-fly delta (retro FPS's jump path) — the `map_or` None arm.
        let reg = registry(1);
        let mut storage = SceneStorage::default();
        storage.locals.insert(e(1), Transform::IDENTITY);
        storage.controllers.insert(e(1), ctrl(0));
        storage
            .pending_controls
            .push((0, Vec3::new(0.0, 2.0, 0.0), 0.0, 0.0, None));
        ControllerSystem.run(&WorldStep::new(0), &reg, &mut storage);
        assert!(
            (storage.locals.get(e(1)).unwrap().translation.y - 2.0).abs() < 1.0e-6,
            "vertical move_local accumulated as a delta"
        );
        // A second identical delta keeps accumulating (proving it is not seated).
        storage
            .pending_controls
            .push((0, Vec3::new(0.0, 2.0, 0.0), 0.0, 0.0, None));
        ControllerSystem.run(&WorldStep::new(0), &reg, &mut storage);
        assert!((storage.locals.get(e(1)).unwrap().translation.y - 4.0).abs() < 1.0e-6);
    }

    #[test]
    fn controller_with_a_seat_places_the_eye_at_the_absolute_height() {
        // `seat_y = Some(m)`: the local eye is seated at exactly `m`, ignoring the
        // vertical component of `move_local` — the `map_or` Some arm. The
        // horizontal step is still applied.
        let reg = registry(1);
        let mut storage = SceneStorage::default();
        storage
            .locals
            .insert(e(1), Transform::from_translation(Vec3::new(0.0, 99.0, 0.0)));
        storage.controllers.insert(e(1), ctrl(0));
        let seat = Meters::new(5.0).expect("seat is finite");
        storage
            .pending_controls
            .push((0, Vec3::new(1.0, 42.0, 0.0), 0.0, 0.0, Some(seat)));
        ControllerSystem.run(&WorldStep::new(0), &reg, &mut storage);
        let local = storage.locals.get(e(1)).unwrap();
        assert!(
            (local.translation.y - 5.0).abs() < 1.0e-6,
            "eye seated at the absolute height, ignoring the prior y and move_local.y"
        );
        assert!(
            (local.translation.x - 1.0).abs() < 1.0e-5,
            "horizontal step still applied under a seat"
        );
        // Re-seating at the same height is idempotent (absolute, not a delta).
        storage
            .pending_controls
            .push((0, Vec3::ZERO, 0.0, 0.0, Some(seat)));
        ControllerSystem.run(&WorldStep::new(0), &reg, &mut storage);
        assert!((storage.locals.get(e(1)).unwrap().translation.y - 5.0).abs() < 1.0e-6);
    }

    #[test]
    fn controller_input_for_unknown_index_is_ignored() {
        let reg = registry(1);
        let mut storage = SceneStorage::default();
        storage.locals.insert(e(1), Transform::IDENTITY);
        storage.controllers.insert(e(1), ctrl(0));
        storage
            .pending_controls
            .push((7, Vec3::new(9.0, 0.0, 9.0), 9.0, 9.0, None));
        ControllerSystem.run(&WorldStep::new(0), &reg, &mut storage);
        assert_eq!(storage.locals.get(e(1)).unwrap().translation.z, 0.0);
    }

    #[test]
    fn controller_uses_identity_when_node_has_no_local() {
        let reg = registry(1);
        let mut storage = SceneStorage::default();
        storage.controllers.insert(e(1), ctrl(0));
        storage
            .pending_controls
            .push((0, Vec3::new(1.0, 0.0, 0.0), 0.0, 0.0, None));
        ControllerSystem.run(&WorldStep::new(0), &reg, &mut storage);
        assert!((storage.locals.get(e(1)).unwrap().translation.x - 1.0).abs() < 1.0e-5);
    }

    #[test]
    fn controller_system_debug_is_renderable() {
        assert!(format!("{:?}", ControllerSystem).contains("ControllerSystem"));
    }

    #[test]
    fn player_move_system_translates_the_addressed_player_and_drains() {
        let reg = registry(2);
        let mut storage = SceneStorage::default();
        storage.locals.insert(e(1), Transform::IDENTITY);
        storage.players.insert(e(1), 0);
        storage
            .locals
            .insert(e(2), Transform::from_translation(Vec3::new(5.0, 0.0, 0.0)));
        storage.players.insert(e(2), 1);
        storage.pending_moves.push((0, Vec3::new(1.0, 2.0, 0.0)));

        PlayerMoveSystem.run(&WorldStep::new(0), &reg, &mut storage);

        assert_eq!(storage.locals.get(e(1)).unwrap().translation.x, 1.0);
        assert_eq!(storage.locals.get(e(1)).unwrap().translation.y, 2.0);
        assert_eq!(storage.locals.get(e(2)).unwrap().translation.x, 5.0);
        assert!(storage.pending_moves.is_empty());
    }

    #[test]
    fn player_move_system_accumulates_across_ticks() {
        let reg = registry(1);
        let mut storage = SceneStorage::default();
        storage.locals.insert(e(1), Transform::IDENTITY);
        storage.players.insert(e(1), 0);
        for _ in 0..3 {
            storage.pending_moves.push((0, Vec3::new(0.5, 0.0, 0.0)));
            PlayerMoveSystem.run(&WorldStep::new(0), &reg, &mut storage);
        }
        assert_eq!(storage.locals.get(e(1)).unwrap().translation.x, 1.5);
    }

    #[test]
    fn player_move_for_unknown_index_is_ignored() {
        let reg = registry(1);
        let mut storage = SceneStorage::default();
        storage.locals.insert(e(1), Transform::IDENTITY);
        storage.players.insert(e(1), 0);
        storage.pending_moves.push((7, Vec3::new(9.0, 9.0, 9.0)));
        PlayerMoveSystem.run(&WorldStep::new(0), &reg, &mut storage);
        assert_eq!(storage.locals.get(e(1)).unwrap().translation.x, 0.0);
    }

    #[test]
    fn player_move_system_debug_is_renderable() {
        assert!(format!("{:?}", PlayerMoveSystem).contains("PlayerMoveSystem"));
    }

    #[test]
    fn spin_system_rotates_spun_nodes_and_skips_invalid_axes() {
        let reg = registry(2);
        let mut storage = SceneStorage::default();
        storage.locals.insert(e(1), Transform::IDENTITY);
        storage.spins.insert(e(1), Spin::new(Vec3::UNIT_Y, 360));
        storage
            .locals
            .insert(e(2), Transform::from_translation(Vec3::new(9.0, 0.0, 0.0)));
        storage
            .spins
            .insert(e(2), Spin::new(Vec3::new(0.0, 0.0, 0.0), 360));

        SpinSystem.run(&WorldStep::new(90), &reg, &mut storage);

        assert!((storage.locals.get(e(1)).unwrap().rotation.w - 1.0).abs() > 1.0e-6);
        assert_eq!(storage.locals.get(e(2)).unwrap().translation.x, 9.0);
    }

    #[test]
    fn spin_system_debug_is_renderable() {
        assert!(format!("{:?}", SpinSystem).contains("SpinSystem"));
    }

    #[test]
    fn procanim_system_animates_a_positioned_node_around_its_resting_pose() {
        let reg = registry(1);
        let mut storage = SceneStorage::default();
        let base = Transform::from_translation(Vec3::new(1.0, 2.0, 3.0));
        storage.locals.insert(e(1), base);
        storage
            .procanims
            .insert(e(1), ProcAnim::new(base, 0.5, 4, Vec3::UNIT_Y, 8, 0));

        ProcAnimSystem.run(&WorldStep::new(1), &reg, &mut storage);

        let local = storage.locals.get(e(1)).unwrap();
        assert!((local.translation.y - 2.5).abs() < 1.0e-5);
        assert_eq!(local.translation.x, 1.0);
        assert_eq!(local.translation.z, 3.0);
    }

    #[test]
    fn procanim_system_debug_is_renderable() {
        assert!(format!("{:?}", ProcAnimSystem).contains("ProcAnimSystem"));
    }

    #[test]
    fn propagation_covers_root_child_and_localless_and_uncomputed_parent() {
        let reg = registry(4);
        let mut storage = SceneStorage::default();
        storage
            .locals
            .insert(e(1), Transform::from_translation(Vec3::new(1.0, 0.0, 0.0)));
        storage
            .locals
            .insert(e(2), Transform::from_translation(Vec3::new(0.0, 2.0, 0.0)));
        storage.parents.insert(e(2), e(1));
        storage
            .locals
            .insert(e(4), Transform::from_translation(Vec3::new(5.0, 0.0, 0.0)));
        storage.parents.insert(e(4), e(3));

        TransformPropagation.run(&WorldStep::new(0), &reg, &mut storage);

        assert_eq!(storage.worlds.get(e(1)).unwrap().translation.x, 1.0);
        let w2 = storage.worlds.get(e(2)).unwrap();
        assert_eq!(w2.translation.x, 1.0);
        assert_eq!(w2.translation.y, 2.0);
        assert!(storage.worlds.get(e(3)).is_none());
        assert_eq!(storage.worlds.get(e(4)).unwrap().translation.x, 5.0);
    }

    #[test]
    fn propagation_is_deterministic_across_runs() {
        let run = || {
            let reg = registry(2);
            let mut storage = SceneStorage::default();
            storage
                .locals
                .insert(e(1), Transform::from_translation(Vec3::new(1.0, 0.0, 0.0)));
            storage
                .locals
                .insert(e(2), Transform::from_translation(Vec3::new(0.0, 1.0, 0.0)));
            storage.parents.insert(e(2), e(1));
            TransformPropagation.run(&WorldStep::new(0), &reg, &mut storage);
            let w = storage.worlds.get(e(2)).unwrap();
            (w.translation.x, w.translation.y)
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn transform_propagation_debug_is_renderable() {
        assert!(format!("{:?}", TransformPropagation).contains("TransformPropagation"));
    }

    /// End-to-end proof that the ECS component-command seam drives a real module's
    /// storage: `ComponentCommandBuffer<SceneStorage>` stages typed inserts/removes
    /// against `SceneStorage`'s own columns via field selectors, applied at a
    /// barrier — no `TypeId`/`unsafe`/downcast.
    #[test]
    fn component_command_buffer_drives_scene_storage_columns() {
        use crate::bounds::Bounds;
        use axiom_ecs::{ComponentCommandBuffer, World};
        use axiom_math::Transform;

        let mut world: World<SceneStorage> = World::new();
        let entity = world.spawn_handle().id();

        let mut buffer = ComponentCommandBuffer::new();
        let local = buffer.insert_component(
            entity,
            Transform::from_translation(Vec3::new(1.0, 2.0, 3.0)),
            |s: &mut SceneStorage| &mut s.locals,
        );
        let bound = buffer.insert_component(
            entity,
            Bounds::new(Vec3::new(0.5, 0.5, 0.5)),
            |s: &mut SceneStorage| &mut s.bounds,
        );
        assert!(world.storage().locals.get(entity).is_none());

        let report = buffer.apply(&mut world);
        assert_eq!(report.outcome(local).unwrap().inserted(), Some(false));
        assert_eq!(report.outcome(bound).unwrap().inserted(), Some(false));
        assert!(world.storage().locals.get(entity).is_some());
        assert!(world.storage().bounds.get(entity).is_some());

        let mut remover = ComponentCommandBuffer::new();
        let removed = remover.remove_component(entity, |s: &mut SceneStorage| &mut s.bounds);
        let report = remover.apply(&mut world);
        assert_eq!(report.outcome(removed).unwrap().removed(), Some(true));
        assert!(world.storage().bounds.get(entity).is_none());
    }
}
