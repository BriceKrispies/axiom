//! [`FbmConfig`] — the typed parameter set of a fractal Brownian motion field.

use axiom_kernel::Ratio;

use crate::frequency::Frequency;
use crate::lacunarity::Lacunarity;

/// The parameters of a fractal Brownian motion (FBM) field, all typed so no naked
/// scalar reaches the public noise API.
///
/// The four knobs are the classic FBM controls: how many `octaves` to sum, the
/// base [`Frequency`] of the first octave, the [`Lacunarity`] (per-octave
/// frequency growth), and the `gain` (per-octave amplitude decay) — typed as the
/// kernel's [`Ratio`] because it genuinely lives in `[0, 1]`.
///
/// [`FbmConfig::new`] fills the canonical defaults (lacunarity `2.0`, gain `0.5`);
/// the fields are public, so an advanced caller overrides a single knob with
/// struct-update syntax, e.g. `FbmConfig { gain, ..FbmConfig::new(octaves, freq) }`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FbmConfig {
    /// Number of octaves summed (clamped to at least one when sampled).
    pub octaves: u32,
    /// Base frequency of the first octave.
    pub frequency: Frequency,
    /// Per-octave frequency multiplier.
    pub lacunarity: Lacunarity,
    /// Per-octave amplitude multiplier, in `[0, 1]`.
    pub gain: Ratio,
}

impl FbmConfig {
    /// A config with the canonical FBM defaults: octave-[`Lacunarity::DOUBLING`]
    /// and a gain of `0.5` (each octave contributes half the amplitude of the
    /// previous). `0.5` is finite, so the total [`Ratio::finite_or_zero`] yields
    /// exactly `0.5`.
    pub fn new(octaves: u32, frequency: Frequency) -> Self {
        FbmConfig {
            octaves,
            frequency,
            lacunarity: Lacunarity::DOUBLING,
            gain: Ratio::finite_or_zero(0.5),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_fills_canonical_defaults() {
        let cfg = FbmConfig::new(5, Frequency::new(1.8).unwrap());
        assert_eq!(cfg.octaves, 5);
        assert_eq!(cfg.frequency.get(), 1.8);
        assert_eq!(cfg.lacunarity, Lacunarity::DOUBLING);
        assert_eq!(cfg.gain.get(), 0.5);
    }

    #[test]
    fn fields_support_struct_update_override() {
        let base = FbmConfig::new(3, Frequency::new(1.0).unwrap());
        let overridden = FbmConfig {
            gain: Ratio::finite_or_zero(0.25),
            ..base
        };
        assert_eq!(overridden.gain.get(), 0.25);
        assert_eq!(overridden.octaves, 3);
        assert_ne!(overridden, base);
    }
}
