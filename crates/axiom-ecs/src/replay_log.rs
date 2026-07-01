//! A deterministic record of ECS commands for replay from a known world.

use crate::column_set::ColumnSet;
use crate::command_buffer::{CommandBuffer, CommandReport};
use crate::entity_handle::EntityHandle;
use crate::world::World;

/// One recorded structural command (a spawn, or a despawn of a handle).
#[derive(Debug, Clone, Copy)]
struct Recorded {
    despawn: bool,
    target: Option<EntityHandle>,
}

/// An ordered log of structural commands that can be replayed against a world.
///
/// Given the same initial world and the same recorded sequence, replay produces
/// the same final world — the basis future networking/replay systems build on.
/// The log records only spawn/despawn (the [`CommandBuffer`] surface for Phase 1);
/// replay re-issues them through a fresh command buffer applied at one barrier, so
/// ordering is identical to live application.
#[derive(Debug, Clone, Default)]
pub struct ReplayLog {
    commands: Vec<Recorded>,
}

impl ReplayLog {
    /// Create an empty replay log.
    pub fn new() -> Self {
        ReplayLog {
            commands: Vec::new(),
        }
    }

    /// Record a spawn command.
    pub fn record_spawn(&mut self) {
        self.commands.push(Recorded {
            despawn: false,
            target: None,
        });
    }

    /// Record a despawn of `handle`.
    pub fn record_despawn(&mut self, handle: EntityHandle) {
        self.commands.push(Recorded {
            despawn: true,
            target: Some(handle),
        });
    }

    /// The number of recorded commands.
    pub fn len(&self) -> usize {
        self.commands.len()
    }

    /// Whether nothing is recorded.
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    /// Replay the recorded commands against `world`, in order, at one barrier.
    pub fn replay<S: ColumnSet>(&self, world: &mut World<S>) -> CommandReport {
        let mut buffer = CommandBuffer::new();
        self.commands.iter().for_each(|command| {
            command
                .despawn
                .then(|| command.target.map(|handle| buffer.despawn(handle)));
            (!command.despawn).then(|| buffer.spawn());
        });
        buffer.apply(world)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::column_set::ColumnSet;
    use crate::component_column::ComponentColumn;
    use crate::erased_column::ErasedColumn;
    use axiom_kernel::BinaryWriter;

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

    fn snapshot_bytes(world: &World<Storage>) -> Vec<u8> {
        let mut writer = BinaryWriter::new();
        world.write_snapshot(&mut writer);
        writer.into_bytes()
    }

    #[test]
    fn new_and_default_are_empty() {
        assert!(ReplayLog::new().is_empty());
        assert!(ReplayLog::default().is_empty());
        assert_eq!(ReplayLog::new().len(), 0);
    }

    #[test]
    fn same_log_reproduces_the_same_world() {
        let mut log = ReplayLog::new();
        log.record_spawn();
        log.record_spawn();
        assert_eq!(log.len(), 2);

        let mut world_a: World<Storage> = World::new();
        let mut world_b: World<Storage> = World::new();
        log.replay(&mut world_a);
        log.replay(&mut world_b);
        assert_eq!(snapshot_bytes(&world_a), snapshot_bytes(&world_b));
        assert_eq!(world_a.entity_count(), 2);
    }

    #[test]
    fn replay_applies_despawn_commands() {
        let mut world: World<Storage> = World::new();
        let first = world.spawn_handle();
        world.spawn_handle();
        let mut log = ReplayLog::new();
        log.record_despawn(first);
        let report = log.replay(&mut world);
        assert_eq!(report.outcome(0).unwrap().despawned(), Some(true));
        assert_eq!(world.entity_count(), 1);
    }

    #[test]
    fn reordering_independent_spawns_is_not_semantically_meaningful() {
        let mut a_log = ReplayLog::new();
        a_log.record_spawn();
        a_log.record_spawn();
        let mut world_a: World<Storage> = World::new();
        a_log.replay(&mut world_a);

        let mut b_log = ReplayLog::new();
        b_log.record_spawn();
        b_log.record_spawn();
        let mut world_b: World<Storage> = World::new();
        b_log.replay(&mut world_b);

        assert_eq!(snapshot_bytes(&world_a), snapshot_bytes(&world_b));
    }

    #[test]
    fn different_logs_produce_different_worlds() {
        let mut two = ReplayLog::new();
        two.record_spawn();
        two.record_spawn();
        let mut three = ReplayLog::new();
        three.record_spawn();
        three.record_spawn();
        three.record_spawn();

        let mut world_two: World<Storage> = World::new();
        two.replay(&mut world_two);
        let mut world_three: World<Storage> = World::new();
        three.replay(&mut world_three);

        assert_ne!(
            snapshot_bytes(&world_two),
            snapshot_bytes(&world_three),
            "a meaningfully different command sequence yields a different world"
        );
    }
}
