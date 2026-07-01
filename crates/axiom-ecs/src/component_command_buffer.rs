//! Deterministic, typed component insert/remove commands, applied at a barrier.
//!
//! The companion to [`crate::CommandBuffer`] (which stages spawn/despawn): this
//! stages **typed component mutations** against a consumer's storage `S` without
//! the layer ever knowing `S`'s component types — and without `TypeId`, `Any`,
//! `unsafe`, or downcasting. The trick is a caller-supplied **selector**
//! `fn(&mut S) -> &mut ComponentColumn<T>` (e.g. `|s| &mut s.locals`): the caller
//! names the concrete column at stage time, so `T` is never recovered from
//! erasure. Each staged command is a uniform boxed op, applied in FIFO order at
//! [`apply`](Self::apply).

use axiom_kernel::EntityId;

use crate::column_set::ColumnSet;
use crate::command_buffer::{CommandOutcome, CommandReport};
use crate::component_column::ComponentColumn;
use crate::world::World;

/// One staged typed-component command: a boxed op that applies itself to the
/// world and reports its outcome. Uniform, so [`apply`](ComponentCommandBuffer::apply)
/// is a branchless `map` with no per-kind matching.
type Op<S> = Box<dyn FnOnce(&mut World<S>) -> CommandOutcome>;

/// A FIFO queue of typed component insert/remove commands, applied to a
/// `World<S>` only at an explicit barrier.
///
/// Construct via [`crate::EcsApi::component_command_buffer`] or [`Self::new`].
/// Unlike [`crate::CommandBuffer`] it is neither `Clone` nor derive-`Debug`
/// (it holds boxed closures); it is the deferred, typed companion to that
/// structural buffer.
pub struct ComponentCommandBuffer<S> {
    ops: Vec<Op<S>>,
}

impl<S> ComponentCommandBuffer<S> {
    /// Create an empty buffer.
    pub fn new() -> Self {
        ComponentCommandBuffer { ops: Vec::new() }
    }

    /// Queue setting `entity`'s component of type `T` to `value`, routed to its
    /// column by `selector`. Returns the command's ticket; after
    /// [`apply`](Self::apply) the outcome's [`CommandOutcome::inserted`] reports
    /// whether a previous value was replaced.
    pub fn insert_component<T: 'static>(
        &mut self,
        entity: EntityId,
        value: T,
        selector: fn(&mut S) -> &mut ComponentColumn<T>,
    ) -> usize
    where
        S: 'static,
    {
        let ticket = self.ops.len();
        self.ops.push(Box::new(move |world: &mut World<S>| {
            CommandOutcome::from_insert(
                selector(world.storage_mut())
                    .insert(entity, value)
                    .is_some(),
            )
        }));
        ticket
    }

    /// Queue removing `entity`'s component of type `T`, routed by `selector`.
    /// Returns the ticket; the outcome's [`CommandOutcome::removed`] reports
    /// whether a value was present.
    pub fn remove_component<T: 'static>(
        &mut self,
        entity: EntityId,
        selector: fn(&mut S) -> &mut ComponentColumn<T>,
    ) -> usize
    where
        S: 'static,
    {
        let ticket = self.ops.len();
        self.ops.push(Box::new(move |world: &mut World<S>| {
            CommandOutcome::from_remove(selector(world.storage_mut()).remove(entity).is_some())
        }));
        ticket
    }

    /// The number of queued (not-yet-applied) commands.
    pub fn len(&self) -> usize {
        self.ops.len()
    }

    /// Whether no commands are queued.
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    /// Apply every queued command to `world` in FIFO order, then empty the queue.
    /// Returns a [`CommandReport`] with one [`CommandOutcome`] per command,
    /// addressable by the ticket each stage call returned.
    pub fn apply(&mut self, world: &mut World<S>) -> CommandReport
    where
        S: ColumnSet,
    {
        let ops = std::mem::take(&mut self.ops);
        CommandReport::from_outcomes(ops.into_iter().map(|op| op(world)).collect())
    }
}

impl<S> Default for ComponentCommandBuffer<S> {
    fn default() -> Self {
        ComponentCommandBuffer::new()
    }
}

