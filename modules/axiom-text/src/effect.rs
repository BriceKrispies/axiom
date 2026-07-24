//! Deterministic, tick-driven per-glyph text effects.
//!
//! An effect is *data* evaluated at an explicit integer tick — never wall-clock.
//! Evaluation yields a per-glyph modification (offset, alpha, visibility) that the
//! glyph batch folds in; effects never mutate the canonical text content. Shake
//! derives its offset from stable integer hashing of `(seed, glyph, tick)`, so the
//! same state and tick produce byte-identical output.

use axiom_host::Pixels;
use axiom_kernel::Ratio;
use axiom_math::Vec2;

/// Which effect to apply. Fieldless so evaluation dispatches through a fixed
/// function table (no `match`); parameters live on [`TextEffect`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EffectKind {
    /// Reveal glyphs one at a time (typewriter).
    #[default]
    Reveal,
    /// Fade all glyphs in over the duration.
    Fade,
    /// Vertical sine wave across glyph columns.
    Wave,
    /// Deterministic per-glyph jitter.
    Shake,
    /// Uniform scale/alpha pulse (alpha only here).
    Pulse,
    /// Vertical bounce that settles.
    Bounce,
}

impl EffectKind {
    /// The stable byte discriminant.
    pub const fn raw(self) -> u8 {
        [0u8, 1, 2, 3, 4, 5][self as usize]
    }
    /// Recover from a byte.
    pub fn from_raw(raw: u8) -> Option<EffectKind> {
        [
            Self::Reveal,
            Self::Fade,
            Self::Wave,
            Self::Shake,
            Self::Pulse,
            Self::Bounce,
        ]
        .get(raw as usize)
        .copied()
    }
}

/// A configured effect. `speed`/`amplitude` are in glyphs-per-tick, pixels, or
/// radians-per-tick depending on the kind; `seed` salts the deterministic shake.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextEffect {
    /// The kind of effect.
    pub kind: EffectKind,
    /// Tick the effect begins at.
    pub start_tick: u64,
    /// Ticks the effect runs over (`0` = never-ending, for wave/shake).
    pub duration: u64,
    /// Rate parameter (glyphs/tick for reveal, radians/tick for wave/pulse), a
    /// dimensionless multiplier.
    pub speed: Ratio,
    /// Magnitude in pixels (for wave/shake/bounce).
    pub amplitude: Pixels,
    /// Salt for the deterministic shake hash.
    pub seed: u32,
}

impl TextEffect {
    /// A typewriter reveal at `speed` glyphs per tick.
    pub fn reveal(speed: Ratio) -> TextEffect {
        TextEffect {
            kind: EffectKind::Reveal,
            start_tick: 0,
            duration: 0,
            speed,
            amplitude: Pixels::new(0.0).expect("finite zero amplitude"),
            seed: 0,
        }
    }
}

/// The per-glyph result of evaluating an effect: an additive offset, a
/// multiplicative alpha, and a visibility flag. Internal — an effect's public
/// contribution is folded into the [`crate::GlyphInstance`], never exposed as a
/// naked scalar.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct GlyphMod {
    /// Additive pixel offset.
    pub offset: Vec2,
    /// Alpha multiplier in `0.0..=1.0`.
    pub alpha: f32,
    /// Whether the glyph is drawn at all.
    pub visible: bool,
}

impl GlyphMod {
    /// The identity modification (no change).
    pub const IDENTITY: GlyphMod = GlyphMod {
        offset: Vec2::ZERO,
        alpha: 1.0,
        visible: true,
    };

    /// Compose two modifications: offsets add, alphas multiply, visibility ANDs.
    pub fn combine(self, other: GlyphMod) -> GlyphMod {
        GlyphMod {
            offset: Vec2::new(
                self.offset.x + other.offset.x,
                self.offset.y + other.offset.y,
            ),
            alpha: self.alpha * other.alpha,
            visible: self.visible & other.visible,
        }
    }
}

/// Evaluate a stack of effects for glyph `index` (of `total`) at `tick`, folding
/// each into one [`GlyphMod`], applied in stable declaration order.
pub(crate) fn evaluate(effects: &[TextEffect], index: u32, total: u32, tick: u64) -> GlyphMod {
    effects.iter().fold(GlyphMod::IDENTITY, |acc, effect| {
        acc.combine(eval_one(*effect, index, total, tick))
    })
}

/// The elapsed ticks since an effect started (saturating; `0` before start).
fn elapsed(effect: TextEffect, tick: u64) -> u64 {
    tick.saturating_sub(effect.start_tick)
}

/// Evaluate one effect via the kind dispatch table.
fn eval_one(effect: TextEffect, index: u32, total: u32, tick: u64) -> GlyphMod {
    let table: [fn(TextEffect, u32, u32, u64) -> GlyphMod; 6] =
        [reveal, fade, wave, shake, pulse, bounce];
    table[effect.kind.raw() as usize](effect, index, total, tick)
}

/// A cheap deterministic hash of three integers to `0.0..1.0`.
fn hash01(a: u32, b: u32, c: u64) -> f32 {
    let mut h = 1469598103934665603u64 ^ u64::from(a);
    h = (h ^ u64::from(b)).wrapping_mul(1099511628211);
    h = (h ^ c).wrapping_mul(1099511628211);
    ((h >> 40) as f32) / (1u64 << 24) as f32
}

