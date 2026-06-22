//! Deterministic command staging, applied at an explicit barrier.

use crate::column_set::ColumnSet;
use crate::entity_handle::EntityHandle;
use crate::world::World;

/// One staged structural command: a spawn, or a despawn of a specific handle.
///
/// A tagged struct rather than a sum type, so application dispatches on a `bool`
/// (gated side effects) instead of pattern-matching variants.
#[derive(Debug, Clone, Copy)]
struct Command {
    despawn: bool,
    target: Option<EntityHandle>,
}

/// The outcome of applying one command, in the same FIFO position it was queued.
#[derive(Debug, Clone, Copy)]
pub struct CommandOutcome {
    spawned: Option<EntityHandle>,
    despawned: Option<bool>,
}

impl CommandOutcome {
    /// The handle a spawn command produced, if this outcome was a spawn.
    pub fn spawned(&self) -> Option<EntityHandle> {
        self.spawned
    }

    /// Whether a despawn command removed a live entity, if this was a despawn.
    /// `Some(false)` means the target handle was stale/dead — a clean no-op.
    pub fn despawned(&self) -> Option<bool> {
        self.despawned
    }
}

/// The result of [`CommandBuffer::apply`]: one [`CommandOutcome`] per queued
/// command, in FIFO order, addressable by the ticket [`CommandBuffer::spawn`] /
/// [`CommandBuffer::despawn`] returned.
#[derive(Debug, Clone, Default)]
pub struct CommandReport {
    outcomes: Vec<CommandOutcome>,
}

impl CommandReport {
    /// The handles produced by spawn commands, in application order.
    pub fn spawned(&self) -> impl Iterator<Item = EntityHandle> + '_ {
        self.outcomes.iter().filter_map(CommandOutcome::spawned)
    }

    /// The outcome for the command with the given ticket, if any.
    pub fn outcome(&self, ticket: usize) -> Option<&CommandOutcome> {
        self.outcomes.get(ticket)
    }

    /// The number of commands applied.
    pub fn len(&self) -> usize {
        self.outcomes.len()
    }

    /// Whether no commands were applied.
    pub fn is_empty(&self) -> bool {
        self.outcomes.is_empty()
    }
}

/// A FIFO queue of structural commands applied to a world only at an explicit
/// barrier ([`apply`](Self::apply)).
///
/// Systems iterating the world cannot mutate its structure directly; they queue
/// spawns/despawns here, and the queue is drained against the world at a known
/// point. Commands apply in the exact order they were queued, and commands
/// against stale/dead entities fail cleanly (recorded as `despawned() ==
/// Some(false)`).
///
/// Phase 1 covers `spawn` and `despawn`. Generic component insert/remove commands
/// are deferred — see `crates/axiom-ecs/PHASE_1_DEFERRED.md`.
#[derive(Debug, Clone, Default)]
pub struct CommandBuffer {
    commands: Vec<Command>,
}

impl CommandBuffer {
    /// Create an empty command buffer.
    pub fn new() -> Self {
        CommandBuffer {
            commands: Vec::new(),
        }
    }

    /// Queue a spawn. Returns the command's ticket; after [`apply`](Self::apply)
    /// the produced handle is at [`CommandReport::outcome`]`(ticket)`.
    pub fn spawn(&mut self) -> usize {
        let ticket = self.commands.len();
        self.commands.push(Command {
            despawn: false,
            target: None,
        });
        ticket
    }

    /// Queue a despawn of `handle`. Returns the command's ticket.
    pub fn despawn(&mut self, handle: EntityHandle) -> usize {
        let ticket = self.commands.len();
        self.commands.push(Command {
            despawn: true,
            target: Some(handle),
        });
        ticket
    }

    /// The number of queued (not-yet-applied) commands.
    pub fn len(&self) -> usize {
        self.commands.len()
    }

