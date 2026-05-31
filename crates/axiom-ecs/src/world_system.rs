//! The contract for a system that runs over the world each frame.

use crate::entity_store::EntityStore;

/// A unit of per-frame behavior over the world.
///
/// A `WorldSystem` reads and mutates the entity store; it is run in
/// registration order by [`crate::World::advance`] when the engine frame is
/// active. Systems carry no state that escapes the store (determinism is the
/// store's; a system is a pure transformation over it).
pub trait WorldSystem<R> {
    /// Run the system against the world's entity store for one frame.
    fn run(&self, store: &mut EntityStore<R>);
}
