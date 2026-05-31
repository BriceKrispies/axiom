//! The contract for a system that runs over the world each frame.

use crate::entity_registry::EntityRegistry;

/// A unit of per-frame behavior over the world.
///
/// A `WorldSystem` reads the live [`EntityRegistry`] and reads/mutates the
/// component storage `S`; it is run in registration order by
/// [`crate::World::advance`] when the engine frame is active. Determinism is
/// the registry's and the columns' (both ordered); a system is a pure
/// transformation over them.
pub trait WorldSystem<S> {
    /// Run the system for one frame over the live entities and the storage.
    fn run(&self, entities: &EntityRegistry, storage: &mut S);
}
