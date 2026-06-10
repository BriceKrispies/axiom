//! The world: live entities + component storage + the systems that advance it.

use axiom_frame::FrameContext;
use axiom_kernel::{
    BinaryReader, BinaryWriter, EntityId, KernelError, KernelErrorCode, KernelErrorScope,
    KernelResult, SchemaVersion, TypeSchema,
};

use crate::column_set::ColumnSet;
use crate::entity_registry::EntityRegistry;
use crate::schedule_phase::SchedulePhase;
use crate::world_step::WorldStep;
use crate::world_system::WorldSystem;

/// Wire schema version of a serialized world.
const WORLD_SCHEMA: SchemaVersion = SchemaVersion::new(1, 0);

/// The single deterministic world model: an [`EntityRegistry`] of live
/// entities, a consumer-defined component storage `S` (a struct of
/// [`crate::ComponentColumn`]s), and an ordered list of [`WorldSystem`]s.
///
/// `S` is generic so each consumer (and the modules it composes) defines its
/// own component set; the world knows nothing about what components exist.
/// `advance` consumes a [`FrameContext`] and runs the registered systems only
/// when the frame is active — the same gating `axiom-scene::advance` uses, and
/// this layer's adapter over the frame layer. Systems run in two ordered
/// [`SchedulePhase`]s: every `Startup` system runs once on the first active
/// advance, then every `Update` system runs on each active advance.
pub struct World<S> {
    entities: EntityRegistry,
    storage: S,
    startup: Vec<Box<dyn WorldSystem<S>>>,
    update: Vec<Box<dyn WorldSystem<S>>>,
    startup_done: bool,
}

impl<S> std::fmt::Debug for World<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("World")
            .field("entities", &self.entities.len())
            .field("systems", &self.system_count())
            .finish()
    }
}

impl<S: Default> World<S> {
    /// Create an empty world with default storage and no systems.
    pub fn new() -> Self {
        World {
            entities: EntityRegistry::new(),
            storage: S::default(),
            startup: Vec::new(),
            update: Vec::new(),
            startup_done: false,
        }
    }
}

impl<S> World<S> {
    /// Register a system in the [`SchedulePhase::Update`] phase: it runs on every
    /// active advance, in registration order. Shorthand for
    /// [`Self::register_system_in`] with `Update`.
    pub fn register_system(&mut self, system: Box<dyn WorldSystem<S>>) {
        self.register_system_in(SchedulePhase::Update, system);
    }

    /// Register a system into an explicit [`SchedulePhase`]. `Startup` systems run
    /// exactly once, on the first active advance, before any `Update` system;
    /// `Update` systems run on every active advance. Within a phase, systems run
    /// in registration order.
    pub fn register_system_in(&mut self, phase: SchedulePhase, system: Box<dyn WorldSystem<S>>) {
        match phase {
            SchedulePhase::Startup => self.startup.push(system),
            SchedulePhase::Update => self.update.push(system),
        }
    }

    /// The number of registered systems across all phases.
    pub fn system_count(&self) -> usize {
        self.startup.len() + self.update.len()
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

    /// Advance the world for one engine frame at logical time `tick`: run the
    /// registered systems by phase — every `Startup` system once on the first
    /// active advance, then every `Update` system — but only when the frame is
    /// active (not skipped and it executed at least one runtime step). A frame
    /// that is not active runs nothing and does not consume the startup phase, so
    /// `Startup` systems still fire on the first *active* advance. The caller owns
    /// the tick — a real app passes its accumulating simulation tick; a test or a
    /// frame-N renderer passes the frame it wants. The tick reaches systems via
    /// [`WorldStep`].
    pub fn advance(&mut self, tick: u64, frame: &FrameContext<'_>) {
        if frame.is_skipped() || frame.runtime_step_count() == 0 {
            return;
        }
        let step = WorldStep::new(tick);
        if !self.startup_done {
            for system in &self.startup {
                system.run(&step, &self.entities, &mut self.storage);
            }
            self.startup_done = true;
        }
        for system in &self.update {
            system.run(&step, &self.entities, &mut self.storage);
        }
    }
}

impl<S: Default> Default for World<S> {
    fn default() -> Self {
        World::new()
    }
}

impl<S: ColumnSet> World<S> {
    /// Serialize the whole world's component state: a schema header, the column
    /// count, then each column's bytes in `columns()` order.
    pub fn serialize(&self, writer: &mut BinaryWriter) {
        WORLD_SCHEMA.write_to(writer);
        let columns = self.storage.columns();
        writer.write_u32(columns.len() as u32);
        for (_, column) in columns {
            column.write(writer);
        }
    }

