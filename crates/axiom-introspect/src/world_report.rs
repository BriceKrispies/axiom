//! A live observation of the ECS world.

use axiom_ecs::World;

/// A small, inspectable summary of the ECS world at an instant: how many
/// entities it holds and how many systems it advances.
///
/// This is the introspection layer's adapter over the world layer (Layer 05):
/// the world is foundational, and observability sits on top of it. It is a live
/// summary, not a serialized snapshot — the serialized agent channel is
/// [`crate::FrameReport`]; this answers "how big is the world right now".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WorldReport {
    entities: u64,
    systems: u64,
}

impl WorldReport {
    /// Observe a world, capturing its entity and system counts.
    pub fn observe<R>(world: &World<R>) -> Self {
        WorldReport {
            entities: world.len() as u64,
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use axiom_ecs::{EntityStore, WorldSystem};

    #[derive(Default)]
    struct Row;

    struct Noop;
    impl WorldSystem<Row> for Noop {
        fn run(&self, _: &mut EntityStore<Row>) {}
    }

    #[test]
    fn observe_captures_entity_and_system_counts() {
        use axiom_frame::FrameContext;

        let mut world: World<Row> = World::new();
        world.register_system(Box::new(Noop));
        world.spawn();
        world.spawn();
        world.spawn();

        // Advance once over an active frame so the registered system actually
        // runs, then observe.
        let frame = &crate::fixtures::active_engine_frames(1)[0];
        world.advance(&FrameContext::new(frame));

        let report = WorldReport::observe(&world);
        assert_eq!(report.entities(), 3);
        assert_eq!(report.systems(), 1);
    }

    #[test]
    fn empty_world_reports_zero() {
        let world: World<Row> = World::new();
        let report = WorldReport::observe(&world);
        assert_eq!(report.entities(), 0);
        assert_eq!(report.systems(), 0);
    }
}
