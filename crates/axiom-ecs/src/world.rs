//! The world: an entity store plus the systems that advance it each frame.

use axiom_frame::FrameContext;
use axiom_kernel::EntityId;

use crate::entity_store::EntityStore;
use crate::world_system::WorldSystem;

/// The single deterministic world model: a generic [`EntityStore`] of component
/// rows plus an ordered list of [`WorldSystem`]s that advance it once per
/// engine frame.
///
/// `advance` consumes a [`FrameContext`] and runs the registered systems only
/// when the frame is active — the same lifecycle/step gating
/// `axiom-scene::advance` uses — which is this layer's adapter over the frame
/// layer. Entity ops delegate to the store; iteration is ascending-id ordered.
pub struct World<R> {
    store: EntityStore<R>,
    systems: Vec<Box<dyn WorldSystem<R>>>,
}

impl<R> std::fmt::Debug for World<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("World")
            .field("entities", &self.store.len())
            .field("systems", &self.systems.len())
            .finish()
    }
}

impl<R> World<R> {
    /// Create an empty world with no systems.
    pub fn new() -> Self {
        World {
            store: EntityStore::new(),
            systems: Vec::new(),
        }
    }

    /// Register a system; systems run in registration order on `advance`.
    pub fn register_system(&mut self, system: Box<dyn WorldSystem<R>>) {
        self.systems.push(system);
    }

    /// The number of registered systems.
    pub fn system_count(&self) -> usize {
        self.systems.len()
    }

    /// Advance the world for one engine frame: run every registered system, in
    /// order, but only when the frame is active (not skipped and it executed at
    /// least one runtime step). Mirrors `axiom-scene::advance`.
    pub fn advance(&mut self, frame: &FrameContext<'_>) {
        if frame.is_skipped() || frame.runtime_step_count() == 0 {
            return;
        }
        for system in &self.systems {
            system.run(&mut self.store);
        }
    }

    // --- entity store delegation ---

    /// Borrow the underlying entity store.
    pub fn store(&self) -> &EntityStore<R> {
        &self.store
    }

    /// Mutably borrow the underlying entity store.
    pub fn store_mut(&mut self) -> &mut EntityStore<R> {
        &mut self.store
    }

    /// Borrow an entity's component row.
    pub fn get(&self, entity: EntityId) -> Option<&R> {
        self.store.get(entity)
    }

    /// Mutably borrow an entity's component row.
    pub fn get_mut(&mut self, entity: EntityId) -> Option<&mut R> {
        self.store.get_mut(entity)
    }

    /// Iterate `(entity, &row)` in ascending entity-id order.
    pub fn iter(&self) -> impl Iterator<Item = (EntityId, &R)> {
        self.store.iter()
    }

    /// The number of live entities.
    pub fn len(&self) -> usize {
        self.store.len()
    }

    /// Whether the world has no entities.
    pub fn is_empty(&self) -> bool {
        self.store.is_empty()
    }
}

impl<R: Default> World<R> {
    /// Spawn a new entity with a default component row.
    pub fn spawn(&mut self) -> EntityId {
        self.store.spawn()
    }
}

impl<R> Default for World<R> {
    fn default() -> Self {
        World::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures;

    #[derive(Debug, Clone, Default)]
    struct Row {
        ticks: u32,
    }

    /// A system that bumps every entity's `ticks` counter.
    struct BumpTicks;
    impl WorldSystem<Row> for BumpTicks {
        fn run(&self, store: &mut EntityStore<Row>) {
            let ids: Vec<_> = store.iter().map(|(id, _)| id).collect();
            for id in ids {
                store.get_mut(id).unwrap().ticks += 1;
            }
        }
    }

    fn world_with_one_entity() -> (World<Row>, EntityId) {
        let mut world = World::new();
        world.register_system(Box::new(BumpTicks));
        let e = world.spawn();
        (world, e)
    }

    #[test]
    fn new_and_default_worlds_are_empty() {
        let a: World<Row> = World::new();
        let b: World<Row> = World::default();
        assert!(a.is_empty());
        assert_eq!(a.len(), 0);
        assert_eq!(a.system_count(), 0);
        assert!(b.is_empty());
    }

    #[test]
    fn spawn_get_iter_delegate_to_store() {
        let mut world: World<Row> = World::new();
        let e = world.spawn();
        assert_eq!(world.len(), 1);
        assert!(!world.is_empty());
        assert_eq!(world.get(e).unwrap().ticks, 0);
        world.get_mut(e).unwrap().ticks = 5;
        assert_eq!(world.get(e).unwrap().ticks, 5);
        assert_eq!(world.iter().count(), 1);
        assert_eq!(world.store().len(), 1);
        assert_eq!(world.store_mut().len(), 1);
    }

    #[test]
    fn advance_runs_systems_when_frame_active() {
        let (mut world, e) = world_with_one_entity();
        assert_eq!(world.system_count(), 1);
        let frame = fixtures::active_engine_frame();
        world.advance(&FrameContext::new(&frame));
        assert_eq!(world.get(e).unwrap().ticks, 1);
        world.advance(&FrameContext::new(&frame));
        assert_eq!(world.get(e).unwrap().ticks, 2);
    }

    #[test]
    fn advance_skips_systems_for_a_skipped_frame() {
        let (mut world, e) = world_with_one_entity();
        let frame = fixtures::skipped_engine_frame();
        world.advance(&FrameContext::new(&frame));
        assert_eq!(world.get(e).unwrap().ticks, 0, "skipped frame runs no systems");
    }

    #[test]
    fn advance_skips_systems_when_no_runtime_step_ran() {
        // A visible frame with zero runtime steps is not "skipped", but still
        // must not advance the world.
        let (mut world, e) = world_with_one_entity();
        let frame = fixtures::active_zero_step_engine_frame();
        let ctx = FrameContext::new(&frame);
        assert!(!ctx.is_skipped());
        assert_eq!(ctx.runtime_step_count(), 0);
        world.advance(&ctx);
        assert_eq!(world.get(e).unwrap().ticks, 0, "zero-step frame runs no systems");
    }

    #[test]
    fn debug_renders_counts() {
        let (world, _) = world_with_one_entity();
        let s = format!("{world:?}");
        assert!(s.contains("World"));
        assert!(s.contains("entities"));
    }
}
