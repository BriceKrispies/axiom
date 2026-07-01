//! [`Fbm`] — a multi-octave fractal Brownian motion field with domain warp.
//!
//! An FBM sums several octaves of [`crate::gradient_noise`], scaling frequency by
//! the config [`crate::Lacunarity`] and amplitude by the config gain each octave,
//! then normalizes by the total amplitude so the result is a bounded
//! [`NoiseValue`] in `[-1, 1]`. Per-octave seeds and the three domain-warp channels
//! are derived through the kernel's [`StableHash`], so the field is deterministic
//! and platform-stable.

use axiom_kernel::StableHash;
use axiom_math::Vec3;

use crate::fbm_config::FbmConfig;
use crate::gradient_noise::raw_gradient_noise;
use crate::noise_value::NoiseValue;
use crate::warp_strength::WarpStrength;

/// Salts folded (with the field seed) into the three decorrelated domain-warp
/// offset channels. Distinct constants keep the x/y/z warp fields independent.
const WARP_SALT_X: u64 = 0x1111_1111_1111_1111;
const WARP_SALT_Y: u64 = 0x2222_2222_2222_2222;
const WARP_SALT_Z: u64 = 0x3333_3333_3333_3333;

/// A fractal Brownian motion field over world-space [`Vec3`] positions,
/// deterministic in `(seed, config, point)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fbm {
    seed: u64,
    config: FbmConfig,
}

impl Fbm {
    /// Build an FBM field from a 64-bit seed and a typed [`FbmConfig`].
    pub fn new(seed: u64, config: FbmConfig) -> Self {
        Fbm { seed, config }
    }

    /// Sample the field at `p`, returning a bounded [`NoiseValue`] in `[-1, 1]`.
    ///
    /// Sums `octaves` (at least one) octaves of gradient noise, scaling frequency
    /// by the lacunarity and amplitude by the gain each octave, then normalizes by
    /// the accumulated amplitude. Because the amplitude starts at `1.0` and at
    /// least one octave always runs, the total amplitude is always `>= 1.0`, so the
    /// normalization never divides by zero.
    pub fn sample(&self, p: Vec3) -> NoiseValue {
        let octaves = self.config.octaves.max(1);
        let gain = self.config.gain.get();
        let lacunarity = self.config.lacunarity.get();
        // Fold the octaves, carrying (sum, total_amplitude, amplitude, frequency).
        let (sum, total_amp, _amp, _freq) = (0..octaves).fold(
            (0.0f32, 0.0f32, 1.0f32, self.config.frequency.get()),
            |(sum, total_amp, amp, freq), octave| {
                let octave_seed = StableHash::of_words(&[self.seed, u64::from(octave)]).raw();
                let n = raw_gradient_noise(octave_seed, p.mul_scalar(freq));
                (
                    sum + n * amp,
                    total_amp + amp,
                    amp * gain,
                    freq * lacunarity,
                )
            },
        );
        NoiseValue::from_signal(sum / total_amp)
    }

    /// Domain-warped sample: displace `p` by a vector-valued field (three
    /// decorrelated FBM channels) scaled by `warp`, then sample the base field at
    /// the warped location. A [`WarpStrength`] of `0.0` produces a zero offset, so
    /// the result equals [`Fbm::sample`] at `p` — no special-case branch needed.
    pub fn sample_warped(&self, p: Vec3, warp: WarpStrength) -> NoiseValue {
        let qx = self.reseeded(WARP_SALT_X);
        let qy = self.reseeded(WARP_SALT_Y);
        let qz = self.reseeded(WARP_SALT_Z);
        let offset = Vec3::new(qx.sample(p).get(), qy.sample(p).get(), qz.sample(p).get())
            .mul_scalar(warp.get());
        self.sample(p.add(offset))
    }