fn reveal(effect: TextEffect, index: u32, _total: u32, tick: u64) -> GlyphMod {
    let revealed = (elapsed(effect, tick) as f32 * effect.speed.get()) as u64;
    GlyphMod {
        visible: u64::from(index) < revealed.max(0),
        ..GlyphMod::IDENTITY
    }
}

fn fade(effect: TextEffect, _index: u32, _total: u32, tick: u64) -> GlyphMod {
    let t = (elapsed(effect, tick) as f32 * effect.speed.get()).clamp(0.0, 1.0);
    GlyphMod {
        alpha: t,
        ..GlyphMod::IDENTITY
    }
}

fn wave(effect: TextEffect, index: u32, _total: u32, tick: u64) -> GlyphMod {
    let phase = tick as f32 * effect.speed.get() + index as f32 * 0.5;
    GlyphMod {
        offset: Vec2::new(0.0, phase.sin() * effect.amplitude.get()),
        ..GlyphMod::IDENTITY
    }
}

fn shake(effect: TextEffect, index: u32, _total: u32, tick: u64) -> GlyphMod {
    let dx = hash01(effect.seed, index, tick) - 0.5;
    let dy = hash01(effect.seed ^ 0x9E37, index, tick) - 0.5;
    GlyphMod {
        offset: Vec2::new(dx * effect.amplitude.get(), dy * effect.amplitude.get()),
        ..GlyphMod::IDENTITY
    }
}

fn pulse(effect: TextEffect, _index: u32, _total: u32, tick: u64) -> GlyphMod {
    let a = 0.5 + 0.5 * (tick as f32 * effect.speed.get()).sin();
    GlyphMod {
        alpha: a,
        ..GlyphMod::IDENTITY
    }
}

fn bounce(effect: TextEffect, index: u32, _total: u32, tick: u64) -> GlyphMod {
    let phase = (tick as f32 * effect.speed.get() - index as f32 * 0.3).max(0.0);
    let settle = (-phase).exp();
    GlyphMod {
        offset: Vec2::new(0.0, -(phase.sin().abs()) * effect.amplitude.get() * settle),
        ..GlyphMod::IDENTITY
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn effect(
        kind: EffectKind,
        speed: f32,
        amplitude: f32,
        seed: u32,
        duration: u64,
    ) -> TextEffect {
        TextEffect {
            kind,
            start_tick: 0,
            duration,
            speed: Ratio::finite_or_zero(speed),
            amplitude: Pixels::new(amplitude).unwrap(),
            seed,
        }
    }

    #[test]
    fn reveal_progresses_with_tick_and_is_deterministic() {
        let e = [TextEffect::reveal(Ratio::finite_or_zero(1.0))];
        // At tick 2, glyphs 0 and 1 are revealed, glyph 2 is not.
        assert!(evaluate(&e, 0, 5, 2).visible);
        assert!(evaluate(&e, 1, 5, 2).visible);
        assert!(!evaluate(&e, 2, 5, 2).visible);
        // Same inputs → identical output.
        assert_eq!(evaluate(&e, 2, 5, 2), evaluate(&e, 2, 5, 2));
    }

    #[test]
    fn shake_is_deterministic_and_bounded() {
        let e = effect(EffectKind::Shake, 0.0, 4.0, 7, 0);
        let a = evaluate(&[e], 3, 10, 100);
        let b = evaluate(&[e], 3, 10, 100);
        assert_eq!(a, b, "same state+tick is byte-identical");
        assert!(a.offset.x.abs() <= 2.0, "bounded by amplitude/2");
        // Different tick → generally different offset.
        assert_ne!(evaluate(&[e], 3, 10, 101).offset, a.offset);
    }

    #[test]
    fn kinds_round_trip_and_compose() {
        [
            EffectKind::Reveal,
            EffectKind::Fade,
            EffectKind::Wave,
            EffectKind::Shake,
            EffectKind::Pulse,
            EffectKind::Bounce,
        ]
        .into_iter()
        .for_each(|k| assert_eq!(EffectKind::from_raw(k.raw()), Some(k)));
        assert_eq!(EffectKind::from_raw(9), None);
        // Fade + reveal compose: alpha from fade, visibility from reveal.
        let stack = [
            effect(EffectKind::Fade, 0.5, 0.0, 0, 0),
            TextEffect::reveal(Ratio::finite_or_zero(1.0)),
        ];
        let m = evaluate(&stack, 0, 3, 1);
        assert!((m.alpha - 0.5).abs() < 1e-6);
        assert!(m.visible);
    }

    #[test]
    fn every_kind_evaluates() {
        [
            EffectKind::Reveal,
            EffectKind::Fade,
            EffectKind::Wave,
            EffectKind::Shake,
            EffectKind::Pulse,
            EffectKind::Bounce,
        ]
        .into_iter()
        .for_each(|kind| {
            let m = evaluate(&[effect(kind, 0.2, 3.0, 1, 10)], 1, 4, 5);
            assert!(m.alpha.is_finite() & m.offset.x.is_finite() & m.offset.y.is_finite());
        });
    }
}
