//! The phase a registered [`crate::WorldSystem`] runs in.

/// When a [`crate::WorldSystem`] runs relative to the world's lifetime.
///
/// A [`crate::World`] advances its systems in two ordered phases on each active
/// [`crate::World::advance`]: every [`Startup`](Self::Startup) system runs
/// exactly once — on the world's first active advance — then every
/// [`Update`](Self::Update) system runs, on that advance and every subsequent
/// active one. Within a phase, systems run in registration order. This is the
/// labelled-schedule primitive the engine frontend's `add_systems(Startup, …)`
/// / `add_systems(Update, …)` is built on; it stays a pure function of the
/// advance tick, so the world remains replay-deterministic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SchedulePhase {
    /// Runs exactly once, on the world's first active advance, before any
    /// `Update` system.
    Startup,
    /// Runs on every active advance.
    Update,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_and_update_are_distinct_phases() {
        assert_ne!(SchedulePhase::Startup, SchedulePhase::Update);
    }
}