    /// Replace the world's component columns with state previously produced by
    /// [`Self::serialize`]. Entities and systems are untouched; each column's
    /// contents are replaced wholesale.
    pub fn deserialize(&mut self, reader: &mut BinaryReader<'_>) -> KernelResult<()> {
        let version = SchemaVersion::read_from(reader)?;
        if !WORLD_SCHEMA.is_compatible_with(version) {
            return Err(KernelError::new(
                KernelErrorScope::Binary,
                KernelErrorCode::SchemaVersionMismatch,
                "world schema major version is incompatible",
            ));
        }
        let count = reader.read_u32()? as usize;
        let columns = self.storage.columns_mut();
        if count != columns.len() {
            return Err(KernelError::new(
                KernelErrorScope::Binary,
                KernelErrorCode::TruncatedData,
                "serialized world column count does not match the storage",
            ));
        }
        for (_, column) in columns {
            column.read_replace(reader)?;
        }
        Ok(())
    }

    /// Describe the world: per column, its role name, component schema, and
    /// entry count — the world describing its own contents as data.
    pub fn describe(&self) -> Vec<(&'static str, TypeSchema, usize)> {
        self.storage
            .columns()
            .into_iter()
            .map(|(name, column)| (name, column.describe(), column.entry_count()))
            .collect()
    }
}

#[cfg(test)]
mod serial_tests {
    use super::*;
    use crate::component_column::ComponentColumn;
    use crate::erased_column::ErasedColumn;

    #[derive(Default)]
    struct TestStorage {
        a: ComponentColumn<u32>,
        b: ComponentColumn<u64>,
    }

    impl ColumnSet for TestStorage {
        fn columns(&self) -> Vec<(&'static str, &dyn ErasedColumn)> {
            vec![("a", &self.a), ("b", &self.b)]
        }

        fn columns_mut(&mut self) -> Vec<(&'static str, &mut dyn ErasedColumn)> {
            vec![("a", &mut self.a), ("b", &mut self.b)]
        }
    }

    fn populated() -> World<TestStorage> {
        let mut world: World<TestStorage> = World::new();
        let e1 = world.spawn();
        let e2 = world.spawn();
        world.storage_mut().a.insert(e1, 10);
        world.storage_mut().a.insert(e2, 20);
        world.storage_mut().b.insert(e1, 1000);
        world
    }

    #[test]
    fn whole_world_round_trips() {
        let world = populated();
        let mut w = BinaryWriter::new();
        world.serialize(&mut w);
        let bytes = w.into_bytes();

        let mut loaded: World<TestStorage> = World::new();
        loaded.deserialize(&mut BinaryReader::new(&bytes)).unwrap();
        assert_eq!(loaded.storage().a.len(), 2);
        assert_eq!(loaded.storage().a.get(EntityId::from_raw(1)), Some(&10));
        assert_eq!(loaded.storage().a.get(EntityId::from_raw(2)), Some(&20));
        assert_eq!(loaded.storage().b.get(EntityId::from_raw(1)), Some(&1000));
    }

    #[test]
    fn describe_reports_columns_schemas_and_counts() {
        let world = populated();
        let description = world.describe();
        assert_eq!(description.len(), 2);
        assert_eq!(description[0].0, "a");
        assert_eq!(description[0].1.name(), "u32");
        assert_eq!(description[0].2, 2);
        assert_eq!(description[1].0, "b");
        assert_eq!(description[1].1.name(), "u64");
        assert_eq!(description[1].2, 1);
    }

    #[test]
    fn deserialize_rejects_incompatible_schema() {
        let mut w = BinaryWriter::new();
        SchemaVersion::new(WORLD_SCHEMA.major() + 1, 0).write_to(&mut w);
        let bytes = w.into_bytes();
        let mut world: World<TestStorage> = World::new();
        assert_eq!(
            world
                .deserialize(&mut BinaryReader::new(&bytes))
                .unwrap_err()
                .code(),
            KernelErrorCode::SchemaVersionMismatch
        );
    }

    #[test]
    fn deserialize_rejects_wrong_column_count() {
        let mut w = BinaryWriter::new();
        WORLD_SCHEMA.write_to(&mut w);
        w.write_u32(99); // storage has 2 columns, not 99
        let bytes = w.into_bytes();
        let mut world: World<TestStorage> = World::new();
        assert_eq!(
            world
                .deserialize(&mut BinaryReader::new(&bytes))
                .unwrap_err()
                .code(),
            KernelErrorCode::TruncatedData
        );
    }

