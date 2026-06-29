//! The value-type vocabulary the [`crate::Draw2dApi`] particle surface (§10.1)
//! traffics in: the [`EmitterId`] handle it returns and the [`EmitterConfig`]
//! description it accepts. Pure value types — no behaviour lives here; the
//! particle *system* (the live field, its stepping) is private to the module
//! and is never exposed (particles are presentation-only and feed no sim-readable
//! getter).

use axiom_host::Rgba;
use axiom_kernel::{Meters, Ratio, Seconds};
use axiom_math::Vec2;

/// A particle emitter registered with [`crate::Draw2dApi::create_emitter`]. A
/// zero-based index into the builder's emitter table; only valid for the builder
/// that minted it. Carries no behaviour — it is the noun [`crate::Draw2dApi::emit`]
/// names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EmitterId(u32);

impl EmitterId {
    /// Construct from a raw zero-based index.
    pub const fn from_raw(raw: u32) -> Self {
        EmitterId(raw)
    }

    /// The underlying zero-based index.
    pub const fn raw(self) -> u32 {
        self.0
    }
}

/// A particle-emitter description (§10.1): the resolved, value-typed recipe a
/// burst is spawned from. Every scalar is a kernel quantity newtype, never a
/// naked float. A burst of `count` particles is spawned at the emit point, each
/// flying along the emit direction at `speed`, perturbed perpendicular to it by
/// up to `spread` of that speed (a deterministic per-particle jitter), pulled by
/// `gravity`, drawn as a `size`-wide quad whose colour lerps `color_start` →
/// `color_end` across its `lifetime`, on z-order `layer`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EmitterConfig {
    /// How many particles a single [`crate::Draw2dApi::emit`] spawns.
    pub count: u32,
    /// How long each particle lives (presentation seconds).
    pub lifetime: Seconds,
    /// Initial speed along the emit direction.
    pub speed: Meters,
    /// Perpendicular jitter as a fraction of `speed` (`0` = a clean jet).
    pub spread: Ratio,
    /// Constant acceleration applied each step (e.g. downward gravity).
    pub gravity: Vec2,
    /// The drawn quad's half-extent.
    pub size: Meters,
    /// Colour at birth (`age = 0`).
    pub color_start: Rgba,
    /// Colour at death (`age = lifetime`).
    pub color_end: Rgba,
    /// The z-order layer the emitted quads draw on.
    pub layer: i32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ratio(v: f32) -> Ratio {
        Ratio::new(v).unwrap()
    }

    fn rgba(v: f32) -> Rgba {
        Rgba::new(ratio(v), ratio(v), ratio(v), ratio(1.0))
    }

    #[test]
    fn emitter_id_round_trips() {
        assert_eq!(EmitterId::from_raw(3).raw(), 3);
        assert_eq!(EmitterId::from_raw(3), EmitterId::from_raw(3));
        assert_ne!(EmitterId::from_raw(3), EmitterId::from_raw(4));
        assert!(EmitterId::from_raw(0) < EmitterId::from_raw(1));
    }

    #[test]
    fn emitter_config_preserves_its_parts() {
        let config = EmitterConfig {
            count: 8,
            lifetime: Seconds::new(2.0).unwrap(),
            speed: Meters::new(5.0).unwrap(),
            spread: ratio(0.25),
            gravity: Vec2::new(0.0, -9.8),
            size: Meters::new(0.5).unwrap(),
            color_start: rgba(1.0),
            color_end: rgba(0.0),
            layer: 3,
        };
        assert_eq!(config.count, 8);
        assert_eq!(config.lifetime, Seconds::new(2.0).unwrap());
        assert_eq!(config.speed, Meters::new(5.0).unwrap());
        assert_eq!(config.spread, ratio(0.25));
        assert_eq!(config.gravity, Vec2::new(0.0, -9.8));
        assert_eq!(config.size, Meters::new(0.5).unwrap());
        assert_eq!(config.color_start, rgba(1.0));
        assert_eq!(config.color_end, rgba(0.0));
        assert_eq!(config.layer, 3);
    }
}