    /// A copy of this field with a fresh seed derived (with `salt`) through the
    /// kernel digest — the decorrelated channel used by the domain warp.
    fn reseeded(&self, salt: u64) -> Self {
        Fbm {
            seed: StableHash::of_words(&[self.seed, salt]).raw(),
            config: self.config,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frequency::Frequency;
    use axiom_kernel::Ratio;

    fn fbm(seed: u64, octaves: u32, frequency: f32) -> Fbm {
        Fbm::new(
            seed,
            FbmConfig::new(octaves, Frequency::new(frequency).unwrap()),
        )
    }

    #[test]
    fn sample_is_deterministic() {
        let f = fbm(123, 5, 1.0);
        let p = Vec3::new(2.0, -1.0, 0.5);
        assert_eq!(f.sample(p), f.sample(p));
    }

    #[test]
    fn sample_stays_bounded() {
        let f = fbm(5150, 6, 0.9);
        for i in 0..3000 {
            let t = i as f32;
            let p = Vec3::new(t * 0.017, t * 0.029 - 5.0, -t * 0.011);
            let n = f.sample(p).get();
            assert!((-1.0..=1.0).contains(&n), "fbm out of range: {n}");
        }
    }

    #[test]
    fn octave_count_affects_output() {
        let p = Vec3::new(3.3, -2.2, 1.1);
        let one = fbm(77, 1, 1.5).sample(p);
        let many = fbm(77, 6, 1.5).sample(p);
        assert_ne!(one, many, "octave count should affect output");
    }

    #[test]
    fn zero_octaves_is_treated_as_one() {
        // `octaves.max(1)` means a zero-octave config samples a single octave —
        // identical to an explicit one-octave field at the same seed/frequency.
        let p = Vec3::new(0.37, 0.91, -0.22);
        let zero = Fbm::new(
            77,
            FbmConfig {
                octaves: 0,
                ..FbmConfig::new(1, Frequency::new(1.5).unwrap())
            },
        );
        let one = fbm(77, 1, 1.5);
        assert_eq!(zero.sample(p), one.sample(p));
    }

    #[test]
    fn sample_varies_across_space() {
        let f = fbm(2024, 4, 1.0);
        // Non-integer coordinates: gradient noise is exactly 0 at integer lattice
        // points, so integer samples would all read 0 and falsely match.
        let a = f.sample(Vec3::new(0.37, 0.91, -0.22));
        let b = f.sample(Vec3::new(5.13, 5.67, 5.41));
        assert_ne!(a, b, "fbm should vary across space");
    }

    #[test]
    fn warped_sample_is_deterministic() {
        let f = fbm(808, 4, 1.0);
        let p = Vec3::new(1.0, 2.0, 3.0);
        let w = WarpStrength::new(0.5).unwrap();
        assert_eq!(f.sample_warped(p, w), f.sample_warped(p, w));
    }

    #[test]
    fn zero_warp_equals_plain_sample() {
        let f = fbm(303, 4, 1.0);
        let p = Vec3::new(-1.5, 0.25, 4.0);
        let zero = WarpStrength::new(0.0).unwrap();
        assert_eq!(f.sample_warped(p, zero), f.sample(p));
    }

    #[test]
    fn non_zero_warp_changes_output() {
        let f = fbm(606, 5, 1.0);
        let p = Vec3::new(2.5, -3.5, 0.75);
        let plain = f.sample(p);
        let warped = f.sample_warped(p, WarpStrength::new(1.5).unwrap());
        assert!((-1.0..=1.0).contains(&warped.get()));
        assert_ne!(plain, warped, "non-zero warp should change output");
    }

    #[test]
    fn honors_overridden_gain() {
        // A different gain reshapes the octave sum (proves the config gain is read,
        // not a hardwired constant).
        let p = Vec3::new(1.7, -0.3, 2.9);
        let base = FbmConfig::new(5, Frequency::new(1.2).unwrap());
        let low_gain = FbmConfig {
            gain: Ratio::finite_or_zero(0.2),
            ..base
        };
        let a = Fbm::new(451, base).sample(p);
        let b = Fbm::new(451, low_gain).sample(p);
        assert_ne!(a, b, "overridden gain should change the field");
    }
}