    #[test]
    fn deserialize_rejects_truncation_at_every_prefix() {
        let world = populated();
        let mut w = BinaryWriter::new();
        world.serialize(&mut w);
        let bytes = w.into_bytes();
        for len in 0..bytes.len() {
            let mut fresh: World<TestStorage> = World::new();
            assert!(fresh
                .deserialize(&mut BinaryReader::new(&bytes[..len]))
                .is_err());
        }
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
        fn run(&self, _step: &WorldStep, entities: &EntityRegistry, storage: &mut Storage) {
            let pairs: Vec<(EntityId, i32)> = entities
                .iter()
                .filter_map(|e| storage.value.get(e).map(|v| (e, *v)))
                .collect();
            for (e, v) in pairs {
                storage.doubled.insert(e, v * 2);
            }
        }
    }

    /// Records the advance tick into the `doubled` column for every entity.
    struct RecordTick;
    impl WorldSystem<Storage> for RecordTick {
        fn run(&self, step: &WorldStep, entities: &EntityRegistry, storage: &mut Storage) {
            let ids: Vec<EntityId> = entities.iter().collect();
            for e in ids {
                storage.doubled.insert(e, step.tick() as i32);
            }
        }
    }

    /// Appends `mark` as a decimal digit onto `doubled` each run, so the final
    /// value encodes the exact sequence of systems that ran (e.g. `12` = a
    /// `Mark(1)` then a `Mark(2)`).
    struct Mark(i32);
    impl WorldSystem<Storage> for Mark {
        fn run(&self, _step: &WorldStep, entities: &EntityRegistry, storage: &mut Storage) {
            let ids: Vec<EntityId> = entities.iter().collect();
            for e in ids {
                let n = storage.doubled.get(e).copied().unwrap_or(0);
                storage.doubled.insert(e, n * 10 + self.0);
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
        world.advance(0, &FrameContext::new(&frame));
        assert_eq!(world.storage().doubled.get(e), Some(&10));
    }

    #[test]
    fn advance_passes_the_tick_to_systems() {
        let mut world: World<Storage> = World::new();
        world.register_system(Box::new(RecordTick));
        let e = world.spawn();
        let frame = fixtures::active_engine_frame();
        world.advance(42, &FrameContext::new(&frame));
        assert_eq!(world.storage().doubled.get(e), Some(&42));
    }

    #[test]
    fn advance_skips_systems_for_a_skipped_frame() {
        let (mut world, e) = world_with_one_value(5);
        let frame = fixtures::skipped_engine_frame();
        world.advance(0, &FrameContext::new(&frame));
        assert!(
            world.storage().doubled.get(e).is_none(),
            "skipped frame runs no systems"
        );
    }

    #[test]
    fn advance_skips_systems_when_no_runtime_step_ran() {
        // Visible but zero-step: not "skipped", but still must not advance.
        let (mut world, e) = world_with_one_value(5);
        let frame = fixtures::active_zero_step_engine_frame();
        let ctx = FrameContext::new(&frame);
        assert!(!ctx.is_skipped());
        assert_eq!(ctx.runtime_step_count(), 0);
        world.advance(0, &ctx);
        assert!(
            world.storage().doubled.get(e).is_none(),
            "zero-step frame runs no systems"
        );
    }

    #[test]
    fn startup_runs_once_before_update_on_each_active_advance() {
        let mut world: World<Storage> = World::new();
        world.register_system_in(SchedulePhase::Startup, Box::new(Mark(1)));
        world.register_system_in(SchedulePhase::Update, Box::new(Mark(2)));
        assert_eq!(world.system_count(), 2);
        let e = world.spawn();
        let frame = fixtures::active_engine_frame();
        // Advance 1: startup(1) then update(2) -> 12.
        world.advance(0, &FrameContext::new(&frame));
        assert_eq!(world.storage().doubled.get(e), Some(&12));
        // Advance 2: update only -> 12*10 + 2 = 122.
        world.advance(1, &FrameContext::new(&frame));
        assert_eq!(world.storage().doubled.get(e), Some(&122));
        // Advance 3: update only -> 1222. Startup never runs again.
        world.advance(2, &FrameContext::new(&frame));
        assert_eq!(world.storage().doubled.get(e), Some(&1222));
    }

    #[test]
    fn an_inactive_first_frame_does_not_consume_the_startup_phase() {
        let mut world: World<Storage> = World::new();
        world.register_system_in(SchedulePhase::Startup, Box::new(Mark(1)));
        let e = world.spawn();
        // A skipped frame runs nothing and leaves startup pending.
        world.advance(0, &FrameContext::new(&fixtures::skipped_engine_frame()));
        assert!(world.storage().doubled.get(e).is_none());
        // The first *active* advance is where startup finally fires.
        world.advance(1, &FrameContext::new(&fixtures::active_engine_frame()));
        assert_eq!(world.storage().doubled.get(e), Some(&1));
    }

    #[test]
    fn debug_renders_counts() {
        let (world, _) = world_with_one_value(1);
        let s = format!("{world:?}");
        assert!(s.contains("World"));
        assert!(s.contains("entities"));
    }
}
