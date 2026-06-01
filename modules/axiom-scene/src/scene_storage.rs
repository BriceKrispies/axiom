//! The scene's ECS component storage and the transform-hierarchy system.
//!
//! This is where `axiom-scene` becomes a *semantic adapter over the ECS layer*:
//! the scene's standard component columns live in [`SceneStorage`] (the `S` a
//! generic [`axiom_ecs::World`] holds), and [`TransformPropagation`] is the one
//! [`axiom_ecs::WorldSystem`] that turns local transforms + parent links into
//! world transforms — the engine embodiment of "a transform hierarchy is just a
//! system over the world."

use std::collections::BTreeMap;

use axiom_ecs::{ComponentColumn, EntityRegistry, WorldStep, WorldSystem};
use axiom_kernel::EntityId;
use axiom_math::Transform;

use crate::camera::Camera;
use crate::light::Light;
use crate::renderable::Renderable;
use crate::spin::Spin;

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
    pub spins: ComponentColumn<Spin>,
}

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
        for (entity, local) in updates {
            storage.locals.insert(entity, local);
        }
    }
}

/// Compute world transforms for `ids` (in the given order) from `locals` +
/// `parents`, writing them into `storage.worlds`. The single implementation
/// shared by [`TransformPropagation`] (per-frame, via the ECS world) and
/// [`crate::scene::Scene::update_world_transforms`] (on demand).
pub(crate) fn propagate(ids: impl Iterator<Item = EntityId>, storage: &mut SceneStorage) {
    let mut worlds: BTreeMap<EntityId, Transform> = BTreeMap::new();
    for id in ids {
        if let Some(&local) = storage.locals.get(id) {
            let world = match storage.parents.get(id).and_then(|p| worlds.get(p).copied()) {
                Some(parent_world) => Transform::combine(parent_world, local),
                None => local,
            };
            worlds.insert(id, world);
        }
    }
    for (id, world) in worlds {
        storage.worlds.insert(id, world);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_math::Vec3;

    fn e(raw: u64) -> EntityId {
        EntityId::from_raw(raw)
    }

    /// Build a registry holding entities `1..=n`.
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
        assert!(s.spins.is_empty());
    }

    #[test]
    fn spin_system_rotates_spun_nodes_and_skips_invalid_axes() {
        let reg = registry(2);
        let mut storage = SceneStorage::default();
        // e1: a valid spin; its local starts at identity.
        storage.locals.insert(e(1), Transform::IDENTITY);
        storage.spins.insert(e(1), Spin::new(Vec3::UNIT_Y, 360));
        // e2: a degenerate (zero-axis) spin — the filter_map None arm; its local
        // must be left untouched.
        storage.locals.insert(e(2), Transform::from_translation(Vec3::new(9.0, 0.0, 0.0)));
        storage.spins.insert(e(2), Spin::new(Vec3::new(0.0, 0.0, 0.0), 360));

        SpinSystem.run(&WorldStep::new(90), &reg, &mut storage);

        // e1 became a non-identity rotation about Y (a quarter turn).
        assert!((storage.locals.get(e(1)).unwrap().rotation.w - 1.0).abs() > 1.0e-6);
        // e2 is unchanged (invalid axis -> skipped).
        assert_eq!(storage.locals.get(e(2)).unwrap().translation.x, 9.0);
    }

    #[test]
    fn spin_system_debug_is_renderable() {
        assert!(format!("{:?}", SpinSystem).contains("SpinSystem"));
    }

    #[test]
    fn propagation_covers_root_child_and_localless_and_uncomputed_parent() {
        // e1: root with a local       -> parent-link None arm.
        // e2: child of e1 with a local-> parent world present, Some arm.
        // e3: NO local                -> the `if let Some(local)` false arm.
        // e4: child of e3 with a local-> parent has no world, and_then None arm.
        let reg = registry(4);
        let mut storage = SceneStorage::default();
        storage.locals.insert(e(1), Transform::from_translation(Vec3::new(1.0, 0.0, 0.0)));
        storage.locals.insert(e(2), Transform::from_translation(Vec3::new(0.0, 2.0, 0.0)));
        storage.parents.insert(e(2), e(1));
        // e3 deliberately has no local.
        storage.locals.insert(e(4), Transform::from_translation(Vec3::new(5.0, 0.0, 0.0)));
        storage.parents.insert(e(4), e(3));

        TransformPropagation.run(&WorldStep::new(0), &reg, &mut storage);

        // Root world == its local.
        assert_eq!(storage.worlds.get(e(1)).unwrap().translation.x, 1.0);
        // Child accumulates parent + child translation.
        let w2 = storage.worlds.get(e(2)).unwrap();
        assert_eq!(w2.translation.x, 1.0);
        assert_eq!(w2.translation.y, 2.0);
        // No-local entity produced no world.
        assert!(storage.worlds.get(e(3)).is_none());
        // Child of a world-less parent falls back to its own local.
        assert_eq!(storage.worlds.get(e(4)).unwrap().translation.x, 5.0);
    }

    #[test]
    fn propagation_is_deterministic_across_runs() {
        let run = || {
            let reg = registry(2);
            let mut storage = SceneStorage::default();
            storage.locals.insert(e(1), Transform::from_translation(Vec3::new(1.0, 0.0, 0.0)));
            storage.locals.insert(e(2), Transform::from_translation(Vec3::new(0.0, 1.0, 0.0)));
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
}
