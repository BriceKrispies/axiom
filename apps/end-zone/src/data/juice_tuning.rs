//! Presentation-effect tuning: the bounded lifetimes and clamped amplitudes
//! every juice effect is built from. Its own file because it is a distinct
//! authoring concern from the simulation's behaviour numbers — and because
//! `tuning.rs` is at the app's file-size budget.

/// Presentation-effect tuning: bounded lifetimes and clamped amplitudes for
/// every juice effect. All effects decay to exactly zero.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct JuiceTuning {
    /// Max simultaneous effects (fixed pool).
    pub max_effects: usize,
    /// Dust burst: particles, life, max radius, max amplitude.
    pub dust_particles: usize,
    pub dust_life_ticks: u32,
    pub dust_radius: f32,
    /// Impact ring life + max radius.
    pub ring_life_ticks: u32,
    pub ring_radius: f32,
    /// Speed streaks: count + life.
    pub streak_count: usize,
    pub streak_life_ticks: u32,
    /// Ball trail: sample count + spacing ticks.
    pub trail_points: usize,
    pub trail_spacing_ticks: u32,
    /// Catch flash life.
    pub flash_life_ticks: u32,
    /// Field wobble: max amplitude (yd) + life.
    pub field_wobble_amplitude: f32,
    pub field_wobble_life_ticks: u32,
    /// Player squash: max pose compression `0..=1` + life.
    pub squash_amplitude: f32,
    pub squash_life_ticks: u32,
    /// Multiplier on flash effects (catch flash, throw pulse) — the flash
    /// accessibility control; `0` spawns no flash effects at all.
    pub flash_scale: f32,
}

impl Default for JuiceTuning {
    fn default() -> Self {
        JuiceTuning {
            max_effects: 16,
            dust_particles: 10,
            dust_life_ticks: 34,
            dust_radius: 1.7,
            ring_life_ticks: 26,
            ring_radius: 2.2,
            streak_count: 6,
            streak_life_ticks: 16,
            trail_points: 14,
            trail_spacing_ticks: 2,
            flash_life_ticks: 18,
            field_wobble_amplitude: 0.16,
            field_wobble_life_ticks: 30,
            squash_amplitude: 0.35,
            squash_life_ticks: 18,
            flash_scale: 1.0,
        }
    }
}
