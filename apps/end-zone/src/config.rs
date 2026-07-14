//! App-level configuration: the explicit deterministic seed and the fixed
//! simulation step. Simulation code never reads a wall clock; the only time is
//! the tick counter, and the only randomness is derived from [`EndZoneConfig::seed`].

/// Fixed simulation step: 60 Hz, in nanoseconds (matches the engine's
/// `RuntimeStep` convention used by the physics facade).
pub const FIXED_STEP_NANOS: u64 = 16_666_667;

/// Seconds per fixed step.
pub const DT: f32 = FIXED_STEP_NANOS as f32 / 1_000_000_000.0;

/// Players fielded per team in the showcase.
pub const PLAYERS_PER_TEAM: usize = 7;

/// Total players in the sim's fixed array (both teams).
pub const PLAYER_COUNT: usize = PLAYERS_PER_TEAM * 2;

/// The default explicit seed for the showcase.
pub const DEFAULT_SEED: u64 = 0x5EED_0E2D_0001;

/// Top-level app configuration. Everything deterministic hangs off `seed`;
/// presentation variation uses `seed ^ stable event id`, never a fresh source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EndZoneConfig {
    /// The explicit deterministic seed for this session.
    pub seed: u64,
}

impl Default for EndZoneConfig {
    fn default() -> Self {
        EndZoneConfig { seed: DEFAULT_SEED }
    }
}

impl EndZoneConfig {
    /// A config with an explicit seed.
    pub fn with_seed(seed: u64) -> Self {
        EndZoneConfig { seed }
    }
}