impl<S> core::fmt::Debug for ComponentCommandBuffer<S> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ComponentCommandBuffer")
            .field("len", &self.ops.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::erased_column::ErasedColumn;

    #[derive(Default)]
    struct Storage {
        a: ComponentColumn<u32>,
        b: ComponentColumn<u64>,
    }
    impl ColumnSet for Storage {
        fn columns(&self) -> Vec<(&'static str, &dyn ErasedColumn)> {
            vec![("a", &self.a), ("b", &self.b)]
        }
        fn columns_mut(&mut self) -> Vec<(&'static str, &mut dyn ErasedColumn)> {
            vec![("a", &mut self.a), ("b", &mut self.b)]
        }
    }

    fn select_a(s: &mut Storage) -> &mut ComponentColumn<u32> {
        &mut s.a
    }
    fn select_b(s: &mut Storage) -> &mut ComponentColumn<u64> {
        &mut s.b
    }

    #[test]
    fn new_and_default_are_empty() {
        let from_new: ComponentCommandBuffer<Storage> = ComponentCommandBuffer::new();
        let from_default: ComponentCommandBuffer<Storage> = ComponentCommandBuffer::default();
        assert!(from_new.is_empty());
        assert_eq!(from_new.len(), 0);
        assert!(from_default.is_empty());
    }

    #[test]
    fn commands_apply_only_at_the_barrier_in_fifo_order() {
        let mut world: World<Storage> = World::new();
        let entity = world.spawn_handle().id();
        let mut buffer = ComponentCommandBuffer::new();
        let t0 = buffer.insert_component(entity, 10u32, select_a);
        let t1 = buffer.insert_component(entity, 5u64, select_b);
        assert!(world.storage().a.get(entity).is_none());
        assert_eq!(buffer.len(), 2);

        let report = buffer.apply(&mut world);
        assert_eq!(world.storage().a.get(entity), Some(&10));
        assert_eq!(world.storage().b.get(entity), Some(&5));
        assert!(buffer.is_empty(), "the queue drains on apply");
        assert_eq!(report.len(), 2);
        assert_eq!(report.outcome(t0).unwrap().inserted(), Some(false));
        assert_eq!(report.outcome(t1).unwrap().inserted(), Some(false));
    }

    #[test]
    fn insert_reports_replacement_and_remove_reports_presence() {
        let mut world: World<Storage> = World::new();
        let entity = world.spawn_handle().id();
        world.storage_mut().a.insert(entity, 1);

        let mut buffer = ComponentCommandBuffer::new();
        let overwrite = buffer.insert_component(entity, 2u32, select_a);
        let remove_present = buffer.remove_component(entity, select_a);
        let remove_absent = buffer.remove_component(entity, select_b);
        let report = buffer.apply(&mut world);

        assert_eq!(report.outcome(overwrite).unwrap().inserted(), Some(true));
        assert_eq!(
            report.outcome(remove_present).unwrap().removed(),
            Some(true)
        );
        assert_eq!(
            report.outcome(remove_absent).unwrap().removed(),
            Some(false)
        );
        assert!(world.storage().a.get(entity).is_none());
    }

    #[test]
    fn outcome_accessors_are_none_for_the_other_kinds() {
        let mut world: World<Storage> = World::new();
        let entity = world.spawn_handle().id();
        let mut buffer = ComponentCommandBuffer::new();
        let insert = buffer.insert_component(entity, 7u32, select_a);
        let remove = buffer.remove_component(entity, select_a);
        let report = buffer.apply(&mut world);

        let insert_outcome = report.outcome(insert).unwrap();
        assert!(insert_outcome.spawned().is_none());
        assert_eq!(insert_outcome.despawned(), None);
        assert_eq!(insert_outcome.removed(), None);

        let remove_outcome = report.outcome(remove).unwrap();
        assert_eq!(remove_outcome.inserted(), None);
        assert_eq!(remove_outcome.spawned(), None);
    }

    #[test]
    fn seam_inserts_compose_with_despawn_cleanup() {
        let mut world: World<Storage> = World::new();
        let handle = world.spawn_handle();
        let entity = handle.id();
        let mut buffer = ComponentCommandBuffer::new();
        buffer.insert_component(entity, 1u32, select_a);
        buffer.insert_component(entity, 2u64, select_b);
        buffer.apply(&mut world);

        let columns = world.storage().columns();
        assert_eq!(columns.len(), 2);
        assert_eq!(columns[0].0, "a");
        assert_eq!(columns[1].0, "b");

        assert!(world.despawn_handle(handle));
        assert!(world.storage().a.get(entity).is_none());
        assert!(world.storage().b.get(entity).is_none());
    }

    #[test]
    fn apply_on_empty_buffer_is_an_empty_report() {
        let mut world: World<Storage> = World::new();
        let mut buffer: ComponentCommandBuffer<Storage> = ComponentCommandBuffer::new();
        let report = buffer.apply(&mut world);
        assert!(report.is_empty());
        assert_eq!(report.len(), 0);
    }

    #[test]
    fn debug_reports_pending_length() {
        let mut world: World<Storage> = World::new();
        let entity = world.spawn_handle().id();
        let mut buffer = ComponentCommandBuffer::new();
        buffer.insert_component(entity, 1u32, select_a);
        let rendered = format!("{buffer:?}");
        assert!(rendered.contains("ComponentCommandBuffer"));
        assert!(rendered.contains("len"));
        assert!(rendered.contains('1'));
    }
}
