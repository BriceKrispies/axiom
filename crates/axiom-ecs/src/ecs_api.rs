//! The curated entry point for constructing ECS primitives.

use crate::command_buffer::CommandBuffer;
use crate::component_command_buffer::ComponentCommandBuffer;
use crate::entity_registry::EntityRegistry;
use crate::event_buffer::EventBuffer;
use crate::replay_log::ReplayLog;
use crate::tracked_column::TrackedColumn;
use crate::world::World;

/// The documented entry point to the ECS layer.
///
/// `EcsApi` is the front door for constructing the layer's primitives — a world,
/// a standalone entity registry, command and event buffers, a resource store, a
/// change-tracked column, and a replay log. The primitive types remain public so
/// consumers can name them (an engine layer may expose curated primitives), but
/// `EcsApi` is the single place that constructs them, so a consumer never has to
/// learn each type's constructor.
#[derive(Debug, Clone, Copy, Default)]
pub struct EcsApi;

impl EcsApi {
    /// A new, empty world over consumer-defined component storage `S`.
    pub fn world<S: Default>(&self) -> World<S> {
        World::new()
    }

    /// A new, empty entity registry.
    pub fn registry(&self) -> EntityRegistry {
        EntityRegistry::new()
    }

    /// A new, empty command buffer (spawn/despawn).
    pub fn command_buffer(&self) -> CommandBuffer {
        CommandBuffer::new()
    }

    /// A new, empty component-command buffer for typed component insert/remove
    /// against a consumer storage `S`.
    pub fn component_command_buffer<S>(&self) -> ComponentCommandBuffer<S> {
        ComponentCommandBuffer::new()
    }

    /// A new, empty replay log.
    pub fn replay_log(&self) -> ReplayLog {
        ReplayLog::new()
    }

    /// A new, empty event buffer for event type `E`.
    pub fn event_buffer<E>(&self) -> EventBuffer<E> {
        EventBuffer::new()
    }

    /// A new, empty change-tracked column for component type `T`.
    pub fn tracked_column<T>(&self) -> TrackedColumn<T> {
        TrackedColumn::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Storage;

    #[test]
    fn constructs_empty_primitives() {
        let api = EcsApi;
        assert!(api.world::<Storage>().is_empty());
        assert!(api.registry().is_empty());
        assert!(api.command_buffer().is_empty());
        assert!(api.component_command_buffer::<Storage>().is_empty());
        assert!(api.replay_log().is_empty());
        assert!(api.event_buffer::<u32>().is_empty());
        assert!(api.tracked_column::<u32>().is_empty());
    }

    #[test]
    fn default_and_debug() {
        let api = <EcsApi as Default>::default();
        assert!(format!("{api:?}").contains("EcsApi"));
        assert!(api.registry().is_empty());
    }
}
