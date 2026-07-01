//! A live observation of the ECS world.

use axiom_ecs::World;
use axiom_kernel::{BinaryReader, BinaryWriter, KernelResult, SchemaVersion};

/// The wire schema version of a [`WorldReport`]. Bumped on incompatible layout
/// changes; the major component gates compatibility.
const SCHEMA: SchemaVersion = SchemaVersion::new(1, 0);

/// A small, inspectable summary of the ECS world at an instant: how many
/// entities it holds and how many systems it advances.
///
/// This is the introspection layer's adapter over the ecs layer:
/// the world is foundational, and observability sits on top of it. It is a live
/// summary of "how big is the world right now". Like [`crate::FrameReport`] it
/// is serializable through the kernel binary primitives, so an agent can read
/// it over the same byte channel — the [`crate::IntrospectApi`] facade exposes
/// both the per-frame snapshot and this world snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WorldReport {
    entities: u64,
    systems: u64,
}

impl WorldReport {
    /// Observe a world, capturing its entity and system counts.
    pub fn observe<S>(world: &World<S>) -> Self {
        WorldReport {
            entities: world.entity_count() as u64,
            systems: world.system_count() as u64,
        }
    }

    /// The number of live entities in the world.
    pub const fn entities(&self) -> u64 {
        self.entities
    }

    /// The number of systems the world advances each frame.
    pub const fn systems(&self) -> u64 {
        self.systems
    }

    /// Serialize this report to bytes — the world snapshot an external agent
    /// reads, on the same kernel byte channel as [`crate::FrameReport`].
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut writer = BinaryWriter::new();
        SCHEMA.write_to(&mut writer);
        writer.write_u64(self.entities);
        writer.write_u64(self.systems);
        writer.into_bytes()
    }

    /// Decode a report previously produced by [`Self::to_bytes`]. Fails with
    /// [`axiom_kernel::KernelErrorCode::SchemaVersionMismatch`] for an
    /// incompatible major version, or a binary error for truncated/invalid data.
    pub fn from_bytes(bytes: &[u8]) -> KernelResult<Self> {
        let mut reader = BinaryReader::new(bytes);
        SchemaVersion::read_from(&mut reader)
            .and_then(|version| {
                SCHEMA
                    .is_compatible_with(version)
                    .then_some(())
                    .ok_or_else(|| {
                        axiom_kernel::KernelError::new(
                            axiom_kernel::KernelErrorScope::Binary,
                            axiom_kernel::KernelErrorCode::SchemaVersionMismatch,
                            "WorldReport schema major version is incompatible",
                        )
                    })
            })
            .and_then(|()| reader.read_u64())
            .and_then(|entities| {
                reader
                    .read_u64()
                    .map(|systems| WorldReport { entities, systems })
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_ecs::{EntityRegistry, WorldStep, WorldSystem};
    use axiom_kernel::KernelErrorCode;

    #[derive(Default)]
    struct Storage;

    struct Noop;
    impl WorldSystem<Storage> for Noop {
        fn run(&self, _: &WorldStep, _: &EntityRegistry, _: &mut Storage) {}
    }

    #[test]
    fn observe_captures_entity_and_system_counts() {
        use axiom_frame::FrameContext;

        let mut world: World<Storage> = World::new();
        world.register_system(Box::new(Noop));
        world.spawn();
        world.spawn();
        world.spawn();

        let frame = &crate::fixtures::active_engine_frames(1)[0];
        world.advance(0, &FrameContext::new(frame));

        let report = WorldReport::observe(&world);
        assert_eq!(report.entities(), 3);
        assert_eq!(report.systems(), 1);
    }

    #[test]
    fn empty_world_reports_zero() {
        let world: World<Storage> = World::new();
        let report = WorldReport::observe(&world);
        assert_eq!(report.entities(), 0);
        assert_eq!(report.systems(), 0);
    }

    #[test]
    fn round_trips_through_bytes() {
        let report = WorldReport {
            entities: 7,
            systems: 3,
        };
        let decoded = WorldReport::from_bytes(&report.to_bytes()).unwrap();
        assert_eq!(decoded, report);
        assert_eq!(decoded.entities(), 7);
        assert_eq!(decoded.systems(), 3);
    }

    #[test]
    fn observing_two_identical_worlds_is_deterministic() {
        let build = || {
            let mut world: World<Storage> = World::new();
            world.register_system(Box::new(Noop));
            world.spawn();
            WorldReport::observe(&world).to_bytes()
        };
        assert_eq!(build(), build());
    }

    #[test]
    fn truncation_at_every_prefix_is_err() {
        let bytes = WorldReport {
            entities: 1,
            systems: 1,
        }
        .to_bytes();
        for len in 0..bytes.len() {
            assert!(
                WorldReport::from_bytes(&bytes[..len]).is_err(),
                "truncated decode at len {len} must fail"
            );
        }
    }

    #[test]
    fn incompatible_schema_major_is_rejected() {
        let mut writer = axiom_kernel::BinaryWriter::new();
        SchemaVersion::new(SCHEMA.major() + 1, 0).write_to(&mut writer);
        let bytes = writer.into_bytes();
        assert_eq!(
            WorldReport::from_bytes(&bytes).unwrap_err().code(),
            KernelErrorCode::SchemaVersionMismatch
        );
    }
}
