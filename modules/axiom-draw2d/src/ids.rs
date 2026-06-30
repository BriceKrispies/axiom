//! The value-type vocabulary the [`crate::Draw2dApi`] particle surface (§10.1)
//! traffics in: the [`EmitterId`] handle it returns and the [`EmitterConfig`]
//! description it accepts. Pure value types — no behaviour lives here; the
//! particle *system* (the live field, its stepping) is private to the module
//! and is never exposed (particles are presentation-only and feed no sim-readable
//! getter).

use axiom_host::{Rect, Rgba};
use axiom_kernel::{Meters, Ratio, Seconds};
use axiom_math::Vec2;

/// An inclusive `[min, max]` range of a dimensioned quantity `T` (a kernel
/// quantity newtype such as [`Seconds`] or [`Meters`]) — the value-typed form of
/// the contract's `[min, max]` emitter fields (SPEC-04 §10.1). A burst draws each
/// particle's lifetime / speed / size **deterministically** within its range; a
/// single scalar `v` is the degenerate range [`Range::exact`] (`[v, v]`), so a
/// fixed-value field needs no separate shape. Carries no behaviour — the
/// in-range pick lives behind the [`crate::Draw2dApi`] particle system, the one
/// place the deterministic source (the per-emit seed) is in hand.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Range<T> {
    min: T,
    max: T,
}

impl<T: Copy> Range<T> {
    /// A range spanning `min`..=`max`.
    pub const fn new(min: T, max: T) -> Self {
        Range { min, max }
    }

    /// The degenerate range `[value, value]` — a fixed value carried as a range,
    /// the backward-compatible form of a single scalar field.
    pub const fn exact(value: T) -> Self {
        Range {
            min: value,
            max: value,
        }
    }

    /// The lower endpoint.
    pub const fn min(self) -> T {
        self.min
    }

    /// The upper endpoint.
    pub const fn max(self) -> T {
        self.max
    }
}

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
/// flying along the emit direction at a `speed` **picked deterministically in its
/// range**, perturbed perpendicular to it by up to `spread` of that speed (a
/// deterministic per-particle jitter), pulled by `gravity`, drawn as a `size`-wide
/// quad (also a per-particle in-range pick) whose colour lerps `color_start` →
/// `color_end` across its `lifetime` (likewise picked in range), on z-order
/// `layer`. `lifetime` / `speed` / `size` are `[min, max]` [`Range`]s; a fixed
/// value is the degenerate [`Range::exact`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EmitterConfig {
    /// How many particles a single [`crate::Draw2dApi::emit`] spawns.
    pub count: u32,
    /// Each particle's lifetime range (presentation seconds); picked per particle.
    pub lifetime: Range<Seconds>,
    /// The initial-speed range along the emit direction; picked per particle.
    pub speed: Range<Meters>,
    /// Perpendicular jitter as a fraction of the picked speed (`0` = a clean jet).
    pub spread: Ratio,
    /// Constant acceleration applied each step (e.g. downward gravity).
    pub gravity: Vec2,
    /// The drawn quad's half-extent range; picked per particle.
    pub size: Range<Meters>,
    /// Colour at birth (`age = 0`).
    pub color_start: Rgba,
    /// Colour at death (`age = lifetime`).
    pub color_end: Rgba,
    /// The z-order layer the emitted quads draw on.
    pub layer: i32,
}

/// A flip-book sprite animation (§10.2): an ordered list of atlas sub-rect
/// `frames` played back at `fps` whole frames per second. A pure value recipe the
/// [`crate::Draw2dApi::sample_animation`] sampler reads — it carries no behaviour
/// itself (the sampling lives on the facade, like every other draw verb). `fps` is
/// an integer frame rate (the universal sprite-sheet convention): a flip-book
/// advances exactly one frame per `1/fps` seconds, so the rate is a frame *count*
/// per second, not a fractional scalar — which is why it is a `u32`, not a
/// dimensionless [`Ratio`].
#[derive(Debug, Clone, PartialEq)]
pub struct SpriteAnimation {
    /// The ordered atlas sub-rects, one per animation frame.
    pub frames: Vec<Rect>,
    /// The playback rate, in whole frames per second.
    pub fps: u32,
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
    fn range_new_exact_and_endpoints() {
        // A two-endpoint range carries both bounds; `exact` is the degenerate
        // [v, v] a scalar field collapses to.
        let span = Range::new(Seconds::new(1.0).unwrap(), Seconds::new(3.0).unwrap());
        assert_eq!(span.min(), Seconds::new(1.0).unwrap());
        assert_eq!(span.max(), Seconds::new(3.0).unwrap());
        let fixed = Range::exact(Meters::new(0.5).unwrap());
        assert_eq!(fixed.min(), Meters::new(0.5).unwrap());
        assert_eq!(fixed.max(), Meters::new(0.5).unwrap());
        assert_eq!(fixed, Range::new(Meters::new(0.5).unwrap(), Meters::new(0.5).unwrap()));
    }

    #[test]
    fn emitter_config_preserves_its_parts() {
        let config = EmitterConfig {
            count: 8,
            lifetime: Range::new(Seconds::new(1.0).unwrap(), Seconds::new(2.0).unwrap()),
            speed: Range::exact(Meters::new(5.0).unwrap()),
            spread: ratio(0.25),
            gravity: Vec2::new(0.0, -9.8),
            size: Range::new(Meters::new(0.5).unwrap(), Meters::new(1.5).unwrap()),
            color_start: rgba(1.0),
            color_end: rgba(0.0),
            layer: 3,
        };
        assert_eq!(config.count, 8);
        assert_eq!(config.lifetime.min(), Seconds::new(1.0).unwrap());
        assert_eq!(config.lifetime.max(), Seconds::new(2.0).unwrap());
        assert_eq!(config.speed, Range::exact(Meters::new(5.0).unwrap()));
        assert_eq!(config.spread, ratio(0.25));
        assert_eq!(config.gravity, Vec2::new(0.0, -9.8));
        assert_eq!(config.size.max(), Meters::new(1.5).unwrap());
        assert_eq!(config.color_start, rgba(1.0));
        assert_eq!(config.color_end, rgba(0.0));
        assert_eq!(config.layer, 3);
    }

    #[test]
    fn sprite_animation_preserves_its_parts() {
        // Mirrors `emitter_config_preserves_its_parts`: read the fields back
        // (comparing the `frames` Vec, not the whole struct) so the derived
        // SpriteAnimation eq/clone stay uninstantiated, exactly as EmitterConfig's.
        let frames = vec![
            Rect::new(Vec2::ZERO, Vec2::ONE),
            Rect::new(Vec2::new(1.0, 0.0), Vec2::ONE),
        ];
        let anim = SpriteAnimation {
            frames: frames.clone(),
            fps: 12,
        };
        assert_eq!(anim.frames, frames);
        assert_eq!(anim.fps, 12);
    }
}
