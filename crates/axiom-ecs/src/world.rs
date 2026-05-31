//! The world: live entities + component storage + the systems that advance it.

use axiom_frame::FrameContext;
use axiom_kernel::EntityId;

use crate::entity_registry::EntityRegistry;
use crate::world_system::WorldSystem;

/// The single deterministic world model: an [`EntityRegistry`] of live
/// entities, a consumer-defined component storage `S` (a struct of
/// [`crate::ComponentColumn`]s), and an ordered list of [`WorldSystem`]s.
///
/// `S` is generic so each consumer (and the modules it composes) defines its
/// own component set; the world knows nothing about what components exist.
/// `advance` consumes a [`FrameContext`] and runs the registered systems only
/// when the frame is active — the same gating `axiom-scene::advance` uses, and
/// this layer's adapter over the frame layer.
pub struct World<S> {
    entities: EntityRegistry,
    storage: S,
    systems: Vec<Box<dyn WorldSystem<S>>>,
}

impl<S> std::fmt::Debug for World<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("World")
            .field("entities", &self.entities.len())
            .field("systems", &self.systems.len())
            .finish()
    }
}

impl<S: Default> World<S> {
    /// Create an empty world with default storage and no systems.
    pub fn new() -> Self {
        World {
            entities: EntityRegistry::new(),
            storage: S::default(),
            systems: Vec::new(),
        }
    }
}

impl<S> World<S> {
    /// Register a system; systems run in registration order on `advance`.
    pub fn register_system(&mut self, system: Box<dyn WorldSystem<S>>) {
        self.systems.push(system);
    }

    /// The number of registered systems.
    pub fn system_count(&self) -> usize {
        self.systems.len()
    }

    /// Mint and register a new entity. Components are inserted into the
    /// storage's columns under the returned id.
    pub fn spawn(&mut self) -> EntityId {
        self.entities.spawn()
    }

    /// Remove an entity from the live set (column cleanup is the consumer's).
    pub fn despawn(&mut self, entity: EntityId) -> bool {
        self.entities.despawn(entity)
    }

    /// The live entity registry.
    pub fn entities(&self) -> &EntityRegistry {
        &self.entities
    }

    /// Borrow the component storage.
    pub fn storage(&self) -> &S {
        &self.storage
    }

    /// Mutably borrow the component storage (to insert/remove components).
    pub fn storage_mut(&mut self) -> &mut S {
        &mut self.storage
    }

    /// The number of live entities.
    pub fn entity_count(&self) -> usize {
        self.entities.len()
    }

    /// Whether the world has no live entities.
    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    /// Advance the world for one engine frame: run every registered system, in
    /// order, but only when the frame is active (not skipped and it executed at
    /// least one runtime step). Mirrors `axiom-scene::advance`.
    pub fn advance(&mut self, frame: &FrameContext<'_>) {
        if frame.is_skipped() || frame.runtime_step_count() == 0 {
            return;
        }
        for system in &self.systems {
            system.run(&self.entities, &mut self.storage);
        }
    }
}

impl<S: Default> Default for World<S> {
    fn default() -> Self {
        World::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::component_column::ComponentColumn;
    use crate::fixtures;

    /// A tiny two-column storage: a source value and a doubled mirror.
    #[derive(Default)]
    struct Storage {
        value: ComponentColumn<i32>,
        doubled: ComponentColumn<i32>,
    }

    /// A system that writes `doubled = value * 2` for every entity with a value.
    struct DoubleValues;
    impl WorldSystem<Storage> for DoubleValues {
        fn run(&self, entities: &EntityRegistry, storage: &mut Storage) {
            let pairs: Vec<(EntityId, i32)> = entities
                .iter()
                .filter_map(|e| storage.value.get(e).map(|v| (e, *v)))
                .collect();
            for (e, v) in pairs {
                storage.doubled.insert(e, v * 2);
            }
        }
    }

    fn world_with_one_value(v: i32) -> (World<Storage>, EntityId) {
        let mut world: World<Storage> = World::new();
        world.register_system(Box::new(DoubleValues));
        let e = world.spawn();
        world.storage_mut().value.insert(e, v);
        (world, e)
    }

    #[test]
    fn new_and_default_worlds_are_empty() {
        let a: World<Storage> = World::new();
        let b: World<Storage> = World::default();
        assert!(a.is_empty());
        assert_eq!(a.entity_count(), 0);
        assert_eq!(a.system_count(), 0);
        assert!(b.is_empty());
    }

    #[test]
    fn spawn_despawn_and_accessors() {
        let mut world: World<Storage> = World::new();
        let e = world.spawn();
        assert_eq!(world.entity_count(), 1);
        assert!(!world.is_empty());
        assert!(world.entities().contains(e));
        world.storage_mut().value.insert(e, 3);
        assert_eq!(world.storage().value.get(e), Some(&3));
        assert!(world.despawn(e));
        assert!(!world.entities().contains(e));
    }

    #[test]
    fn advance_runs_systems_when_frame_active() {
        let (mut world, e) = world_with_one_value(5);
        assert_eq!(world.system_count(), 1);
        let frame = fixtures::active_engine_frame();
        world.advance(&FrameContext::new(&frame));
        assert_eq!(world.storage().doubled.get(e), Some(&10));
    }

    #[test]
    fn advance_skips_systems_for_a_skipped_frame() {
        let (mut world, e) = world_with_one_value(5);
        let frame = fixtures::skipped_engine_frame();
        world.advance(&FrameContext::new(&frame));
        assert!(world.storage().doubled.get(e).is_none(), "skipped frame runs no systems");
    }

    #[test]
    fn advance_skips_systems_when_no_runtime_step_ran() {
        // Visible but zero-step: not "skipped", but still must not advance.
        let (mut world, e) = world_with_one_value(5);
        let frame = fixtures::active_zero_step_engine_frame();
        let ctx = FrameContext::new(&frame);
        assert!(!ctx.is_skipped());
        assert_eq!(ctx.runtime_step_count(), 0);
        world.advance(&ctx);
        assert!(world.storage().doubled.get(e).is_none(), "zero-step frame runs no systems");
    }

    #[test]
    fn debug_renders_counts() {
        let (world, _) = world_with_one_value(1);
        let s = format!("{world:?}");
        assert!(s.contains("World"));
        assert!(s.contains("entities"));
    }
}
