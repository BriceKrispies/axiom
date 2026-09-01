//! [`UnitNoise`] — an unsigned noise sample in `[0, 1]`, at double precision.

/// A coherent-noise sample, always in the closed interval `[0, 1]`.
///
/// The unsigned, double-precision counterpart to [`crate::NoiseValue`]. Two
/// differences, both deliberate:
///
/// - **Range.** [`crate::NoiseValue`] is signed `[-1, 1]` — the natural output
///   of a gradient basis, where the value is a dot product either side of zero.
///   A *value* basis interpolates hashes that are already unsigned, so its
///   output never crosses zero and squeezing it into a signed type would
///   misdescribe it. A caller that has to remember "this signed type is really
///   only ever positive here" is a caller that will eventually forget.
/// - **Precision.** `f64`, because this family is evaluated at bake time to
///   produce texture tiles and drive geometry, and because it is the oracle a
///   shader gets pinned against. `axiom_math::Scalar` states the rule: `f32` is
///   the *interchange* scalar; evaluate at the precision the domain requires and
///   narrow once, at the boundary. `UnitNoise::get_single` is that boundary.
///
/// The value is always produced by arithmetic (a weighted average of hashed
/// lattice corners), so the constructor is **total** — [`UnitNoise::from_signal`]
/// never fails: it clamps into range and maps any non-finite input to `0.0`.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct UnitNoise(f64);

impl UnitNoise {
    /// Construct a noise sample from a *computed* signal, clamping into
    /// `[0, 1]` and mapping any non-finite result (NaN / ±infinity) to `0.0`.
    ///
    /// Total by construction, mirroring [`crate::NoiseValue::from_signal`] and
    /// [`axiom_kernel::Ratio::finite_or_zero`]. The clamp is a no-op for every
    /// value this crate produces — a weighted average of samples already in
    /// `[0, 1)` cannot leave `[0, 1)` — so it is a guard on the boundary, not a
    /// transformation of the signal, and it costs the callers' golden values
    /// nothing.
    pub fn from_signal(value: f64) -> Self {
        UnitNoise([0.0, value.clamp(0.0, 1.0)][value.is_finite() as usize])
    }

    /// The underlying signal in `[0, 1]`, at full precision.
    pub const fn get(self) -> f64 {
        self.0
    }

    /// The signal narrowed to the engine's interchange scalar.
    ///
    /// The named boundary, for the same reason `axiom_math::DVec3::to_single`
    /// is one: "compute in `f64`, narrow once" is only auditable if the
    /// narrowing is a symbol you can search for rather than an `as f32` at a
    /// call site.
    pub fn get_single(self) -> f32 {
        self.0 as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passes_in_range_values_through() {
        assert_eq!(UnitNoise::from_signal(0.3).get(), 0.3);
        assert_eq!(UnitNoise::from_signal(0.0).get(), 0.0);
        assert_eq!(UnitNoise::from_signal(1.0).get(), 1.0);
    }

    #[test]
    fn clamps_both_ends() {
        assert_eq!(UnitNoise::from_signal(2.0).get(), 1.0);
        assert_eq!(UnitNoise::from_signal(-2.0).get(), 0.0);
    }

    #[test]
    fn sanitizes_non_finite_to_zero() {
        assert_eq!(UnitNoise::from_signal(f64::NAN).get(), 0.0);
        assert_eq!(UnitNoise::from_signal(f64::INFINITY).get(), 0.0);
        assert_eq!(UnitNoise::from_signal(f64::NEG_INFINITY).get(), 0.0);
    }

    /// The precision claim, asserted: a sample whose digits `f32` cannot hold
    /// survives `get` and is narrowed only by `get_single`.
    #[test]
    fn get_keeps_double_precision_and_get_single_is_the_one_narrowing() {
        let signal = 0.1_f64;
        let n = UnitNoise::from_signal(signal);
        assert_eq!(n.get(), signal);
        assert_ne!(f64::from(n.get_single()), signal);
        assert_eq!(n.get_single(), signal as f32);
    }

    #[test]
    fn samples_are_ordered() {
        assert!(UnitNoise::from_signal(0.25) < UnitNoise::from_signal(0.75));
    }
}
