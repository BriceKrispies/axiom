//! [`NoiseValue`] — a signed unit noise sample in `[-1, 1]`.

/// A coherent-noise sample, always in the closed interval `[-1, 1]`.
///
/// This is the typed boundary where a raw `f32` becomes a *noise value*: the
/// output of [`crate::value_noise`] and [`crate::Fbm::sample`] is a bounded
/// signed unit, not an arbitrary float, so callers stop passing naked scalars
/// whose range they would have to guess. It follows the kernel's
/// [`axiom_kernel::Ratio`] shape, but with the noise-specific `[-1, 1]` clamp.
///
/// The value is always produced by arithmetic (a normalized sum of gradient dot
/// products), so the constructor is **total** — [`NoiseValue::from_signal`] never
/// fails: it clamps into range and maps any non-finite input to `0.0`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NoiseValue(f32);

impl NoiseValue {
    /// Construct a noise value from a *computed* signal, clamping into `[-1, 1]`
    /// and mapping any non-finite result (NaN / ±infinity) to `0.0`. Total by
    /// construction: the inputs are finite in practice, so the sanitizing arm is a
    /// defined fallback rather than an error path (mirrors
    /// [`axiom_kernel::Ratio::finite_or_zero`]).
    pub fn from_signal(value: f32) -> Self {
        NoiseValue([0.0, value.clamp(-1.0, 1.0)][value.is_finite() as usize])
    }

    /// The underlying signal in `[-1, 1]`.
    pub const fn get(self) -> f32 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passes_in_range_values_through() {
        assert_eq!(NoiseValue::from_signal(0.3).get(), 0.3);
        assert_eq!(NoiseValue::from_signal(0.0).get(), 0.0);
        assert_eq!(NoiseValue::from_signal(-0.75).get(), -0.75);
    }

    #[test]
    fn clamps_both_ends() {
        assert_eq!(NoiseValue::from_signal(2.0).get(), 1.0);
        assert_eq!(NoiseValue::from_signal(-2.0).get(), -1.0);
    }

    #[test]
    fn sanitizes_non_finite_to_zero() {
        assert_eq!(NoiseValue::from_signal(f32::NAN).get(), 0.0);
        assert_eq!(NoiseValue::from_signal(f32::INFINITY).get(), 0.0);
        assert_eq!(NoiseValue::from_signal(f32::NEG_INFINITY).get(), 0.0);
    }
}