    /// Whether no commands are queued.
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    /// Apply every queued command to `world` in FIFO order, then empty the queue.
    /// Returns a [`CommandReport`] of the per-command outcomes.
    pub fn apply<S: ColumnSet>(&mut self, world: &mut World<S>) -> CommandReport {
        let commands = std::mem::take(&mut self.commands);
        let outcomes = commands
            .into_iter()
            .map(|command| CommandOutcome {
                spawned: (!command.despawn).then(|| world.spawn_handle()),
                // For a despawn command `target` is always `Some`; `filter` keeps
                // it only for despawns, so there is no unreachable `None` arm.
                despawned: command
                    .target
                    .filter(|_| command.despawn)
                    .map(|handle| world.despawn_handle(handle)),
            })
            .collect();
        CommandReport { outcomes }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::column_set::ColumnSet;
    use crate::component_column::ComponentColumn;
    use crate::erased_column::ErasedColumn;
    use crate::world::World;

    #[derive(Default)]
    struct Storage {
        value: ComponentColumn<u32>,
    }
    impl ColumnSet for Storage {
        fn columns(&self) -> Vec<(&'static str, &dyn ErasedColumn)> {
            vec![("value", &self.value)]
        }
        fn columns_mut(&mut self) -> Vec<(&'static str, &mut dyn ErasedColumn)> {
            vec![("value", &mut self.value)]
        }
    }

    #[test]
    fn new_and_default_are_empty() {
        assert!(CommandBuffer::new().is_empty());
        assert!(CommandBuffer::default().is_empty());
        assert_eq!(CommandBuffer::new().len(), 0);
    }

    #[test]
    fn commands_apply_only_at_the_barrier() {
        let mut world: World<Storage> = World::new();
        let mut buffer = CommandBuffer::new();
        buffer.spawn();
        buffer.spawn();
        // Nothing happens to the world until apply.
        assert_eq!(world.entity_count(), 0);
        assert_eq!(buffer.len(), 2);
        let report = buffer.apply(&mut world);
        assert_eq!(world.entity_count(), 2, "spawns happened at the barrier");
        assert!(buffer.is_empty(), "the queue drains on apply");
        assert_eq!(report.len(), 2);
    }

    #[test]
    fn spawn_commands_report_handles_in_fifo_order() {
        let mut world: World<Storage> = World::new();
        let mut buffer = CommandBuffer::new();
        let t0 = buffer.spawn();
        let t1 = buffer.spawn();
        let report = buffer.apply(&mut world);
        let first = report.outcome(t0).unwrap().spawned().unwrap();
        let second = report.outcome(t1).unwrap().spawned().unwrap();
        assert_eq!(first.id().raw(), 1);
        assert_eq!(second.id().raw(), 2);
        let spawned: Vec<u64> = report.spawned().map(|h| h.id().raw()).collect();
        assert_eq!(spawned, vec![1, 2]);
    }

    #[test]
    fn despawn_command_removes_entity_and_components_at_barrier() {
        let mut world: World<Storage> = World::new();
        let handle = world.spawn_handle();
        world.storage_mut().value.insert(handle.id(), 7);
        let mut buffer = CommandBuffer::new();
        let ticket = buffer.despawn(handle);
        // Still live until the barrier.
        assert_eq!(world.entity_count(), 1);
        let report = buffer.apply(&mut world);
        assert_eq!(report.outcome(ticket).unwrap().despawned(), Some(true));
        assert_eq!(world.entity_count(), 0);
        assert!(world.storage().value.get(handle.id()).is_none());
        // The immutable column view exposes the same single named column.
        let columns = world.storage().columns();
        assert_eq!(columns.len(), 1);
        assert_eq!(columns[0].0, "value");
    }

    #[test]
    fn despawn_of_stale_handle_fails_cleanly() {
        let mut world: World<Storage> = World::new();
        let handle = world.spawn_handle();
        world.despawn_handle(handle); // already gone
        let mut buffer = CommandBuffer::new();
        let ticket = buffer.despawn(handle);
        let report = buffer.apply(&mut world);
        assert_eq!(report.outcome(ticket).unwrap().despawned(), Some(false));
    }

    #[test]
    fn mixed_commands_apply_in_order() {
        let mut world: World<Storage> = World::new();
        let mut buffer = CommandBuffer::new();
        let spawn_ticket = buffer.spawn();
        // Apply the spawn first to get a handle, then despawn it in a second batch.
        let report = buffer.apply(&mut world);
        let handle = report.outcome(spawn_ticket).unwrap().spawned().unwrap();
        let despawn_ticket = buffer.despawn(handle);
        let report2 = buffer.apply(&mut world);
        assert_eq!(
            report2.outcome(despawn_ticket).unwrap().despawned(),
            Some(true)
        );
        assert_eq!(world.entity_count(), 0);
        // A spawn outcome has no despawn record and vice versa.
        assert_eq!(report.outcome(spawn_ticket).unwrap().despawned(), None);
        assert!(report2.outcome(despawn_ticket).unwrap().spawned().is_none());
        assert!(report.outcome(99).is_none());
        assert!(!report.is_empty());
    }
}
