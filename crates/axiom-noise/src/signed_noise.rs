//! [`SignedNoise`] — a signed noise sample in `[-1, 1]`, at double precision.

/// A coherent-noise sample in the closed interval `[-1, 1]`.
///
/// The third and last output type in this layer, and the axes that separate the
/// three are worth stating once:
///
/// | | range | precision |
/// |---|---|---|
/// | [`crate::NoiseValue`] | signed | `f32` |
/// | [`crate::UnitNoise`] | unsigned | `f64` |
/// | `SignedNoise` | signed | `f64` |
///
/// **Range** is a property of the basis, not a convention: a gradient basis
/// returns a dot product either side of zero, and a value basis interpolates
/// hashes that are already unsigned. Squeezing one into the other's type would
/// oblige every caller to remember which half of the range is really reachable.
///
/// **Precision** follows `axiom_math::Scalar`: `f32` for the basis that feeds
/// `axiom_field`'s WGSL compiler, whose CPU-to-GPU parity is measured at that
/// precision, and `f64` for the bake-time bases evaluated on the CPU to produce
/// texture tiles.
///
/// Total by construction, like its siblings: [`SignedNoise::from_signal`]
/// clamps into range and maps any non-finite input to `0.0`.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct SignedNoise(f64);

impl SignedNoise {
    /// Construct from a *computed* signal, clamping into `[-1, 1]` and mapping
    /// any non-finite result to `0.0`.
    pub fn from_signal(value: f64) -> Self {
        SignedNoise([0.0, value.clamp(-1.0, 1.0)][value.is_finite() as usize])
    }

    /// The underlying signal in `[-1, 1]`, at full precision.
    pub const fn get(self) -> f64 {
        self.0
    }

    /// The signal narrowed to the engine's interchange scalar. The one named
    /// narrowing point — see `axiom_math::DVec3::to_single`.
    pub fn get_single(self) -> f32 {
        self.0 as f32
    }

    /// Remapped into `[0, 1]`, the unsigned half-scale form a mask or a
    /// texture channel wants.
    ///
    /// Named rather than left to the call site because `x * 0.5 + 0.5` written
    /// by hand is where a sign convention gets flipped and nobody notices until
    /// a texture reads inverted.
    pub fn to_unit(self) -> crate::unit_noise::UnitNoise {
        crate::unit_noise::UnitNoise::from_signal(self.0 * 0.5 + 0.5)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passes_in_range_values_through() {
        assert_eq!(SignedNoise::from_signal(-0.75).get(), -0.75);
        assert_eq!(SignedNoise::from_signal(0.0).get(), 0.0);
        assert_eq!(SignedNoise::from_signal(1.0).get(), 1.0);
    }

    #[test]
    fn clamps_both_ends() {
        assert_eq!(SignedNoise::from_signal(2.0).get(), 1.0);
        assert_eq!(SignedNoise::from_signal(-2.0).get(), -1.0);
    }

    #[test]
    fn sanitizes_non_finite_to_zero() {
        assert_eq!(SignedNoise::from_signal(f64::NAN).get(), 0.0);
        assert_eq!(SignedNoise::from_signal(f64::INFINITY).get(), 0.0);
        assert_eq!(SignedNoise::from_signal(f64::NEG_INFINITY).get(), 0.0);
    }

    #[test]
    fn to_unit_maps_the_signed_range_onto_the_unit_one() {
        assert_eq!(SignedNoise::from_signal(-1.0).to_unit().get(), 0.0);
        assert_eq!(SignedNoise::from_signal(0.0).to_unit().get(), 0.5);
        assert_eq!(SignedNoise::from_signal(1.0).to_unit().get(), 1.0);
    }

    #[test]
    fn get_keeps_double_precision_and_get_single_is_the_one_narrowing() {
        let signal = 0.1_f64;
        let n = SignedNoise::from_signal(signal);
        assert_eq!(n.get(), signal);
        assert_ne!(f64::from(n.get_single()), signal);
        assert_eq!(n.get_single(), signal as f32);
    }

    #[test]
    fn samples_are_ordered() {
        assert!(SignedNoise::from_signal(-0.5) < SignedNoise::from_signal(0.5));
    }
}
